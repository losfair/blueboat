use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use hyper::{Body, Method, Request, Response};

use crate::{
  ipc::{BlueboatIpcReq, BlueboatIpcReqV, BlueboatRequest},
  metadata::Metadata,
  server::generic_invoke,
};

use super::mq::MessageQueueTopic;

const MIN_BATCH_SIZE: u32 = 1;
const MAX_BATCH_SIZE: u32 = 100;

struct SseTask {
  topic: MessageQueueTopic,
  sender: hyper::body::Sender,
  wait: bool,
  n: u32,
  seq: [u8; 10],
}

pub async fn handle_sse(
  request_id: &str,
  req: Request<Body>,
  md: Arc<Metadata>,
) -> Result<Response<Body>> {
  if req.method() != Method::GET {
    return Ok(Response::new(Body::from("only GET requests are allowed")));
  }

  let mq = super::MQ
    .get()
    .ok_or_else(|| anyhow::anyhow!("MQ is not initialized"))?;

  let bb_req = BlueboatRequest::from_hyper_no_body(&req)?;
  let res = generic_invoke(
    BlueboatIpcReq {
      id: request_id.to_string(),
      v: BlueboatIpcReqV::SseAuth(bb_req),
    },
    md.clone(),
    None,
  )
  .await?;
  if res.response.status != 200 {
    return Ok(res.response.into_hyper(res.body)?);
  }

  let uri = req.uri();
  let params = uri
    .query()
    .map(|v| {
      url::form_urlencoded::parse(v.as_bytes())
        .into_owned()
        .collect()
    })
    .unwrap_or_else(HashMap::new);
  let ns_param = params.get("ns").map(|x| x.as_str()).unwrap_or_default();
  let topic_param = params.get("topic").map(|x| x.as_str()).unwrap_or_default();
  let from_seq_param = req
    .headers()
    .get("last-event-id")
    .map(|x| x.to_str().unwrap_or_default())
    .unwrap_or_default();
  let batch_size_param = params
    .get("batch_size")
    .map(|x| x.as_str())
    .unwrap_or_default();
  let no_wait = params
    .get("no_wait")
    .map(|x| x.as_str())
    .unwrap_or_default();

  let ns = match md.pubsub.get(ns_param) {
    Some(x) => mq.namespace(x.namespace_bytes),
    None => return Ok(Response::new(Body::from("namespace not found"))),
  };
  let topic = ns.topic(topic_param.to_string());

  let mut from_seq = [0u8; 10];
  if from_seq_param.is_empty() {
    from_seq = topic.seq().await?;
  } else {
    match hex::decode_to_slice(from_seq_param, &mut from_seq) {
      Ok(()) => {}
      Err(_) => return Ok(Response::new(Body::from("invalid last-event-id"))),
    }
  }

  let wait = no_wait != "1";
  let n = if wait {
    MAX_BATCH_SIZE
  } else {
    if batch_size_param.is_empty() {
      MAX_BATCH_SIZE
    } else {
      let n = match batch_size_param.parse::<u32>() {
        Ok(x) => x,
        Err(_) => return Ok(Response::new(Body::from("invalid batch_size"))),
      };

      if n < MIN_BATCH_SIZE || n > MIN_BATCH_SIZE {
        return Ok(Response::new(Body::from("batch_size out of range")));
      }
      n
    }
  };

  let (sender, body) = Body::channel();
  let task = SseTask {
    topic,
    sender,
    wait,
    n,
    seq: from_seq,
  };
  tokio::spawn(async move {
    task.run().await;
    log::debug!("exiting sse task");
  });

  let mut res = Response::new(body);
  res.headers_mut().insert(
    "content-type",
    hyper::header::HeaderValue::from_static("text/event-stream"),
  );
  Ok(res)
}

impl SseTask {
  async fn run(mut self) {
    if !self
      .send_sse(&SseMessage {
        event: "init",
        data: &[],
        id: &hex::encode(&self.seq),
      })
      .await
    {
      return;
    }

    loop {
      let pop_fut = self.topic.pop(self.seq, self.n as usize, self.wait);
      let stop_fut = watch_sender_disconnect(&mut self.sender);

      let res = tokio::select! {
        res = pop_fut => res,
        _ = stop_fut => {
          return;
        }
      };
      let res = match res {
        Ok(x) => x,
        Err(e) => {
          tracing::error!(error = %e, "mq pop error");
          return;
        }
      };
      for (seq, data) in &res {
        if !self
          .send_sse(&SseMessage {
            event: "message",
            data: data,
            id: &hex::encode(seq),
          })
          .await
        {
          return;
        }
      }
      if !self.wait {
        return;
      }
      if res.is_empty() {
        tracing::error!("mq pop empty");
        return;
      }
      self.seq = res.last().unwrap().0;
    }
  }

  async fn send_sse<'a, 'b, 'c>(&mut self, msg: &SseMessage<'a, 'b, 'c>) -> bool {
    let chunk = match msg.encode() {
      Ok(x) => x,
      Err(e) => {
        tracing::error!(error = %e, event = msg.event, "failed to encode sse message");
        return false;
      }
    };

    if self.sender.send_data(chunk.into()).await.is_err() {
      return false;
    }

    true
  }
}

struct SseMessage<'a, 'b, 'c> {
  event: &'a str,
  data: &'b [u8],
  id: &'c str,
}

impl<'a, 'b, 'c> SseMessage<'a, 'b, 'c> {
  fn encode(&self) -> Result<Vec<u8>> {
    let data = std::str::from_utf8(self.data)?;
    for &b in self.data {
      if b == b'\n' {
        anyhow::bail!("sse data may not contain newline");
      }
    }
    let msg = format!("event: {}\ndata: {}\nid: {}\n\n", self.event, data, self.id);
    Ok(msg.into_bytes())
  }
}

async fn watch_sender_disconnect(sender: &mut hyper::body::Sender) {
  loop {
    if futures::future::poll_fn(|cx| sender.poll_ready(cx))
      .await
      .is_err()
    {
      return;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
  }
}
