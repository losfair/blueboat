use crate::interface::{AsyncCall, AsyncCallV};
use crate::remote_buffer::*;
use crate::runtime::Runtime;
use anyhow::Result;
use rusty_v8 as v8;
use rusty_workers::kv::WorkerDataTransaction;
use rusty_workers::rpc::FetchServiceClient;
use rusty_workers::tarpc;
use rusty_workers::types::*;
use serde::{Deserialize, Serialize};
use slab::Slab;
use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::sync::Mutex as AsyncMutex;

const MAX_KV_KEY_SIZE: usize = 2048;
const MAX_KV_VALUE_SIZE: usize = 4 * 1024 * 1024;
const MAX_FETCH_REQUEST_BODY_SIZE: usize = 2 * 1024 * 1024;
const MAX_KV_SCAN_LIMIT: u32 = 100; // 100 * 2K = 200K max

pub struct IoWaiter {
    remaining_budget: u32,
    inflight: Slab<v8::Global<v8::Function>>,
    task: tokio::sync::mpsc::UnboundedSender<(usize, AsyncCall)>,
    result: crossbeam::channel::Receiver<BackToExecutorItem>,
    conf: Arc<WorkerConfiguration>,
    remote_buffer_set: RemoteBufferSet,
}

pub struct IoProcessor {
    task: tokio::sync::mpsc::UnboundedReceiver<(usize, AsyncCall)>,
    shared: Arc<IoProcessorSharedState>,
}

enum BackToExecutorItem {
    TaskResult((usize, String, Vec<RemoteBuffer>)),
    BufferCreation {
        size: usize,
        return_channel: oneshot::Sender<Option<RemoteBuffer>>,
    },
}

struct IoProcessorSharedState {
    conf: Arc<WorkerConfiguration>,
    worker_runtime: Arc<Runtime>,
    fetch_client: AsyncMutex<Option<FetchServiceClient>>,

    /// The current KV transaction.
    ///
    /// Don't allow multiple ongoing transactions for now, to prevent DoS.
    ongoing_txn: AsyncMutex<Option<WorkerDataTransaction>>,

    result: crossbeam::channel::Sender<BackToExecutorItem>,
}

/// An `IoScope` is a handle that a task sender holds to signal that I/O operations should
/// continue. When an `IoScope` is dropped, all ongoing I/O operations that depend on it
/// will be canceled.
pub struct IoScope {
    _kill: oneshot::Sender<()>,
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
    result: crossbeam::channel::Sender<BackToExecutorItem>,
    index: usize,
}

impl IoScope {
    pub fn new() -> (Self, IoScopeConsumer) {
        let (tx, rx) = oneshot::channel();
        (Self { _kill: tx }, IoScopeConsumer { kill: rx })
    }
}

impl IoWaiter {
    pub fn new(
        conf: Arc<WorkerConfiguration>,
        worker_runtime: Arc<Runtime>,
    ) -> (Self, IoProcessor) {
        let init_budget = conf.executor.max_io_per_request;
        let (result_tx, result_rx) = crossbeam::channel::unbounded();
        let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel();
        let waiter = IoWaiter {
            remaining_budget: init_budget,
            inflight: Slab::new(),
            task: task_tx,
            result: result_rx,
            conf: conf.clone(),
            remote_buffer_set: RemoteBufferSet::new(),
        };
        let processor = IoProcessor {
            task: task_rx,
            shared: Arc::new(IoProcessorSharedState {
                conf,
                worker_runtime,
                fetch_client: AsyncMutex::new(None),
                ongoing_txn: AsyncMutex::new(None),
                result: result_tx,
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

    pub fn wait(
        &mut self,
        scope: &mut v8::HandleScope<'_>,
    ) -> Option<(v8::Global<v8::Function>, String, Vec<RemoteBuffer>)> {
        let (index, result, buffers) = loop {
            match self.result.recv().ok()? {
                BackToExecutorItem::TaskResult(x) => break x,
                BackToExecutorItem::BufferCreation {
                    size,
                    return_channel,
                } => {
                    let buffer = self.remote_buffer_set.allocate(scope, size);
                    drop(return_channel.send(buffer));
                    continue;
                }
            }
        };
        let req = self.inflight.remove(index);

        // A nice point to garbage collect buffer set.
        self.remote_buffer_set.gc();

        Some((req, result, buffers))
    }
}

impl IoProcessor {
    async fn next(&mut self) -> Option<(AsyncCall, IoResponseHandle)> {
        let (index, task) = self.task.recv().await?;
        Some((
            task,
            IoResponseHandle {
                result: self.shared.result.clone(),
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
                            Ok((x, buffers)) => res.respond(format!("{{\"Ok\":{}}}", x), buffers),
                            Err(e) => {
                                debug!("io error: {:?}", e);
                                res.respond(format!("{{\"Err\":{}}}", "\"io error\""), vec![]);
                            }
                        }
                    }
                }
            });
        }
    }
}

impl IoProcessorSharedState {
    async fn allocate_arraybuffer(&self, len: usize) -> GenericResult<RemoteBuffer> {
        let (tx, rx) = oneshot::channel();
        drop(self.result.send(BackToExecutorItem::BufferCreation {
            size: len,
            return_channel: tx,
        }));

        match rx.await {
            Ok(Some(buf)) => Ok(buf),
            Ok(None) => Err(GenericError::Execution(ExecutionError::MemoryLimitExceeded)),
            Err(_) => Err(GenericError::Execution(ExecutionError::IoTimeout)), // Sender dropped
        }
    }

    async fn handle_task(self: Arc<Self>, task: AsyncCall) -> Result<(String, Vec<RemoteBuffer>)> {
        match task.v {
            AsyncCallV::SetTimeout(n) => {
                let dur = Duration::from_millis(n);
                tokio::time::sleep(dur).await;
                Ok(("null".into(), vec![]))
            }
            AsyncCallV::Fetch(mut req) => {
                let body = match task
                    .buffers
                    .get(0)
                    .ok_or_else(|| GenericError::Other("missing body".into()))?
                    .read_to_vec(MAX_FETCH_REQUEST_BODY_SIZE)
                {
                    Some(x) => x,
                    None => return Ok(mk_user_error("fetch request body too large")?),
                };
                req.body = HttpBody::Binary(body);

                let mut fetch_client_locked = self.fetch_client.lock().await;
                let mut fetch_client = if let Some(ref inner) = *fetch_client_locked {
                    inner.clone()
                } else {
                    let client = FetchServiceClient::connect(self.conf.fetch_service).await?;
                    *fetch_client_locked = Some(client.clone());
                    client
                };
                drop(fetch_client_locked);

                let mut fetch_result: Result<ResponseObject, String> =
                    fetch_client.fetch(tarpc::context::current(), req).await??;
                let buffers = if let Ok(ref mut v) = fetch_result {
                    match v.body {
                        HttpBody::Binary(ref mut body) if body.len() > 0 => {
                            // Large body
                            let buf = self.allocate_arraybuffer(body.len()).await?;
                            let backing: &[Cell<u8>] = buf.backing();
                            for (i, b) in backing.iter().enumerate() {
                                b.set(body[i]);
                            }
                            *body = vec![];
                            vec![buf]
                        }
                        _ => vec![],
                    }
                } else {
                    vec![]
                };
                Ok((serde_json::to_string(&fetch_result)?, buffers))
            }
            AsyncCallV::KvGet { namespace, lock } => {
                let key = match task
                    .buffers
                    .get(0)
                    .ok_or_else(|| GenericError::Other("missing key".into()))?
                    .read_to_vec(MAX_KV_KEY_SIZE)
                {
                    Some(x) => x,
                    None => return Ok(mk_user_error("key too large")?),
                };
                let namespace_id = match self.conf.kv_namespaces.get(&namespace) {
                    Some(id) => id,
                    None => return Ok(mk_user_error("namespace does not exist")?),
                };

                let result = if let Some(ref mut txn) = *self.ongoing_txn.lock().await {
                    if lock {
                        if txn
                            .lock_keys(namespace_id, std::iter::once(key.as_slice()))
                            .await?
                            == false
                        {
                            return Ok(mk_user_error("too many locks in this transaction")?);
                        }
                    }
                    txn.get(namespace_id, &key).await?
                } else {
                    let kv = match self.worker_runtime.kv() {
                        Some(x) => x,
                        None => return Ok(mk_user_error("kv disabled")?),
                    };

                    kv.worker_data_get(namespace_id, &key).await?
                };
                Ok(mk_user_ok(result)?)
            }
            AsyncCallV::KvPut { namespace } => {
                let key = match task
                    .buffers
                    .get(0)
                    .ok_or_else(|| GenericError::Other("missing key".into()))?
                    .read_to_vec(MAX_KV_KEY_SIZE)
                {
                    Some(x) => x,
                    None => return Ok(mk_user_error("key too large")?),
                };
                let value = match task
                    .buffers
                    .get(1)
                    .ok_or_else(|| GenericError::Other("missing value".into()))?
                    .read_to_vec(MAX_KV_VALUE_SIZE)
                {
                    Some(x) => x,
                    None => return Ok(mk_user_error("value too large")?),
                };
                let namespace_id = match self.conf.kv_namespaces.get(&namespace) {
                    Some(id) => id,
                    None => return Ok(mk_user_error("namespace does not exist")?),
                };
                if let Some(ref mut txn) = *self.ongoing_txn.lock().await {
                    txn.put(namespace_id, &key, value).await?;
                } else {
                    let kv = match self.worker_runtime.kv() {
                        Some(x) => x,
                        None => return Ok(mk_user_error("kv disabled")?),
                    };

                    kv.worker_data_put(namespace_id, &key, value).await?;
                }
                Ok(mk_user_ok(())?)
            }
            AsyncCallV::KvDelete { namespace } => {
                let key = match task
                    .buffers
                    .get(0)
                    .ok_or_else(|| GenericError::Other("missing key".into()))?
                    .read_to_vec(MAX_KV_KEY_SIZE)
                {
                    Some(x) => x,
                    None => return Ok(mk_user_error("key too large")?),
                };
                let namespace_id = match self.conf.kv_namespaces.get(&namespace) {
                    Some(id) => id,
                    None => return Ok(mk_user_error("namespace does not exist")?),
                };
                if let Some(ref mut txn) = *self.ongoing_txn.lock().await {
                    txn.delete(namespace_id, &key).await?
                } else {
                    let kv = match self.worker_runtime.kv() {
                        Some(x) => x,
                        None => return Ok(mk_user_error("kv disabled")?),
                    };

                    kv.worker_data_delete(namespace_id, &key).await?;
                }
                Ok(mk_user_ok(())?)
            }
            AsyncCallV::KvScan {
                namespace,
                limit,
                lock,
            } => {
                let start_key = match task
                    .buffers
                    .get(0)
                    .ok_or_else(|| GenericError::Other("missing start key".into()))?
                    .read_to_vec(MAX_KV_KEY_SIZE)
                {
                    Some(x) => x,
                    None => return Ok(mk_user_error("start_key too large")?),
                };
                let end_key = match task.buffers.get(1).map(|x| x.read_to_vec(MAX_KV_KEY_SIZE)) {
                    Some(Some(x)) => Some(x),
                    Some(None) => return Ok(mk_user_error("end_key too large")?),
                    None => None,
                };
                let namespace_id = match self.conf.kv_namespaces.get(&namespace) {
                    Some(id) => id,
                    None => return Ok(mk_user_error("namespace does not exist")?),
                };
                if limit > MAX_KV_SCAN_LIMIT {
                    return Ok(mk_user_error("limit is greater than MAX_KV_SCAN_LIMIT")?);
                }

                let keys = if let Some(ref mut txn) = *self.ongoing_txn.lock().await {
                    let keys = txn
                        .scan_keys(namespace_id, &start_key, end_key.as_deref(), limit)
                        .await?;
                    if lock {
                        if txn
                            .lock_keys(namespace_id, keys.iter().map(|x| x.as_slice()))
                            .await?
                            == false
                        {
                            return Ok(mk_user_error("too many locks in this transaction")?);
                        }
                    }
                    keys
                } else {
                    let kv = match self.worker_runtime.kv() {
                        Some(x) => x,
                        None => return Ok(mk_user_error("kv disabled")?),
                    };
                    kv.worker_data_scan_keys(namespace_id, &start_key, end_key.as_deref(), limit)
                        .await?
                };
                Ok(mk_user_ok(keys)?)
            }
            AsyncCallV::KvBeginTransaction => {
                let mut ongoing_txn = self.ongoing_txn.lock().await;

                // Don't consume txn_collector bandwidth if we can do it here.
                if let Some(x) = ongoing_txn.take() {
                    drop(x.rollback().await);
                }

                let kv = match self.worker_runtime.kv() {
                    Some(x) => x,
                    None => return Ok(mk_user_error("kv disabled")?),
                };

                let txn = kv.worker_data_begin_transaction().await?;
                *ongoing_txn = Some(txn);
                Ok(mk_user_ok(())?)
            }
            AsyncCallV::KvRollbackTransaction => {
                let mut ongoing_txn = self.ongoing_txn.lock().await;
                if let Some(x) = ongoing_txn.take() {
                    drop(x.rollback().await);
                    Ok(mk_user_ok(())?)
                } else {
                    Ok(mk_user_error("no ongoing transaction to rollback")?)
                }
            }
            AsyncCallV::KvCommitTransaction => {
                let mut ongoing_txn = self.ongoing_txn.lock().await;
                if let Some(x) = ongoing_txn.take() {
                    match x.commit().await {
                        Ok(committed) => Ok(mk_user_ok(committed)?),
                        Err(e) => {
                            warn!("commit error: {:?}", e);
                            Ok(mk_user_error("commit failed")?)
                        }
                    }
                } else {
                    Ok(mk_user_error("no ongoing transaction to commit")?)
                }
            }
        }
    }
}

impl IoResponseHandle {
    fn respond(self, data: String, buffers: Vec<RemoteBuffer>) {
        drop(
            self.result
                .send(BackToExecutorItem::TaskResult((self.index, data, buffers))),
        );
    }
}

fn mk_user_ok<T: serde::Serialize>(value: T) -> Result<(String, Vec<RemoteBuffer>)> {
    let value: Result<T, ()> = Ok(value);
    Ok((serde_json::to_string(&value)?, vec![]))
}

fn mk_user_error<T: serde::Serialize>(value: T) -> Result<(String, Vec<RemoteBuffer>)> {
    let value: Result<(), T> = Err(value);
    Ok((serde_json::to_string(&value)?, vec![]))
}
