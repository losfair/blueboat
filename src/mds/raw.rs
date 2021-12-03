use std::{io::Cursor, sync::Arc};

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

mod proto {
  include!(concat!(env!("OUT_DIR"), "/mds.rs"));
}

type StreamTy = WebSocketStream<MaybeTlsStream<TcpStream>>;
type SinkTy = SplitSink<StreamTy, tokio_tungstenite::tungstenite::Message>;

#[derive(Error, Debug)]
#[error("ws connection closed")]
struct WsClose;

pub struct RawMds {
  url: String,
  stream: StreamTy,
  mux_width: u32,
}

#[derive(Clone)]
pub struct RawMdsHandle {
  req_tx: flume::Sender<(proto::Request, oneshot::Sender<Result<proto::Response>>)>,
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

  pub async fn authenticate(&mut self, store: &str, keypair: &Keypair) -> Result<()> {
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
      Ok(())
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
    let (sink, stream) = self.stream.split();
    let sink = Arc::new(Mutex::new(sink));
    let (mux_tx, mut mux_rx): (
      Vec<tokio::sync::mpsc::Sender<proto::Response>>,
      Vec<tokio::sync::mpsc::Receiver<proto::Response>>,
    ) = (0..self.mux_width)
      .map(|_| tokio::sync::mpsc::channel(1))
      .unzip();
    let recv_task_handle = tokio::spawn(Self::recv_task(url, stream, mux_tx));
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
    RawMdsHandle { req_tx }
  }
}

impl RawMdsHandle {
  pub async fn run<R: for<'a> Deserialize<'a>>(
    &self,
    script: &str,
    data: &impl Serialize,
  ) -> Result<R> {
    let (res_tx, res_rx) = oneshot::channel();
    let req = proto::Request {
      lane: 0, // to be rewritten
      program: script.to_string(),
      data: serde_json::to_string(data)?,
    };
    self.req_tx.send_async((req, res_tx)).await?;
    let res = res_rx.await??;
    match res.body {
      Some(proto::response::Body::Output(s)) => Ok(serde_json::from_str(&s)?),
      Some(proto::response::Body::Error(s)) => {
        anyhow::bail!("remote error: {}", s.description)
      }
      None => anyhow::bail!("response does not contain a body"),
    }
  }
}
