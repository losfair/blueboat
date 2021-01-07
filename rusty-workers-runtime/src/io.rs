use crate::interface::AsyncCall;
use crate::runtime::Runtime;
use anyhow::Result;
use rusty_v8 as v8;
use rusty_workers::rpc::FetchServiceClient;
use rusty_workers::tarpc;
use rusty_workers::types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use slab::Slab;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::sync::Mutex as AsyncMutex;

pub struct IoWaiter {
    remaining_budget: u32,
    inflight: Slab<v8::Global<v8::Function>>,
    task: tokio::sync::mpsc::UnboundedSender<(usize, AsyncCall)>,
    result: std::sync::mpsc::Receiver<(usize, String)>,
    conf: Arc<WorkerConfiguration>,
}

pub struct IoProcessor {
    worker_runtime: Arc<Runtime>,
    task: tokio::sync::mpsc::UnboundedReceiver<(usize, AsyncCall)>,
    result: std::sync::mpsc::Sender<(usize, String)>,
    shared: Arc<IoProcessorSharedState>,
}

struct IoProcessorSharedState {
    conf: Arc<WorkerConfiguration>,
    fetch_client: AsyncMutex<Option<FetchServiceClient>>,
}

/// An `IoScope` is a handle that a task sender holds to signal that I/O operations should
/// continue. When an `IoScope` is dropped, all ongoing I/O operations that depend on it
/// will be canceled.
pub struct IoScope {
    kill: oneshot::Sender<()>,
}

/// The Rx side of an `IoScope`.
pub struct IoScopeConsumer {
    kill: oneshot::Receiver<()>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum IoTask {
    Ping,
}

struct IoResponseHandle {
    result: std::sync::mpsc::Sender<(usize, String)>,
    index: usize,
}

impl IoScope {
    pub fn new() -> (Self, IoScopeConsumer) {
        let (tx, rx) = oneshot::channel();
        (Self { kill: tx }, IoScopeConsumer { kill: rx })
    }
}

impl IoWaiter {
    pub fn new(
        conf: Arc<WorkerConfiguration>,
        worker_runtime: Arc<Runtime>,
    ) -> (Self, IoProcessor) {
        let init_budget = conf.executor.max_io_per_request;
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel();
        let waiter = IoWaiter {
            remaining_budget: init_budget,
            inflight: Slab::new(),
            task: task_tx,
            result: result_rx,
            conf: conf.clone(),
        };
        let processor = IoProcessor {
            task: task_rx,
            result: result_tx,
            worker_runtime,
            shared: Arc::new(IoProcessorSharedState {
                conf,
                fetch_client: AsyncMutex::new(None),
            }),
        };
        (waiter, processor)
    }

    pub fn issue(
        &mut self,
        count_budget: bool,
        task: AsyncCall,
        cb: v8::Global<v8::Function>,
    ) -> GenericResult<()> {
        if count_budget {
            if self.remaining_budget == 0 {
                return Err(GenericError::IoLimitExceeded);
            }
            self.remaining_budget -= 1;
        }

        if self.inflight.len() >= self.conf.executor.max_io_concurrency as usize {
            return Err(GenericError::IoLimitExceeded);
        }

        let index = self.inflight.insert(cb);
        match self.task.send((index, task)) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.inflight.remove(index);
                Err(GenericError::Other("io worker exited".into()))
            }
        }
    }

    pub fn wait(&mut self) -> Option<(v8::Global<v8::Function>, String)> {
        let (index, result) = self
            .result
            .recv_timeout(std::time::Duration::from_secs(30))
            .ok()?;
        let req = self.inflight.remove(index);
        Some((req, result))
    }
}

impl IoProcessor {
    async fn next(&mut self) -> Option<(AsyncCall, IoResponseHandle)> {
        let (index, task) = self.task.recv().await?;
        Some((
            task,
            IoResponseHandle {
                result: self.result.clone(),
                index,
            },
        ))
    }

    pub async fn run(mut self, mut scope: IoScopeConsumer) {
        use tokio::sync::watch;
        let (_kill_tx, kill_rx) = watch::channel(());

        loop {
            let next = tokio::select! {
                _ = &mut scope.kill => {
                    debug!("IoScope killed");
                    break;
                }
                x = self.next() => x
            };
            let (task, res) = match next {
                Some(x) => x,
                None => {
                    debug!("executor dropped IoWaiter");
                    break;
                }
            };
            let mut kill_rx = kill_rx.clone();
            let shared = self.shared.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = kill_rx.changed() => {
                        debug!("in-flight I/O operation killed");
                    }
                    ret = shared.handle_task(task) => {
                        match ret {
                            Ok(x) => res.respond(format!("{{\"Ok\":{}}}", x)),
                            Err(e) => {
                                debug!("io error: {:?}", e);
                                res.respond(format!("{{\"Err\":{}}}", "\"io error\""));
                            }
                        }
                    }
                }
            });
        }
    }
}

impl IoProcessorSharedState {
    async fn handle_task(self: Arc<Self>, task: AsyncCall) -> Result<String> {
        match task {
            AsyncCall::SetTimeout(n) => {
                let dur = Duration::from_millis(n);
                tokio::time::sleep(dur).await;
                Ok("null".into())
            }
            AsyncCall::Fetch(req) => {
                let mut fetch_client_locked = self.fetch_client.lock().await;
                let mut fetch_client = if let Some(ref inner) = *fetch_client_locked {
                    inner.clone()
                } else {
                    let client = FetchServiceClient::connect(self.conf.fetch_service).await?;
                    *fetch_client_locked = Some(client.clone());
                    client
                };
                drop(fetch_client_locked);

                let fetch_result: Result<ResponseObject, String> =
                    fetch_client.fetch(tarpc::context::current(), req).await??;
                Ok(serde_json::to_string(&fetch_result)?)
            }
        }
    }
}

impl IoResponseHandle {
    fn respond(self, data: String) {
        drop(self.result.send((self.index, data)));
    }
}
