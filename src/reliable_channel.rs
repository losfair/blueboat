use std::{
  collections::HashMap,
  sync::{atomic::AtomicU64, Arc},
};

use crate::metadata::Metadata;
use anyhow::Result;
use erased_serde::Serialize as ErasedSerialize;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use smr::ipc_channel::ipc::{IpcError, IpcReceiver, IpcSender};
use tokio::sync::{oneshot, Semaphore};

#[derive(Serialize, Deserialize)]
pub struct ReliableChannelSeed {
  tx: IpcSender<RchReq>,
  rx: IpcReceiver<RchRsp>,
}

#[derive(Clone)]
pub struct ReliableChannel {
  inner: Arc<ReliableChannelInner>,
}

struct ReliableChannelInner {
  tx: Mutex<IpcSender<RchReq>>,
  id: AtomicU64,
  cb: Mutex<HashMap<u64, oneshot::Sender<RchRsp>>>,
}

impl ReliableChannelSeed {
  pub fn run_forever(self) -> ReliableChannel {
    let tx = Mutex::new(self.tx);
    let me = ReliableChannel {
      inner: Arc::new(ReliableChannelInner {
        tx,
        id: AtomicU64::new(1),
        cb: Mutex::new(HashMap::new()),
      }),
    };
    let me2 = me.clone();
    let rx = self.rx;
    std::thread::spawn(move || loop {
      let rsp = rx.recv().unwrap();
      let cb = me2
        .inner
        .cb
        .lock()
        .remove(&rsp.id)
        .expect("callback not found");
      let _ = cb.send(rsp);
    });
    me
  }
}

impl ReliableChannel {
  fn prepare_call(&self, req: impl RchReqBody + 'static) -> Result<oneshot::Receiver<RchRsp>> {
    let (tx, rx) = oneshot::channel();
    let id = self
      .inner
      .id
      .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    self.inner.cb.lock().insert(id, tx);
    self.inner.tx.lock().send(RchReq {
      id,
      body: Box::new(req),
    })?;
    Ok(rx)
  }

  pub async fn call<T: for<'a> Deserialize<'a>>(
    &self,
    req: impl RchReqBody + 'static,
  ) -> Result<T> {
    let rx = self.prepare_call(req)?;
    let res = rx
      .await
      .map_err(|_| anyhow::anyhow!("failed to receive response from reliable channel"))?;
    let out: Result<T, String> = bincode::deserialize(&res.bincode_body)?;
    out.map_err(|e| anyhow::anyhow!("reliable channel remote error: {}", e))
  }

  pub fn call_sync_slow<T: for<'a> Deserialize<'a>>(
    &self,
    req: impl RchReqBody + 'static,
  ) -> Result<T> {
    let rx = self.prepare_call(req)?;
    let res = std::thread::spawn(move || rx.blocking_recv())
      .join()
      .unwrap_or_else(|_| unreachable!())
      .map_err(|_| anyhow::anyhow!("failed to receive response from reliable channel"))?;
    let out: Result<T, String> = bincode::deserialize(&res.bincode_body)?;
    out.map_err(|e| anyhow::anyhow!("reliable channel remote error: {}", e))
  }
}

#[derive(Serialize, Deserialize)]
struct RchReq {
  id: u64,
  body: Box<dyn RchReqBody>,
}

#[async_trait::async_trait]
#[typetag::serde(tag = "type")]
pub trait RchReqBody: Send {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn ErasedSerialize>>;
}

#[derive(Serialize, Deserialize)]
struct RchRsp {
  id: u64,
  bincode_body: Vec<u8>,
}

pub fn create_reliable_channel(md: Arc<Metadata>) -> ReliableChannelSeed {
  let (req_tx, req_rx): (IpcSender<RchReq>, IpcReceiver<RchReq>) =
    smr::ipc_channel::ipc::channel().unwrap();
  let (rsp_tx, rsp_rx): (IpcSender<RchRsp>, IpcReceiver<RchRsp>) =
    smr::ipc_channel::ipc::channel().unwrap();
  std::thread::spawn(|| {
    let rt = tokio::runtime::Builder::new_multi_thread()
      .worker_threads(1)
      .enable_all()
      .build()
      .unwrap();
    let handle = rt.handle().clone();
    rt.block_on(async move {
      let concurrency = Arc::new(Semaphore::new(8));
      let rsp_tx = Arc::new(Mutex::new(rsp_tx));
      loop {
        let req = match req_rx.recv() {
          Ok(req) => req,
          Err(e) => {
            match e {
              IpcError::Disconnected => {}
              e => {
                tracing::error!(error = ?e, apppath = %md.path, appversion = %md.version, "failed to receive RC request from worker");
              }
            }
            break;
          },
        };
        let sem = concurrency.clone().acquire_owned().await.unwrap();
        let rsp_tx = rsp_tx.clone();
        let md = md.clone();
        handle.spawn(async move {
          let rsp = RchRsp {
            id: req.id,
            bincode_body: bincode::serialize(
              &req.body.handle(md).await.map_err(|e| format!("{}", e)),
            )
            .unwrap(),
          };
          let _ = rsp_tx.lock().send(rsp);
          drop(sem);
        });
      }
    });
  });
  ReliableChannelSeed {
    tx: req_tx,
    rx: rsp_rx,
  }
}
