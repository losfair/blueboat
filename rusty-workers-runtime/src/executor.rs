use rusty_v8 as v8;
use rusty_workers::types::*;
use std::sync::mpsc;
use std::ffi::c_void;
use std::time::Duration;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::sync::{Arc, Mutex};
use crate::error::*;
use maplit::btreemap;
use crate::engine::*;

const SAFE_AREA_SIZE: usize = 1048576;

pub struct Instance {
    isolate: Box<v8::OwnedIsolate>,
    state: Option<InstanceState>,
}

struct InstanceState {
    task_rx: mpsc::Receiver<Task>,
    script: String,
    deadline_tx: tokio::sync::mpsc::UnboundedSender<Option<tokio::time::Instant>>,
    conf: ExecutorConfiguration,
    handle: WorkerHandle,

    event_listeners: BTreeMap<String, Vec<v8::Global<v8::Function>>>,
}

pub struct InstanceHandle {
    isolate_handle: v8::IsolateHandle,
    task_tx: mpsc::SyncSender<Task>,
    termination_reason: TerminationReasonBox,
}

pub struct InstanceTimeControl {
    pub deadline_rx: tokio::sync::mpsc::UnboundedReceiver<Option<tokio::time::Instant>>,
}

struct Task {
    event: Event,
}

struct FetchResponseChannel(Option<tokio::sync::oneshot::Sender<ResponseObject>>);

enum Event {
    Fetch(RequestObject, FetchResponseChannel),
}

struct DoubleMleGuard {
    triggered_mle: bool,
}

struct TypeCache {
    fetch_event: v8::Global<v8::Function>,
    request: v8::Global<v8::Function>,
    response: v8::Global<v8::Function>,
}

impl InstanceHandle {
    pub async fn terminate_for_time_limit(&self) {
        tokio::task::block_in_place(|| {
            *self.termination_reason.0.lock().unwrap() = TerminationReason::TimeLimit;
        });
        self.isolate_handle.terminate_execution();
    }

    pub async fn fetch(&self, req: RequestObject) -> GenericResult<ResponseObject> {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        self.task_tx.try_send(Task { event: Event::Fetch(req, FetchResponseChannel(Some(result_tx))) }).map_err(|_| GenericError::TryAgain)?;
        result_rx.await.map_err(|_| GenericError::TryAgain)
    }
}

impl Drop for InstanceHandle {
    fn drop(&mut self) {
        self.isolate_handle.terminate_execution();
    }
}

impl Event {
    fn name(&self) -> &'static str {
        match self {
            Event::Fetch(_, _) => "fetch",
        }
    }

    fn build<'s>(&self, scope: &mut v8::HandleScope<'s>) -> GenericResult<v8::Local<'s, v8::Value>> {
        match self {
            Event::Fetch(ref req, _) => {
                let json_text = v8::String::new(
                    scope,
                    serde_json::to_string(req).map_err(|_| GenericError::Other("serde_json::to_string failed".into()))?.as_str(),
                ).check(scope)?.into();
                let obj = v8::json::parse(
                    scope,
                    json_text,
                ).check(scope)?;
                let fetch_event_constructor = TypeCache::get(scope).fetch_event.clone();
                let fetch_event_constructor = v8::Local::new(scope, fetch_event_constructor);
                let event = fetch_event_constructor.new_instance(scope, &[]).check(scope)?;

                let key = v8::String::new(scope, "type").check(scope)?;
                let value = v8::String::new(scope, "fetch").check(scope)?;
                event.set(
                    scope,
                    key.into(),
                    value.into(),
                );

                let key = v8::String::new(scope, "request").check(scope)?;
                event.set(
                    scope,
                    key.into(),
                    obj,
                );
                return Ok(event.into());
            }
        }
    }
}

impl Instance {
    pub fn new(worker_handle: WorkerHandle, script: String, conf: &ExecutorConfiguration) -> GenericResult<(Self, InstanceHandle, InstanceTimeControl)> {
        let params = v8::Isolate::create_params()
            .heap_limits(0, conf.max_memory_mb as usize * 1048576);
        let mut isolate = Box::new(v8::Isolate::new(params));
        let isolate_ptr = &mut *isolate as *mut v8::OwnedIsolate;

        isolate.set_slot(DoubleMleGuard {
            triggered_mle: false,
        });

        let termination_reason = TerminationReasonBox(Arc::new(Mutex::new(TerminationReason::Unknown)));
        isolate.set_slot(termination_reason.clone());

        isolate.add_near_heap_limit_callback(
            on_memory_limit_exceeded,
            isolate_ptr as _,
        );

        let (task_tx, task_rx) = mpsc::sync_channel(128); // TODO: backlog size
        let (deadline_tx, deadline_rx) = tokio::sync::mpsc::unbounded_channel();

        let time_control = InstanceTimeControl {
            deadline_rx,
        };
        let handle = InstanceHandle {
            isolate_handle: isolate.thread_safe_handle(),
            task_tx,
            termination_reason,
        };
        let instance = Instance {
            isolate,
            state: Some(InstanceState {
                task_rx,
                script,
                deadline_tx,
                conf: conf.clone(),
                handle: worker_handle,
                event_listeners: BTreeMap::new(),
            }),
        };
        Ok((instance, handle, time_control))
    }

    fn compile<'s>(scope: &mut v8::HandleScope<'s>, script: &str) -> GenericResult<v8::Local<'s, v8::Script>> {
        let script = v8::String::new(scope, script).ok_or(GenericError::V8Unknown)?;
        let script = v8::Script::compile(scope, script, None).ok_or(GenericError::Executor("cannot compile script".into()))?;
        Ok(script)
    }

    pub fn run(mut self) -> GenericResult<()> {
        let mut state = self.state.take().unwrap();

        // Init resources
        state.renew_timeout();
        let mut isolate_scope = v8::HandleScope::new(&mut *self.isolate);
        let context = v8::Context::new(&mut isolate_scope);
        let mut context_scope = v8::ContextScope::new(&mut isolate_scope, context);
        let mut handle_scope = &mut v8::HandleScope::new(&mut context_scope);

        let type_cache = TypeCache::new(handle_scope)?;
        handle_scope.set_slot(type_cache);

        state.init_global_env(handle_scope)?;

        let script = Self::compile(handle_scope, &state.script)?;

        let worker_handle = state.handle.clone();
        handle_scope.set_slot(state);
        script.run(handle_scope).check(&mut handle_scope)?;
        info!("worker instance {} ready", worker_handle.id);

        loop {
            let state = InstanceState::get(handle_scope);
            state.clear_timeout();
            let task = match state.task_rx.recv() {
                Ok(x) => x,
                Err(_) => {
                    // channel closed
                    break;
                }
            };
            state.renew_timeout();

            let event_name = task.event.name();
            let listeners = state.event_listeners.get(event_name).map(|x| x.clone()).unwrap_or(Vec::new());
            let recv = v8::undefined(handle_scope);
            let args = &[task.event.build(handle_scope)?];
            handle_scope.set_slot(task);
            for listener in listeners {
                let target = v8::Local::new(handle_scope, listener);
                target.call(handle_scope, recv.into(), args).check(handle_scope)?;
            }
        }
        Ok(())
    }
}

impl InstanceState {
    fn get(isolate: &mut v8::Isolate) -> &mut Self {
        isolate.get_slot_mut::<Self>().unwrap()
    }

    fn renew_timeout(&self) {
        let timeout = Duration::from_millis(self.conf.max_time_ms as u64);
        drop(self.deadline_tx.send(Some(tokio::time::Instant::now() + timeout)));
    }

    fn clear_timeout(&self) {
        drop(self.deadline_tx.send(None));
    }

    /// Builds the global object.
    fn init_global_env<'s>(&self, scope: &mut v8::HandleScope<'s>) -> GenericResult<()> {
        let global = scope.get_current_context().global(scope);
        let console_props = btreemap! {
            "log".into() => make_function(scope, console_log_callback)?.into(),
        };
        let console_obj = make_object(scope)?;
        add_props_to_object(scope, &console_obj, console_props)?;

        let global_props = btreemap! {
            "addEventListener".into() => make_function(scope, add_event_listener_callback)?.into(),
            "console".into() => console_obj.into(),
            "Response".into() => TypeCache::get(scope).response.clone().into_local(scope).into(),
        };
        add_props_to_object(scope, &global, global_props)?;
        Ok(())
    }
}

impl TypeCache {
    fn get(isolate: &mut v8::Isolate) -> &mut Self {
        isolate.get_slot_mut::<Self>().unwrap()
    }

    fn new<'s>(scope: &mut v8::HandleScope<'s>) -> GenericResult<Self> {
        let props = btreemap! {
            "respondWith".to_string() => make_function(scope, fetch_event_respond_with_callback)?.into(),
        };
        let fetch_event = make_persistent_class(scope, fetch_event_constructor_callback, props)?;
        
        let request = make_persistent_class(scope, request_constructor_callback, btreemap! {

        })?;

        let response = make_persistent_class(scope, response_constructor_callback, btreemap! {

        })?;

        Ok(Self {
            fetch_event,
            request,
            response,
        })
    }
}

extern "C" fn on_memory_limit_exceeded(data: *mut c_void, current_heap_limit: usize, _initial_heap_limit: usize) -> usize {
    let isolate = unsafe {
        &mut *(data as *mut v8::OwnedIsolate)
    };
    let double_mle_guard = isolate.get_slot_mut::<DoubleMleGuard>().unwrap();
    if double_mle_guard.triggered_mle {
        // Proceed as this isn't fatal
        error!("double mle detected. safe area too small?");
    } else {
        // Execution may not terminate immediately if we are in native code. So allocate some "safe area" here.
        double_mle_guard.triggered_mle = true;

        let termination_reason = isolate.get_slot_mut::<TerminationReasonBox>().unwrap();
        *termination_reason.0.lock().unwrap() =  TerminationReason::MemoryLimit;

        isolate.terminate_execution();
    }
    return current_heap_limit + SAFE_AREA_SIZE;
}

fn add_event_listener_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        let key = args.get(0).to_rust_string_lossy(scope);
        let value = v8::Local::try_from(args.get(1)).map_err(|_| JsError::type_error())?;
        let global = v8::Global::new(scope, value);
        let state = InstanceState::get(scope);
        debug!("addEventListener: {}", key);
        state.event_listeners.entry(key).or_insert(Vec::new()).push(global);
        Ok(())
    });
}

fn request_constructor_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        Ok(())
    })
}

fn response_constructor_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        Ok(())
    })
}

fn fetch_event_constructor_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        Ok(())
    })
}

fn fetch_event_respond_with_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        let response = v8::Local::<'_, v8::Object>::try_from(args.get(0)).map_err(|_| JsError::type_error())?;
        let constructor = TypeCache::get(scope).response.clone().into_local(scope);
        if !is_instance_of(scope, constructor, response).map_err(|_| JsError::error())? {
            return Err(JsError::new(JsErrorKind::TypeError, Some("respondWith: must be a response".into())));
        }
        let event = scope.get_slot_mut::<Task>().map(|x| &mut x.event);
        match event {
            Some(Event::Fetch(_, ref mut res)) => {
                let res = res.0.take().ok_or_else(|| JsError::new(JsErrorKind::Error, Some("respondWith: cannot respond twice".into())))?;
                drop(res.send(ResponseObject::default()));
                Ok(())
            }
            None => {
                Err(JsError::error())
            }
        }
    })
}

fn console_log_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        let text = args.get(0).to_rust_string_lossy(scope);
        debug!("console.log: {}", text);
        Ok(())
    })
}
