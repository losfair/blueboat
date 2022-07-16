use std::sync::Arc;

use crate::{
  exec::Executor,
  metadata::Metadata,
  pubsub::MQ,
  reliable_channel::RchReqBody,
  v8util::{FunctionCallbackArgumentsExt, LocalValueExt},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use v8;

use super::util::{v8_deserialize, v8_error, v8_serialize};

const MAX_TOPIC_KEY_SIZE: usize = 100;
const MAX_MESSAGE_SIZE: usize = 10000;

#[derive(Deserialize)]
struct JsPublishRequest {
  namespace: String,
  topic: String,
}

#[derive(Serialize, Deserialize)]
struct PublishRequest {
  namespace: String,
  topic: String,
  data: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct PublishResponse {
  id: String,
}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for PublishRequest {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let ns = md
      .pubsub
      .get(&self.namespace)
      .ok_or_else(|| anyhow::anyhow!("pubsub namespace not found"))?;
    let mq = MQ
      .get()
      .ok_or_else(|| anyhow::anyhow!("pubsub mq not initialized"))?;
    let topic = mq.namespace(ns.namespace_bytes).topic(self.topic);
    let seq = topic.push(&self.data).await?;
    Ok(Box::new(PublishResponse {
      id: hex::encode(&seq),
    }))
  }
}

pub fn api_pubsub_publish(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let req: JsPublishRequest = v8_deserialize(scope, args.get(1))?;

  if req.topic.len() > MAX_TOPIC_KEY_SIZE {
    anyhow::bail!("topic key too long");
  }

  let body = {
    let raw = unsafe { args.get(2).read_bytes_assume_noalias(scope)? };
    let raw = &raw[..];

    if raw.len() > MAX_MESSAGE_SIZE {
      anyhow::bail!("message too large");
    }

    if std::str::from_utf8(raw).is_err() {
      anyhow::bail!("message contains invalid utf8");
    }

    for &b in raw {
      if b == b'\n' {
        anyhow::bail!("message contains newline");
      }
    }

    raw.to_vec()
  };

  let req = PublishRequest {
    namespace: req.namespace,
    topic: req.topic,
    data: body,
  };

  let callback = v8::Global::new(scope, args.load_function_at(3)?);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  Executor::spawn(&exec.clone(), async move {
    let out: Result<PublishResponse> = ctx.rch.call(req).await;

    Executor::enter(&exec, |scope| {
      let out = out.and_then(|rsp| v8_serialize(scope, &rsp));
      let undef = v8::undefined(scope);
      let callback = v8::Local::new(scope, &callback);
      match out {
        Ok(rsp) => {
          callback.call(scope, undef.into(), &[undef.into(), rsp]);
        }
        Err(e) => {
          let e = v8_error("pubsub_publish", scope, &e);
          callback.call(scope, undef.into(), &[e, undef.into()]);
        }
      }
    });
  });

  Ok(())
}
