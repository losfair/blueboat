use crate::engine::*;
use crate::error::*;
use crate::interface::*;
use crate::io::*;
use crate::runtime::{InstanceStatistics, Runtime};
use maplit::btreemap;
use std::collections::BTreeMap;
use rusty_v8 as v8;
use rusty_workers::types::*;
use std::cell::Cell;
use std::convert::TryFrom;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rand::Rng;
use tokio::sync::mpsc;
use std::io::Read;
use crate::isolate::{IsolateGeneration, IsolateGenerationBox};

const SAFE_AREA_SIZE: usize = 1048576;

thread_local! {
    static PROMISE_REJECTION: Cell<Option<String>> = Cell::new(None);
}

pub struct Instance {
    state: Option<InstanceState>,
}

#[derive(Copy, Clone, Debug)]
pub enum TimerControl {
    Start,
    Stop,
    Reset,
}

struct InstanceState {
    rt: tokio::runtime::Handle,
    worker_runtime: Arc<Runtime>,
    task_rx: mpsc::Receiver<Task>,

    /// Unpacked files in the worker bundle.
    files: BTreeMap<String, Arc<[u8]>>,

    script: Arc<[u8]>,

    timer_tx: tokio::sync::mpsc::UnboundedSender<TimerControl>,
    conf: Arc<WorkerConfiguration>,
    handle: WorkerHandle,
    io_waiter: Option<IoWaiter>,

    done: bool,

    fetch_response_channel: Option<tokio::sync::oneshot::Sender<ExecutionResult<ResponseObject>>>,
}

pub struct InstanceHandle {
    isolate_handle: v8::IsolateHandle,
    task_tx: mpsc::Sender<Task>,
    termination_reason: TerminationReasonBox,
    creation_generation: IsolateGeneration,
    current_generation: IsolateGenerationBox,
}

pub struct InstanceTimeControl {
    pub budget: Duration,
    pub timer_rx: mpsc::UnboundedReceiver<TimerControl>,
}

enum Task {
    Fetch(
        RequestObject,
        tokio::sync::oneshot::Sender<ExecutionResult<ResponseObject>>,
        IoScopeConsumer,
    ),
}

struct DoubleMleGuard {
    triggered_mle: bool,
}

impl Task {
    fn make_event(&self) -> ServiceEvent {
        match self {
            Task::Fetch(ref req, _, _) => ServiceEvent::Fetch(FetchEvent {
                request: req.clone(),
            }),
        }
    }
}

impl InstanceHandle {
    /// Properly check generation and perform remote termination.
    fn do_remote_termination(&self) {
        // Take the lock.
        let current_generation = self.current_generation.0.lock().unwrap();

        // Check while holding the lock.
        if *current_generation == self.creation_generation {
            self.isolate_handle.terminate_execution();
        }
    }

    pub async fn terminate_for_time_limit(&self) {
        tokio::task::block_in_place(|| {
            *self.termination_reason.0.lock().unwrap() = TerminationReason::TimeLimit;
            self.do_remote_termination();
        });
    }

    pub async fn fetch(&self, req: RequestObject) -> ExecutionResult<ResponseObject> {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let (_io_scope, io_scope_consumer) = IoScope::new();

        // Send fails if the instance has terminated
        self.task_tx
            .send(Task::Fetch(req, result_tx, io_scope_consumer))
            .await
            .map_err(|_| ExecutionError::NoSuchWorker)?;

        // This errors if the instance terminates without sending a response
        match result_rx.await {
            Ok(res) => res,
            Err(_) => {
                // Instance dropped sender without sending a response.
                // Most probably a runtime error.
                Err(ExecutionError::RuntimeThrowsException)
            }
        }
    }
}

impl Drop for InstanceHandle {
    fn drop(&mut self) {
        let term = || {
            self.do_remote_termination();
        };

        // If we are in a Tokio context, notify the runtime that we may block.
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(term);
        } else {
            term();
        }
    }
}

impl Instance {
    pub fn new(
        isolate: &mut v8::Isolate,
        rt: tokio::runtime::Handle,
        worker_runtime: Arc<Runtime>,
        worker_handle: WorkerHandle,
        bundle: Vec<u8>,
        conf: &WorkerConfiguration,
    ) -> GenericResult<(Self, InstanceHandle, InstanceTimeControl)> {
        // Unpack the bundle.
        let bundle_size = bundle.len();
        let mut archive = tar::Archive::new(std::io::Cursor::new(bundle));
        let mut files: BTreeMap<String, Arc<[u8]>> = BTreeMap::new();
        for entry in archive.entries().map_err(|_| GenericError::Other("bad bundle".into()))? {
            let mut entry = entry.map_err(|_| GenericError::Other("bad entry in bundle".into()))?;
            let path = entry.path().ok().and_then(|x| x.to_str().map(|x| x.to_string())).ok_or_else(|| GenericError::Other("bad path in bundle".into()))?;
            // TODO: Is `size` validated? Now we are validating it again to prevent memory DoS
            if entry.size() > bundle_size as u64 {
                return Err(GenericError::Other("entry size > bundle size".into()));
            }
            let mut data = vec![0; entry.size() as usize].into_boxed_slice();
            entry.read_exact(&mut data).map_err(|_| GenericError::Other("cannot read bundle".into()))?;
            files.insert(path, Arc::from(data));
        }
        drop(archive);

        // Lookup the script.
        let script = files.get("./index.js").ok_or_else(|| GenericError::Other("cannot find ./index.js in bundle".into()))?.clone();

        let termination_reason =
            TerminationReasonBox(Arc::new(Mutex::new(TerminationReason::Unknown)));

        // Initialize isolate.
        let isolate_ptr = isolate as *mut v8::Isolate;

        // Isolate initialization set. Needs cleanup.
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Auto);
        isolate.set_promise_reject_callback(on_promise_rejection);
        isolate.set_slot(Some(DoubleMleGuard {
            triggered_mle: false,
        }));
        isolate.set_slot(Some(termination_reason.clone()));
        isolate.add_near_heap_limit_callback(on_memory_limit_exceeded, isolate_ptr as _);

        // Allocate a channel of size 1. We don't want to put back pressure here.
        // The (async) sending side would block.
        let (task_tx, task_rx) = mpsc::channel(1);

        // TODO: unbounded ok here?
        let (timer_tx, timer_rx) = mpsc::unbounded_channel();

        let time_control = InstanceTimeControl {
            timer_rx,
            budget: Duration::from_millis(conf.executor.max_time_ms as u64),
        };

        let isolate_handle = isolate.thread_safe_handle();
        let generation = isolate.get_slot::<IsolateGenerationBox>().unwrap();

        let handle = InstanceHandle {
            isolate_handle,
            task_tx,
            termination_reason,
            creation_generation: *generation.0.lock().unwrap(),
            current_generation: generation.clone(),
        };
        let instance = Instance {
            state: Some(InstanceState {
                rt,
                worker_runtime,
                task_rx,
                files,
                script,
                timer_tx,
                conf: Arc::new(conf.clone()),
                handle: worker_handle,
                io_waiter: None,
                done: false,
                fetch_response_channel: None,
            }),
        };
        Ok((instance, handle, time_control))
    }

    pub fn cleanup(isolate: &mut v8::Isolate) {
        isolate.set_slot(Option::<DoubleMleGuard>::None);
        isolate.set_slot(Option::<TerminationReasonBox>::None);
        isolate.set_slot(Option::<InstanceState>::None);
    }

    fn compile<'s>(
        scope: &mut v8::HandleScope<'s>,
        script: &str,
    ) -> GenericResult<v8::Local<'s, v8::Script>> {
        let script = v8::String::new(scope, script)
            .ok_or_else(|| GenericError::ScriptInitException("script compilation failed".into()))?;
        let script = v8::Script::compile(scope, script, None)
            .ok_or_else(|| GenericError::ScriptInitException("script compilation failed".into()))?;
        Ok(script)
    }

    pub fn run(&mut self, context_scope: &mut v8::ContextScope<'_, v8::HandleScope<'_>>, ready_callback: impl FnOnce()) -> GenericResult<()> {
        let state = self.state.take().unwrap();
        let worker_runtime = state.worker_runtime.clone();

        let worker_handle = state.handle.clone();

        // Take a HandleScope and initialize the environment.
        {
            let scope = &mut v8::HandleScope::new(context_scope);
            let try_catch = &mut v8::TryCatch::new(scope);
            let scope: &mut v8::HandleScope<'_> = try_catch.as_mut();
            state.init_global_env(scope)?;

            // TODO: Compiler bombs?
            let script = std::str::from_utf8(&state.script).map_err(|_| GenericError::ScriptInitException("cannot decode script as utf-8 text".into()))?;
            let script = Self::compile(scope, script)?;

            // Notify that we are ready so that timing etc. can start
            ready_callback();

            scope.set_slot(Some(state));
            try_catch.check_on_init()?;

            // Now start the timer, since we are starting to run user code.
            InstanceState::get(try_catch).start_timer();

            script.run(try_catch.as_mut());
            try_catch.check_on_init()?;
        }
        info!("worker instance {} ready", worker_handle.id);

        // Wait for tasks.
        loop {
            update_stats(&worker_runtime, &worker_handle, context_scope);

            let scope = &mut v8::HandleScope::new(context_scope);
            let try_catch = &mut v8::TryCatch::new(scope);
            let scope: &mut v8::HandleScope<'_> = try_catch.as_mut();
            let state = InstanceState::get(scope);
            state.stop_timer();
            state.reset_timer();

            // Cleanup state
            state.io_waiter = None; // drop it
            state.done = false;

            let task = match state.task_rx.blocking_recv() {
                Some(x) => x,
                None => {
                    // channel closed
                    break;
                }
            };
            let event = task.make_event();
            let io_scope = state.populate_with_task(task)?;
            state.start_timer();

            // Start I/O processor (per-request).
            //
            // An `IoProcessor` receives the task's `IoScopeConsumer` as its argument, and stops when the
            // corresponding `IoScope` is dropped.
            let (io_waiter, io_processor) =
                IoWaiter::new(state.conf.clone(), state.worker_runtime.clone());
            state.rt.spawn(io_processor.run(io_scope));
            state.io_waiter = Some(io_waiter);

            let global = scope.get_current_context().global(scope);
            let callback_key = make_string(scope, "_dispatchEvent")?;
            let callback = global.get(scope, callback_key.into()).check()?;
            let callback = v8::Local::<'_, v8::Function>::try_from(callback)
                .map_err(|_| GenericError::Other("bad _dispatchEvent".into()))?;
            let recv = v8::undefined(scope);
            let event_js = native_to_js(scope, &event)?;
            callback.call(scope, recv.into(), &[event_js]);

            // Drive to completion.
            loop {
                match try_catch.check_on_task() {
                    Ok(()) => {}
                    Err(e) => {
                        if e.terminates_worker() {
                            InstanceState::try_send_fetch_response(try_catch, Err(e.clone()));
                            return Err(GenericError::Execution(e));
                        } else {
                            debug!("non-critical exception: {:?}", e);
                            try_catch.reset();
                            InstanceState::try_send_fetch_response(try_catch, Err(e));
                            break;
                        }
                    }
                }

                let scope = &mut v8::HandleScope::new(try_catch);
                let state = InstanceState::get(scope);

                if state.done {
                    break;
                }

                // Waiting for I/O now. Stop the timer.
                state.stop_timer();

                // A nice point to update statistics!
                update_stats(&worker_runtime, &worker_handle, scope);

                // Renew lifetime
                let state = InstanceState::get(scope);

                let (callback, data) = match state.io_waiter.as_mut().unwrap().wait() {
                    Some(x) => x,
                    None => {
                        // Doesn't necessarily need to terminate the instance but would need a lot of graceful
                        // handling on both the proxy side and the script side.
                        //
                        // So just terminate it now.
                        InstanceState::try_send_fetch_response(
                            scope,
                            Err(ExecutionError::IoTimeout),
                        );
                        return Err(GenericError::Execution(ExecutionError::IoTimeout));
                    }
                };
                state.start_timer();

                let callback = v8::Local::<'_, v8::Function>::new(scope, callback);
                let json_text = v8::String::new(scope, data.as_str()).check()?;
                let data = v8::json::parse(scope, json_text.into()).check()?;
                callback.call(scope, recv.into(), &[data]);
            }

            // Script marked itself as done but we haven't got any response.
            InstanceState::try_send_fetch_response(
                try_catch,
                Ok(ResponseObject {
                    status: 500,
                    ..Default::default()
                }),
            );
        }
        Ok(())
    }
}

impl InstanceState {
    fn get(isolate: &mut v8::Isolate) -> &mut Self {
        isolate.get_slot_mut::<Option<Self>>().unwrap().as_mut().unwrap()
    }

    fn io_waiter(&mut self) -> JsResult<&mut IoWaiter> {
        self.io_waiter.as_mut().ok_or_else(|| {
            JsError::new(JsErrorKind::Error, Some("io service not available".into()))
        })
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
            "_callService" => make_function(scope, call_service_callback)?.into(),
        };

        // Make sure our internal objects aren't overwritten by adding user props first.
        let user_props: Result<Vec<_>, GenericError> = self.conf.env.iter()
            .map(|(k, v)| Ok((k, make_string(scope, v)?.into())))
            .collect();
        add_props_to_object(scope, &global, user_props?)?;

        add_props_to_object(scope, &global, global_props)?;
        Ok(())
    }

    fn populate_with_task(&mut self, task: Task) -> GenericResult<IoScopeConsumer> {
        match task {
            Task::Fetch(_, res, io_scope) => {
                self.fetch_response_channel = Some(res);
                Ok(io_scope)
            }
        }
    }

    fn try_send_fetch_response(
        isolate: &mut v8::Isolate,
        res: ExecutionResult<ResponseObject>,
    ) -> bool {
        if let Some(ch) = InstanceState::get(isolate).fetch_response_channel.take() {
            ch.send(res).is_ok()
        } else {
            false
        }
    }
}

fn update_stats(worker_runtime: &Runtime, worker_handle: &WorkerHandle, scope: &mut v8::Isolate) {
    let mut stats = v8::HeapStatistics::default();
    scope.get_heap_statistics(&mut stats);
    worker_runtime.update_stats(
        worker_handle,
        InstanceStatistics {
            used_memory_bytes: stats.total_heap_size(),
        },
    );
}

pub extern "C" fn on_memory_limit_exceeded(
    data: *mut c_void,
    current_heap_limit: usize,
    _initial_heap_limit: usize,
) -> usize {
    let isolate = unsafe { &mut *(data as *mut v8::Isolate) };
    let double_mle_guard = isolate.get_slot_mut::<Option<DoubleMleGuard>>().unwrap().as_mut().unwrap();
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

extern "C" fn on_promise_rejection(_msg: v8::PromiseRejectMessage<'_>) {
    PROMISE_REJECTION.with(|x| x.set(Some("unhandled promise rejection".into())));
}

fn call_service_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    wrap_callback(scope, |scope| {
        let scope = &mut v8::HandleScope::new(scope);
        let call: ServiceCall = js_to_native(scope, args.get(0))?;
        match call {
            ServiceCall::Sync(call) => match call {
                SyncCall::Log(s) => {
                    debug!("log: {}", s);
                }
                SyncCall::Done => {
                    let state = InstanceState::get(scope);
                    state.done = true;
                }
                SyncCall::SendFetchResponse(res) => {
                    InstanceState::try_send_fetch_response(scope, Ok(res));
                }
                SyncCall::GetRandomValues(len) => {
                    if len > 65536 {
                        return Err(JsError::new(JsErrorKind::Error, Some("SyncCall::GetRandomValues invoked with length greater than 65536".into())));
                    }
                    let buf = v8::ArrayBuffer::new(scope, len);
                    let backing = buf.get_backing_store();
                    let mut rng = rand::thread_rng();
                    for byte in backing.iter() {
                        byte.set(rng.gen());
                    }
                    retval.set(buf.into());
                }
                SyncCall::GetFile(name) => {
                    let state = InstanceState::get(scope);
                    let file = state.files.get(&name).ok_or_else(|| JsError::new(
                        JsErrorKind::Error,
                        Some(format!("SyncCall::GetFile: file {} does not exist", name))
                    ))?.clone();
                    let buf = v8::ArrayBuffer::new(scope, file.len());
                    let backing = buf.get_backing_store();
                    for (i, byte) in backing.iter().enumerate() {
                        byte.set(file[i]);
                    }
                    retval.set(buf.into());
                }
            },
            ServiceCall::Async(call) => {
                let callback = v8::Local::<'_, v8::Function>::try_from(args.get(1))?;
                let callback = v8::Global::new(scope, callback);
                let state = InstanceState::get(scope);
                state.io_waiter()?.issue(false, call, callback)?;
            }
        }
        Ok(())
    })
}
