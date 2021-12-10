use std::{sync::Arc, time::Duration};

use anyhow::Result;
use rdkafka::producer::FutureRecord;
use serde::{Deserialize, Serialize};
use v8;

use crate::{
  exec::Executor, lpch::BackgroundEntry, metadata::Metadata, objserde::serialize_v8_value,
  package::PackageKey, reliable_channel::RchReqBody, task::get_channel_refresher,
  v8util::FunctionCallbackArgumentsExt,
};

use super::util::v8_error;

#[derive(Serialize, Deserialize)]
struct ScheduleAtLeastOnceRequest {
  wire_bytes: Vec<u8>,
  request_id: String,
}

#[derive(Serialize, Deserialize)]
struct ScheduleAtLeastOnceResponse {}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for ScheduleAtLeastOnceRequest {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let entry = BackgroundEntry {
      app: PackageKey {
        path: md.path.clone(),
        version: md.version.clone(),
      },
      request_id: self.request_id,
      wire_bytes: self.wire_bytes,
    };
    let entry = rmp_serde::to_vec(&entry)?;
    let ch = get_channel_refresher().get_random()?;
    ch.producer
      .send(
        FutureRecord {
          topic: &ch.config.topic,
          partition: None,
          payload: Some(&entry[..]),
          key: None::<&[u8]>,
          timestamp: None,
          headers: None,
        },
        rdkafka::util::Timeout::After(Duration::from_millis(5000)),
      )
      .await
      .map_err(|(e, _)| anyhow::anyhow!("kafka produce failed: {}", e))?;
    Ok(Box::new(ScheduleAtLeastOnceResponse {}))
  }
}

pub fn api_schedule_at_least_once(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let wire_bytes = serialize_v8_value(scope, args.get(1))?;
  let callback = v8::Global::new(scope, args.load_function_at(2)?);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  let req = ScheduleAtLeastOnceRequest {
    request_id: exec.upgrade().unwrap().request_id.clone(),
    wire_bytes,
  };
  Executor::spawn(&exec.clone(), async move {
    let out: Result<ScheduleAtLeastOnceResponse> = ctx.rch.call(req).await;
    Executor::enter(&exec, |scope| {
      let undef = v8::undefined(scope);
      let callback = v8::Local::new(scope, &callback);
      match out {
        Ok(_) => {
          callback.call(scope, undef.into(), &[undef.into(), undef.into()]);
        }
        Err(e) => {
          let e = v8_error("schedule_at_least_once", scope, &e);
          callback.call(scope, undef.into(), &[e, undef.into()]);
        }
      }
    });
  });
  Ok(())
}
