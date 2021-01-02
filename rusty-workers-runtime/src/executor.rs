use rusty_v8 as v8;
use rusty_workers::types::*;
use std::sync::mpsc;
use std::ffi::c_void;
use std::time::Duration;
use std::collections::BTreeMap;
use std::convert::TryFrom;

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
}

pub struct InstanceTimeControl {
    pub deadline_rx: tokio::sync::mpsc::UnboundedReceiver<Option<tokio::time::Instant>>,
}

pub struct Task {
    event: Event,
}

pub enum Event {
    Fetch(RequestObject, tokio::sync::oneshot::Sender<ResponseObject>),
}

struct DoubleMleGuard {
    triggered_mle: bool,
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

    fn build<'s>(self, scope: &mut v8::HandleScope<'s>) -> GenericResult<v8::Local<'s, v8::Value>> {
        match self {
            Event::Fetch(req, res) => {
                let json_text = v8::String::new(
                    scope,
                    serde_json::to_string(&req).map_err(|_| GenericError::Other("serde_json::to_string failed".into()))?.as_str(),
                ).ok_or_else(|| check_exception(scope))?.into();
                let obj = v8::json::parse(
                    scope,
                    json_text,
                ).ok_or_else(|| check_exception(scope))?;
                let event = v8::Object::new(scope);

                let key = v8::String::new(scope, "type").ok_or_else(|| check_exception(scope))?;
                let value = v8::String::new(scope, "fetch").ok_or_else(|| check_exception(scope))?;
                event.set(
                    scope,
                    key.into(),
                    value.into(),
                );

                let key = v8::String::new(scope, "request").ok_or_else(|| check_exception(scope))?;
                event.set(
                    scope,
                    key.into(),
                    obj,
                );
                // TODO: prototype
                return Ok(obj);
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

    fn add_event_listener_callback(
        scope: &mut v8::HandleScope,
        args: v8::FunctionCallbackArguments,
        mut _retval: v8::ReturnValue,
    ) {
        let key = args.get(0).to_rust_string_lossy(scope);
        let value = match v8::Local::try_from(args.get(1)) {
            Ok(x) => x,
            Err(e) => {
                // TODO: throw exception
                return;
            }
        };
        let global = v8::Global::new(scope, value);
        let state = InstanceState::get(scope);
        state.event_listeners.entry(key).or_insert(Vec::new()).push(global);
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
        let global = v8::ObjectTemplate::new(&mut isolate_scope);
        global.set(
            v8::String::new(&mut isolate_scope, "addEventListener").ok_or(GenericError::V8Unknown)?.into(),
            v8::FunctionTemplate::new(&mut isolate_scope, Self::add_event_listener_callback).into(),
        );

        let context = v8::Context::new_from_template(&mut isolate_scope, global);
        let mut context_scope = v8::ContextScope::new(&mut isolate_scope, context);
        let mut handle_scope = &mut v8::HandleScope::new(&mut context_scope);

        let script = Self::compile(handle_scope, &state.script)?;

        let worker_handle = state.handle.clone();
        handle_scope.set_slot(state);
        script.run(handle_scope).ok_or_else(|| check_exception(&mut handle_scope))?;
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
            for listener in listeners {
                let target = v8::Local::new(handle_scope, listener);
                target.call(handle_scope, recv.into(), args).ok_or_else(|| check_exception(handle_scope))?;
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
        isolate.terminate_execution();
    }
    return current_heap_limit + SAFE_AREA_SIZE;
}

fn check_exception(isolate: &mut v8::Isolate) -> GenericError {
    if isolate.is_execution_terminating() {
        GenericError::LimitsExceeded
    } else {
        GenericError::ScriptThrowsException
    }
}
