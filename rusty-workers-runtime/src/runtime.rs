use rusty_v8 as v8;
use lru_time_cache::LruCache;
use rusty_workers::types::*;
use std::time::Duration;
use crate::executor::{Instance, InstanceHandle, InstanceTimeControl, TimerControl};
use tokio::sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock};
use std::sync::{Arc, Weak};
use tokio::sync::oneshot;
use rusty_workers::rpc::FetchServiceClient;
use rusty_workers::tarpc;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::config::Config;

pub struct Runtime {
    instances: AsyncRwLock<LruCache<WorkerHandle, WorkerState>>,
    statistics_update_tx: tokio::sync::mpsc::Sender<(WorkerHandle, InstanceStatistics)>,
    config: Config,
}

struct WorkerState {
    handle: Arc<InstanceHandle>,
    memory_bytes: AtomicUsize,
}

pub struct InstanceStatistics {
    pub used_memory_bytes: usize,
}

pub fn init() {
    let platform = v8::new_default_platform().unwrap();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
}

impl Runtime {
    pub fn new(config: Config) -> Arc<Self> {
        let (statistics_update_tx, statistics_update_rx) = tokio::sync::mpsc::channel(100);
        let max_num_of_instances = config.max_num_of_instances;
        let max_inactive_time_ms = config.max_inactive_time_ms;
        let rt = Arc::new(Runtime {
            instances: AsyncRwLock::new(LruCache::with_expiry_duration_and_capacity(Duration::from_millis(max_inactive_time_ms), max_num_of_instances)), // arbitrary choices
            statistics_update_tx,
            config,
        });
        let rt_weak = Arc::downgrade(&rt);
        tokio::spawn(statistics_update_worker(rt_weak, statistics_update_rx));
        rt
    }

    fn instance_thread(
        rt: tokio::runtime::Handle,
        worker_runtime: Arc<Runtime>,
        worker_handle: WorkerHandle,
        code: String,
        configuration: &WorkerConfiguration,
        result_tx: oneshot::Sender<Result<(InstanceHandle, InstanceTimeControl), GenericError>>,
    ) {
        match Instance::new(rt, worker_runtime, worker_handle.clone(), code, configuration) {
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
                        self.instances.write().await.remove(&worker_handle);

                        break;
                    }
                }
                _ = wait_until(deadline) => {
                    info!("worker {} timed out", worker_handle.id);

                    if let Some(handle) = self.instances.write().await.remove(&worker_handle) {
                        handle.handle.terminate_for_time_limit().await;
                    }

                    break;
                }
            }
        }
    }

    pub async fn list(&self) -> GenericResult<Vec<WorkerHandle>> {
        Ok(self.instances.read().await.peek_iter().map(|x| x.0.clone()).collect())
    }

    pub async fn terminate(&self, worker_handle: &WorkerHandle) -> GenericResult<()> {
        match self.instances.write().await.remove(&worker_handle) {
            Some(_) => Ok(()),
            None => Err(GenericError::NoSuchWorker),
        }
    }

    pub async fn fetch(&self, worker_handle: &WorkerHandle, req: RequestObject) -> GenericResult<ResponseObject> {
        // write() lock for LRU update
        let instance = self.instances.write().await
            .get(&worker_handle).map(|x| x.handle.clone()).ok_or_else(|| GenericError::NoSuchWorker)?;
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
            Self::instance_thread(rt, this, worker_handle_2, code, &configuration, result_tx)
        });
        let result = result_rx.await;
        match result {
            Ok(Ok((handle, timectl))) => {
                self.instances.write().await.insert(worker_handle.clone(), WorkerState {
                    handle: Arc::new(handle),
                    memory_bytes: AtomicUsize::new(0),
                });
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

    pub async fn load(&self) -> GenericResult<u16> {
        let instances = self.instances.read().await;
        let num_instances = instances.len();
        let total_memory: usize = instances.peek_iter().map(|(_, v)| v.memory_bytes.load(Ordering::Relaxed)).sum();
        drop(instances);

        let memory_usage = compute_usage_saturating(
            total_memory as f64,
            self.config.high_memory_threshold_bytes as f64,
            30000,
        );
        let instance_usage = compute_usage_saturating(
            num_instances as f64,
            self.config.max_num_of_instances as f64,
            30000,
        );
        Ok(memory_usage + instance_usage)
    }

    pub fn update_stats(&self, worker_handle: &WorkerHandle, stats: InstanceStatistics) {
        // Allow send to fail since this isn't critical
        drop(self.statistics_update_tx.try_send((worker_handle.clone(), stats)));
    }

    /// This function is added to avoid too long drop time in extreme cases.
    pub async fn lru_gc(&self) {
        // iter() calls remove_expired()
        drop(self.instances.write().await.iter());
    }
}

async fn wait_until(deadline: Option<tokio::time::Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        futures::future::pending::<()>().await;
    }
}

async fn statistics_update_worker(rt: Weak<Runtime>, mut rx: tokio::sync::mpsc::Receiver<(WorkerHandle, InstanceStatistics)>) {
    loop {
        let (handle, stats) = if let Some(x) = rx.recv().await {
            x
        } else {
            break;
        };
        let rt = if let Some(x) = rt.upgrade() {
            x
        } else {
            break;
        };
        let instances = rt.instances.read().await;
        if let Some(state) = instances.peek(&handle) {
            state.memory_bytes.store(stats.used_memory_bytes, Ordering::Relaxed);
        }
    }
}

fn compute_usage_saturating(used: f64, total: f64, mul: u16) -> u16 {
    let usage = used / total;
    let usage = if !usage.is_finite() || usage > 1.0 {
        1.0
    } else {
        usage
    };

    (usage * (mul as f64)) as u16
}
