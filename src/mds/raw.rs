use std::{
  io::Cursor,
  sync::{atomic::AtomicBool, Arc},
};

use anyhow::Result;
use ed25519_dalek::{Keypair, Signer};
use futures::{
  stream::{SplitSink, SplitStream},
  SinkExt, StreamExt,
};
use parking_lot::Mutex as PMutex;
use prost::Message;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
  net::TcpStream,
  sync::{oneshot, Mutex},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::backoff::random_backoff_fast;

use super::keycodec::{decode_path, encode_path};

mod proto {
  include!(concat!(env!("OUT_DIR"), "/mds.rs"));
}

type StreamTy = WebSocketStream<MaybeTlsStream<TcpStream>>;
type SinkTy = SplitSink<StreamTy, tokio_tungstenite::tungstenite::Message>;

#[derive(Error, Debug)]
#[error("ws connection closed")]
struct WsClose;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TriStateCheck<T> {
  Absent,
  Any,
  Value(T),
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TriStateSet<T> {
  Delete,
  Preserve,
  Value(T),
  WithVersionstampedKey { value: T },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixListOptions {
  pub reverse: bool,
  pub want_value: bool,
  pub limit: u32,
  pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareAndSetResult {
  pub versionstamp: Option<String>,
}

pub struct RawMds {
  url: String,
  stream: StreamTy,
  mux_width: u32,
}

#[derive(Error, Debug)]
#[error("retryable mds error: {description}")]
pub struct RetryableMdsError {
  description: String,
}

struct BrokenGuard {
  broken: Arc<AtomicBool>,
}

impl Drop for BrokenGuard {
  fn drop(&mut self) {
    self
      .broken
      .store(true, std::sync::atomic::Ordering::Relaxed);
  }
}

#[derive(Clone)]
pub struct RawMdsHandle {
  req_tx: flume::Sender<(proto::Request, oneshot::Sender<Result<proto::Response>>)>,
  broken: Arc<AtomicBool>,
}

impl RawMds {
  pub async fn open(url: &str, mux_width: u32) -> Result<Self> {
    let (stream, _) = tokio_tungstenite::connect_async(url).await?;
    Ok(RawMds {
      url: url.into(),
      stream,
      mux_width,
    })
  }

  async fn recv_msg<T: Message + Default>(&mut self) -> Result<T> {
    let ws_msg = self.stream.next().await.ok_or(WsClose)??;
    if !ws_msg.is_binary() {
      anyhow::bail!("websocket message is not binary");
    }

    let buf = ws_msg.into_data();
    Ok(T::decode(&mut Cursor::new(buf.as_slice()))?)
  }

  pub async fn authenticate(
    &mut self,
    store: &str,
    keypair: &Keypair,
  ) -> Result<proto::LoginResponse> {
    let challenge: proto::LoginChallenge = self.recv_msg().await?;
    log::info!("server version: {}", challenge.version);
    let sig = keypair.sign(&challenge.challenge);
    let login = proto::Login {
      store: store.to_string(),
      public_key: keypair.public.to_bytes().to_vec(),
      mux_width: self.mux_width,
      signature: sig.to_bytes().to_vec(),
    };
    self
      .stream
      .send(tokio_tungstenite::tungstenite::Message::Binary(
        login.encode_to_vec(),
      ))
      .await?;
    let res: proto::LoginResponse = self.recv_msg().await?;
    if res.ok {
      Ok(res)
    } else {
      anyhow::bail!("login failed")
    }
  }

  async fn send_request(
    sink: &Mutex<SinkTy>,
    lane_id: u32,
    mux_rx: &mut tokio::sync::mpsc::Receiver<proto::Response>,
    mut req: proto::Request,
  ) -> Result<proto::Response> {
    req.lane = lane_id;
    {
      let mut sink = sink.lock().await;
      sink
        .send(tokio_tungstenite::tungstenite::Message::Binary(
          req.encode_to_vec(),
        ))
        .await?;
    }
    let res = match mux_rx.recv().await {
      Some(res) => res,
      None => anyhow::bail!("mds mux rx closed"),
    };
    Ok(res)
  }

  async fn recv_task(
    url: String,
    mut stream: SplitStream<StreamTy>,
    mux_tx: Vec<tokio::sync::mpsc::Sender<proto::Response>>,
    _broken: BrokenGuard,
  ) {
    loop {
      let msg = stream.next().await;
      let msg = match msg {
        Some(Ok(x)) => x,
        Some(Err(e)) => {
          log::error!("mds ws error ({}): {}", url, e);
          break;
        }
        None => {
          log::info!("mds ws closed ({})", url);
          break;
        }
      };
      if !msg.is_binary() {
        log::debug!("ignoring non-binary message ({})", url);
        continue;
      }

      let buf = msg.into_data();
      let res = match proto::Response::decode(&mut Cursor::new(buf.as_slice())) {
        Ok(x) => x,
        Err(e) => {
          log::error!("mds ws decode error ({}): {}", url, e);
          break;
        }
      };
      let lane = res.lane as usize;
      if lane >= mux_tx.len() {
        log::error!("mds ws lane out of range ({}): {}", url, lane);
        break;
      }
      if let Err(_) = mux_tx[lane].send(res).await {
        log::error!("mds ws channel send error ({})", url);
        break;
      }
    }
  }

  pub fn start(self) -> RawMdsHandle {
    let url = self.url;
    let (req_tx, req_rx): (
      flume::Sender<(proto::Request, oneshot::Sender<Result<proto::Response>>)>,
      _,
    ) = flume::bounded(10);
    let broken = Arc::new(AtomicBool::new(false));
    let (sink, stream) = self.stream.split();
    let sink = Arc::new(Mutex::new(sink));
    let (mux_tx, mut mux_rx): (
      Vec<tokio::sync::mpsc::Sender<proto::Response>>,
      Vec<tokio::sync::mpsc::Receiver<proto::Response>>,
    ) = (0..self.mux_width)
      .map(|_| tokio::sync::mpsc::channel(1))
      .unzip();
    let recv_task_handle = tokio::spawn(Self::recv_task(
      url,
      stream,
      mux_tx,
      BrokenGuard {
        broken: broken.clone(),
      },
    ));
    let recv_task_handle = Arc::new(PMutex::new(Some(recv_task_handle)));
    mux_rx.reverse();
    for i in 0..self.mux_width {
      let lane_id = i;
      let sink = sink.clone();
      let req_rx = req_rx.clone();
      let mut mux_rx = mux_rx.pop().unwrap();
      let recv_task_handle = recv_task_handle.clone();
      tokio::spawn(async move {
        while let Ok((req, res_tx)) = req_rx.recv_async().await {
          let res = Self::send_request(&sink, lane_id, &mut mux_rx, req).await;
          let _ = res_tx.send(res);
        }
        if let Some(x) = recv_task_handle.lock().take() {
          x.abort();
        }
      });
    }
    RawMdsHandle { req_tx, broken }
  }
}

impl RawMdsHandle {
  pub fn is_broken(&self) -> bool {
    self.broken.load(std::sync::atomic::Ordering::Relaxed)
  }

  pub fn new_broken() -> Self {
    let (req_tx, _) = flume::bounded(1);
    RawMdsHandle {
      req_tx,
      broken: Arc::new(AtomicBool::new(true)),
    }
  }

  pub async fn run_raw(&self, script: &str, data: String) -> Result<String> {
    let (res_tx, res_rx) = oneshot::channel();
    let req = proto::Request {
      lane: 0, // to be rewritten
      program: script.to_string(),
      data,
    };
    self.req_tx.send_async((req, res_tx)).await?;
    let res = res_rx.await??;
    match res.body {
      Some(proto::response::Body::Output(s)) => Ok(s),
      Some(proto::response::Body::Error(s)) => {
        if s.retryable {
          Err(anyhow::Error::from(RetryableMdsError {
            description: s.description,
          }))
        } else {
          Err(anyhow::anyhow!(
            "non-retryable mds error: {}",
            s.description
          ))
        }
      }
      None => anyhow::bail!("response does not contain a body"),
    }
  }

  pub async fn run<R: for<'a> Deserialize<'a>>(
    &self,
    script: &str,
    data: &impl Serialize,
  ) -> Result<R> {
    let out = self.run_raw(script, serde_json::to_string(data)?).await?;
    Ok(serde_json::from_str(&out)?)
  }

  pub async fn get_many<S: AsRef<str>>(
    &self,
    paths: impl IntoIterator<Item = S>,
    primary: bool,
  ) -> Result<Vec<Option<Vec<u8>>>> {
    let keys = paths
      .into_iter()
      .map(|x| encode_path(x.as_ref()))
      .collect::<Result<Vec<_>>>()?;
    let values: Vec<Option<String>> = self
      .run(
        include_str!("../../mds_ts/output/get_many.js"),
        &serde_json::json!({ "keys": &keys, "primary": primary }),
      )
      .await?;
    if values.len() != keys.len() {
      anyhow::bail!("mds get: unexpected response length");
    }
    Ok(
      values
        .into_iter()
        .map(|x| {
          x.map(|x| base64::decode(x).map_err(anyhow::Error::from))
            .transpose()
        })
        .collect::<Result<Vec<_>>>()?,
    )
  }

  pub async fn prefix_list<S: AsRef<str>>(
    &self,
    prefix: S,
    mut opts: PrefixListOptions,
    primary: bool,
  ) -> Result<Vec<(String, Vec<u8>)>> {
    opts.cursor = opts.cursor.map(|x| encode_path(&x)).transpose()?;
    let prefix = encode_path(prefix.as_ref())?;
    let pairs: Vec<(String, Option<String>)> = self
      .run(
        include_str!("../../mds_ts/output/prefix_list.js"),
        &serde_json::json!({ "prefix": prefix, "opts": opts, "primary": primary }),
      )
      .await?;
    pairs
      .into_iter()
      .map(|(k, v)| -> Result<(String, Vec<u8>)> {
        let k = decode_path(&String::from_utf8_lossy(&base64::decode(&k)?))?;
        let v = v.map(|x| base64::decode(&x)).transpose()?.unwrap_or(vec![]);
        Ok((k, v))
      })
      .collect()
  }

  pub async fn prefix_delete<S: AsRef<str>>(&self, prefix: S) -> Result<()> {
    let prefix = encode_path(prefix.as_ref())?;
    self
      .run(
        include_str!("../../mds_ts/output/prefix_delete.js"),
        &serde_json::json!({ "prefix": prefix }),
      )
      .await?;
    Ok(())
  }

  pub async fn compare_and_set_many<S: AsRef<str>, I: AsRef<[u8]>, V: AsRef<[u8]>>(
    &self,
    paths: impl IntoIterator<Item = (S, TriStateCheck<I>, TriStateSet<V>)>,
  ) -> Result<Option<CompareAndSetResult>> {
    let paths = paths
      .into_iter()
      .map(|(k, c, s)| Ok((encode_path(k.as_ref())?, c, s)))
      .collect::<Result<Vec<_>>>()?;
    let checks: Vec<(&str, Option<String>)> = paths
      .iter()
      .filter_map(|(path, check, set)| {
        // The key will be appended with a unique versionstamp so we assume no conflict will ever happen.
        if matches!(set, TriStateSet::WithVersionstampedKey { .. }) {
          None
        } else {
          match check {
            TriStateCheck::Value(x) => Some((path.as_str(), Some(base64::encode(x.as_ref())))),
            TriStateCheck::Absent => Some((path.as_str(), None)),
            TriStateCheck::Any => None,
          }
        }
      })
      .collect();
    let sets: Vec<(&str, Option<String>)> = paths
      .iter()
      .filter_map(|(path, _, value)| match value {
        TriStateSet::Value(x) => Some((path.as_str(), Some(base64::encode(x.as_ref())))),
        TriStateSet::Delete => Some((path.as_str(), None)),
        TriStateSet::Preserve => None,
        TriStateSet::WithVersionstampedKey { .. } => None,
      })
      .collect();
    let versionstamped_sets: Vec<(&str, Option<String>)> = paths
      .iter()
      .filter_map(|(path, _, value)| match value {
        TriStateSet::WithVersionstampedKey { value } => {
          Some((path.as_str(), Some(base64::encode(value.as_ref()))))
        }
        _ => None,
      })
      .collect();
    for _ in 0..5 {
      let result: Option<CompareAndSetResult> = match self
        .run(
          include_str!("../../mds_ts/output/compare_and_set_many.js"),
          &serde_json::json!({ "checks": &checks, "sets": &sets, "versionstamped_sets": &versionstamped_sets }),
        )
        .await
      {
        Ok(x) => x,
        Err(e) => {
          if e.downcast_ref::<RetryableMdsError>().is_some() {
            log::warn!("retrying compare_and_set_many: {:?}", e);
            random_backoff_fast().await;
            continue;
          }
          return Err(e);
        }
      };
      return Ok(result);
    }
    anyhow::bail!("mds compare_and_set_many: too many retries")
  }
}
