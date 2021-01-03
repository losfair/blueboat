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
use crate::interface::*;

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

    done: bool,

    fetch_response_channel: Option<tokio::sync::oneshot::Sender<ResponseObject>>,
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

enum Task {
    Fetch(RequestObject, tokio::sync::oneshot::Sender<ResponseObject>),
}

struct DoubleMleGuard {
    triggered_mle: bool,
}

impl Task {
    fn make_event(&self) -> ServiceEvent {
        match self {
            Task::Fetch(ref req, _) => ServiceEvent::Fetch(FetchEvent {
                request: req.clone(),
            }),
        }
    }
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
        self.task_tx.try_send(Task::Fetch(req, result_tx)).map_err(|_| GenericError::TryAgain)?;
        result_rx.await.map_err(|_| GenericError::TryAgain)
    }
}

impl Drop for InstanceHandle {
    fn drop(&mut self) {
        self.isolate_handle.terminate_execution();
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
                done: false,
                fetch_response_channel: None,
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
            let mut scope = v8::HandleScope::new(&mut context_scope);
    
            state.init_global_env(&mut scope)?;
    
            let script = Self::compile(&mut scope, &state.script)?;
    
            scope.set_slot(state);
            script.run(&mut scope).check(&mut scope)?;
        }
        info!("worker instance {} ready", worker_handle.id);

        // Wait for tasks.
        loop {
            let mut scope = &mut v8::HandleScope::new(&mut context_scope);
            let state = InstanceState::get(scope);
            state.stop_timer();
            state.reset_timer();
            let task = match state.task_rx.recv() {
                Ok(x) => x,
                Err(_) => {
                    // channel closed
                    break;
                }
            };
            let event = task.make_event();
            state.populate_with_task(task)?;
            state.start_timer();

            let global = scope.get_current_context().global(scope);
            let callback_key = make_string(scope, "_dispatchEvent")?;
            let callback = global.get(scope, callback_key.into()).check(scope)?;
            let callback = v8::Local::<'_, v8::Function>::try_from(callback).map_err(|_| GenericError::Other("bad _dispatchEvent".into()))?;
            let recv = v8::undefined(scope);
            let event_js = native_to_js(scope, &event)?;
            callback.call(scope, recv.into(), &[event_js]).check(scope)?;
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
        let global_props = btreemap! {
            "_callService".into() => make_function(scope, call_service_callback)?.into(),
            "global".into() => global.into(),
        };
        add_props_to_object(scope, &global, global_props)?;
        Ok(())
    }

    fn populate_with_task(&mut self, task: Task) -> GenericResult<()> {
        match task {
            Task::Fetch(_, res) => {
                self.fetch_response_channel = Some(res);
            }
        }
        Ok(())
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
        terminate_with_reason(isolate, TerminationReason::MemoryLimit);
    }
    return current_heap_limit + SAFE_AREA_SIZE;
}

fn call_service_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        let scope = &mut v8::HandleScope::new(scope);
        let call: ServiceCall = js_to_native(scope, args.get(0))?;
        match call {
            ServiceCall::Sync(call) => {
                match call {
                    SyncCall::Log(s) => {
                        debug!("log: {}", s);
                    }
                    SyncCall::Done => {
                        let state = InstanceState::get(scope);
                        state.done = true;
                    }
                    SyncCall::SendFetchResponse(res) => {
                        let state = InstanceState::get(scope);
                        if let Some(ch) = state.fetch_response_channel.take() {
                            drop(ch.send(res));
                        }
                    }
                }
            }
            ServiceCall::Async(call) => {
                let callback = v8::Local::<'_, v8::Function>::try_from(args.get(1))?;
                match call {

                }
            }
        }
        Ok(())
    })
}
