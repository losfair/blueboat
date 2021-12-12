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

use super::util::{v8_deserialize, v8_error, v8_serialize};

#[derive(Serialize, Deserialize)]
struct ScheduleAtLeastOnceRequest {
  wire_bytes: Vec<u8>,
  request_id: String,
  same_version: bool,
}

#[derive(Serialize, Deserialize)]
struct ScheduleAtLeastOnceResponse {}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleAtLeastOnceOpts {
  #[serde(default)]
  same_version: bool,
}

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
      same_version: self.same_version,
    };
    let entry = rmp_serde::to_vec(&entry)?;
    let ch = get_channel_refresher().get_random()?;
    ch.kafka_producer
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleDelayedOpts {
  ts_secs: i64,
  #[serde(default)]
  same_version: bool,
}

#[derive(Serialize, Deserialize)]
struct ScheduleDelayedRequest {
  wire_bytes: Vec<u8>,
  request_id: String,
  ts_secs: i64,
  same_version: bool,
}

#[derive(Serialize, Deserialize)]
struct ScheduleDelayedResponse {
  id: String,
}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for ScheduleDelayedRequest {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let entry = BackgroundEntry {
      app: PackageKey {
        path: md.path.clone(),
        version: md.version.clone(),
      },
      request_id: self.request_id,
      wire_bytes: self.wire_bytes,
      same_version: self.same_version,
    };
    let ch = get_channel_refresher().get_random()?;
    let id = ch
      .add_delayed_task(self.ts_secs, entry)
      .await
      .map_err(|e| anyhow::anyhow!("delayed task scheduling failed: {}", e))?;
    Ok(Box::new(ScheduleDelayedResponse { id }))
  }
}

pub fn api_schedule_at_least_once(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let wire_bytes = serialize_v8_value(scope, args.get(1))?;
  let opts: ScheduleAtLeastOnceOpts = v8_deserialize(scope, args.get(2))?;
  let callback = v8::Global::new(scope, args.load_function_at(3)?);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  let req = ScheduleAtLeastOnceRequest {
    request_id: exec.upgrade().unwrap().request_id.clone(),
    wire_bytes,
    same_version: opts.same_version,
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

pub fn api_schedule_delayed(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let wire_bytes = serialize_v8_value(scope, args.get(1))?;
  let opts: ScheduleDelayedOpts = v8_deserialize(scope, args.get(2))?;
  let callback = v8::Global::new(scope, args.load_function_at(3)?);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  let req = ScheduleDelayedRequest {
    request_id: exec.upgrade().unwrap().request_id.clone(),
    wire_bytes,
    ts_secs: opts.ts_secs,
    same_version: opts.same_version,
  };
  Executor::spawn(&exec.clone(), async move {
    let out: Result<ScheduleDelayedResponse> = ctx.rch.call(req).await;
    Executor::enter(&exec, |scope| {
      let undef = v8::undefined(scope);
      let callback = v8::Local::new(scope, &callback);
      match out {
        Ok(x) => {
          let res = v8_serialize(scope, &x).unwrap();
          callback.call(scope, undef.into(), &[undef.into(), res]);
        }
        Err(e) => {
          let e = v8_error("schedule_delayed", scope, &e);
          callback.call(scope, undef.into(), &[e, undef.into()]);
        }
      }
    });
  });
  Ok(())
}
