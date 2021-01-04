use rusty_v8 as v8;
use lru_time_cache::LruCache;
use rusty_workers::types::*;
use std::time::Duration;
use crate::executor::{Instance, InstanceHandle, InstanceTimeControl, TimerControl};
use tokio::sync::Mutex as AsyncMutex;
use std::sync::Arc;
use tokio::sync::oneshot;

pub struct Runtime {
    instances: AsyncMutex<LruCache<WorkerHandle, Arc<InstanceHandle>>>,
}

pub fn init() {
    let platform = v8::new_default_platform().unwrap();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
}

impl Runtime {
    pub fn new() -> Arc<Self> {
        Arc::new(Runtime {
            instances: AsyncMutex::new(LruCache::with_expiry_duration_and_capacity(Duration::from_secs(600), 500)), // arbitrary choices
        })
    }

    fn instance_thread(
        rt: tokio::runtime::Handle,
        worker_handle: WorkerHandle,
        code: String,
        configuration: &WorkerConfiguration,
        result_tx: oneshot::Sender<Result<(InstanceHandle, InstanceTimeControl), GenericError>>,
    ) {
        match Instance::new(rt, worker_handle.clone(), code, &configuration.executor) {
            Ok((instance, handle, timectl)) => {
                let run_result = instance.run(move || {
                    drop(result_tx.send(Ok((handle, timectl))))
                });
                match run_result {
                    Ok(()) => {
                        info!("worker instance {} exited", worker_handle.id);
                    }
                    Err(e) => {
                        info!("worker instance {} exited with error: {:?}", worker_handle.id, e);
                    }
                }
            }
            Err(e) => {
                drop(result_tx.send(Err(e)));
            }
        }
    }
    async fn monitor_task(self: Arc<Self>, worker_handle: WorkerHandle, mut timectl: InstanceTimeControl) {
        let mut deadline = None;
        let initial_budget = timectl.budget;

        loop {
            tokio::select! {
                op = timectl.timer_rx.recv() => {
                    if let Some(op) = op {
                        match op {
                            TimerControl::Start => {
                                deadline = Some(tokio::time::Instant::now() + timectl.budget);
                            }
                            TimerControl::Stop => {
                                let now = tokio::time::Instant::now();
                                if let Some(deadline) = deadline {
                                    // Restore unused time budget
                                    timectl.budget = if now > deadline { Duration::from_millis(0) } else { deadline - now };
                                    debug!("remaining time budget: {:?}", timectl.budget);
                                }
                                deadline = None;
                            }
                            TimerControl::Reset => {
                                timectl.budget = initial_budget;
                            }
                        }
                    } else {
                        // instance thread stopped
                        info!("stopping monitor for worker {}", worker_handle.id);

                        // May fail if removed by LRU policy / other code
                        self.instances.lock().await.remove(&worker_handle);

                        break;
                    }
                }
                _ = wait_until(deadline) => {
                    info!("worker {} timed out", worker_handle.id);

                    if let Some(handle) = self.instances.lock().await.remove(&worker_handle) {
                        handle.terminate_for_time_limit().await;
                    }

                    break;
                }
            }
        }
    }

    pub async fn list(&self) -> GenericResult<Vec<WorkerHandle>> {
        Ok(self.instances.lock().await.iter().map(|x| x.0.clone()).collect())
    }

    pub async fn terminate(&self, worker_handle: &WorkerHandle) -> GenericResult<()> {
        match self.instances.lock().await.remove(&worker_handle) {
            Some(x) => Ok(()),
            None => Err(GenericError::NoSuchWorker),
        }
    }

    pub async fn fetch(&self, worker_handle: &WorkerHandle, req: RequestObject) -> GenericResult<ResponseObject> {
        let instance = self.instances.lock().await.get(&worker_handle).cloned().ok_or_else(|| GenericError::NoSuchWorker)?;
        instance.fetch(req).await
    }

    pub async fn spawn(self: &Arc<Self>, _appid: String, code: String, configuration: &WorkerConfiguration) -> GenericResult<WorkerHandle> {
        let (result_tx, result_rx) = oneshot::channel();
        let worker_handle = WorkerHandle::generate();
        let this = self.clone();
        let worker_handle_2 = worker_handle.clone();
        let configuration = configuration.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            Self::instance_thread(rt, worker_handle_2, code, &configuration, result_tx)
        });
        let result = result_rx.await;
        match result {
            Ok(Ok((handle, timectl))) => {
                self.instances.lock().await.insert(worker_handle.clone(), Arc::new(handle));
                tokio::spawn(self.clone().monitor_task(worker_handle.clone(), timectl));
                Ok(worker_handle)
            }
            Ok(Err(e)) => {
                Err(e)
            }
            Err(_) => {
                // result_tx dropped: initialization failed.
                Err(GenericError::ScriptCompileException)
            }
        }
    }
}

async fn wait_until(deadline: Option<tokio::time::Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        futures::future::pending::<()>().await;
    }
}