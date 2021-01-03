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

#[derive(Copy, Clone, Debug)]
pub enum TimerControl {
    Start,
    Stop,
    Reset,
}

struct InstanceState {
    task_rx: mpsc::Receiver<Task>,
    script: String,
    timer_tx: tokio::sync::mpsc::UnboundedSender<TimerControl>,
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
    pub budget: Duration,
    pub timer_rx: tokio::sync::mpsc::UnboundedReceiver<TimerControl>,
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
                let obj = native_to_js(scope, req)?;
                let fetch_event_constructor = TypeCache::get(scope).fetch_event.clone().into_local(scope);
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
        let (timer_tx, timer_rx) = tokio::sync::mpsc::unbounded_channel();

        let time_control = InstanceTimeControl {
            timer_rx,
            budget: Duration::from_millis(conf.max_time_ms as u64),
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
                timer_tx,
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
        state.start_timer();
        let mut isolate_scope = v8::HandleScope::new(&mut *self.isolate);
        let context = v8::Context::new(&mut isolate_scope);
        let mut context_scope = v8::ContextScope::new(&mut isolate_scope, context);

        let worker_handle = state.handle.clone();

        // Take a HandleScope and initialize the environment.
        {
            let mut handle_scope = v8::HandleScope::new(&mut context_scope);
    
            let type_cache = TypeCache::new(&mut handle_scope)?;
            handle_scope.set_slot(type_cache);
    
            state.init_global_env(&mut handle_scope)?;
    
            let script = Self::compile(&mut handle_scope, &state.script)?;
    
            handle_scope.set_slot(state);
            script.run(&mut handle_scope).check(&mut handle_scope)?;
        }
        info!("worker instance {} ready", worker_handle.id);

        // Wait for tasks.
        loop {
            let mut handle_scope = &mut v8::HandleScope::new(&mut context_scope);
            let state = InstanceState::get(handle_scope);
            state.stop_timer();
            state.reset_timer();
            let task = match state.task_rx.recv() {
                Ok(x) => x,
                Err(_) => {
                    // channel closed
                    break;
                }
            };
            state.start_timer();

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

    fn start_timer(&self) {
        drop(self.timer_tx.send(TimerControl::Start));
    }

    fn stop_timer(&self) {
        drop(self.timer_tx.send(TimerControl::Stop));
    }

    fn reset_timer(&self) {
        drop(self.timer_tx.send(TimerControl::Reset));
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

    fn get_constructor<'s, M: FnOnce(&Self) -> &v8::Global<v8::Function>>(scope: &mut v8::HandleScope<'s>, mapper: M) -> v8::Local<'s, v8::Function> {
        mapper(Self::get(scope)).clone().into_local(scope)
    }

    fn ensure_instance_of<'s, 'i, 'j, M: FnOnce(&Self) -> &v8::Global<v8::Function>>(
        scope: &mut v8::HandleScope<'s>,
        mapper: M,
        obj: v8::Local<'j, v8::Object>,
        class_name: &'static str,
    ) -> GenericResult<()> {
        let constructor = Self::get_constructor(scope, mapper);
        if !is_instance_of(scope, constructor, obj)? {
            Err(GenericError::Typeck { expected: class_name.into() })
        } else {
            Ok(())
        }
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
        let value = v8::Local::try_from(args.get(1))?;
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
        let this = args.this();
        let mut data = ResponseObject::default();

        let body = args.get(0);
        let init = args.get(1);

        data.status = 200;
        if !init.is_undefined() {
            let init = v8::Local::<'_, v8::Object>::try_from(init)?;

            let key = make_string(scope, "status")?;
            let maybe_status = init.get(scope, key.into()).check(scope)?;
            if !maybe_status.is_undefined() {
                data.status = u16::try_from(maybe_status.uint32_value(scope).check(scope)?)
                    .map_err(|_| JsError::new(JsErrorKind::Error, Some("status code out of bounds".into())))?;
            }
        }
        
        if !body.is_undefined() {
            let body = v8::Local::<'_, v8::String>::try_from(body)?;
            data.body = body.to_rust_string_lossy(scope).into();
        }
;
        let key = make_string(scope, "_data")?;
        let value = native_to_js(scope, &data)?;
        this.create_data_property(scope, key.into(), value.into()).check(scope)?;

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
        let response = v8::Local::<'_, v8::Object>::try_from(args.get(0))?;
        TypeCache::ensure_instance_of(scope, |x| &x.response, response, "Response")?;
        let key = make_string(scope, "_data")?;
        let response_data = response.get(scope, key.into()).check(scope)?;
        let response_data = js_to_native(scope, response_data)?;
        let event = scope.get_slot_mut::<Task>().map(|x| &mut x.event);
        match event {
            Some(Event::Fetch(_, ref mut res)) => {
                let res = res.0.take().ok_or_else(|| JsError::new(JsErrorKind::Error, Some("respondWith: cannot respond twice".into())))?;
                drop(res.send(response_data));
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
