use std::collections::HashMap;

use anyhow::Result;
use rusty_v8 as v8;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use thiserror::Error;

use crate::{
  api::util::{v8_invoke_callback, v8_serialize},
  exec::Executor,
  v8util::FunctionCallbackArgumentsExt,
};

use super::util::v8_deserialize;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ApnsRequest {
  device_token: String,
  options: ApnsOptions,
  aps: ApnsAps,
  data: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ApnsResponse {
  error: Option<ApnsError>,
  apns_id: Option<String>,
  code: u16,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct ApnsError {
  reason: String,
  timestamp: Option<u64>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct ApnsOptions {
  apns_id: Option<String>,
  apns_expiration: Option<u64>,
  apns_priority: Option<ApnsNotificationPriority>,
  apns_topic: Option<String>,
  apns_collapse_id: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct ApnsAps {
  alert: Option<ApnsAlert>,
  badge: Option<u32>,
  sound: Option<String>,
  content_available: bool,
  category: Option<String>,
  mutable_content: bool,
}

#[derive(Serialize, Deserialize, JsonSchema)]
enum ApnsNotificationPriority {
  #[serde(rename = "high")]
  High,
  #[serde(rename = "normal")]
  Normal,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
enum ApnsAlert {
  #[serde(rename = "plain")]
  Plain { content: String },
}

pub fn api_apns_send(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("apns key not found: {0}")]
  struct NotFound(String);

  let key = v8::Local::<v8::String>::try_from(args.get(1))?.to_rust_string_lossy(scope);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  let client = match ctx.apns.get(&key) {
    Some(x) => x,
    None => return Err(NotFound(key).into()),
  };

  let req: ApnsRequest = v8_deserialize(scope, args.get(2))?;
  let callback = v8::Global::new(scope, args.load_function_at(3)?);
  let exec_2 = exec.clone();
  Executor::spawn(&exec_2, async move {
    let a2_req = a2::request::payload::Payload {
      options: a2::request::notification::NotificationOptions {
        apns_id: req.options.apns_id.as_ref().map(|x| x.as_str()),
        apns_expiration: req.options.apns_expiration,
        apns_priority: req.options.apns_priority.as_ref().map(|x| match x {
          ApnsNotificationPriority::High => a2::request::notification::Priority::High,
          ApnsNotificationPriority::Normal => a2::request::notification::Priority::Normal,
        }),
        apns_topic: req.options.apns_topic.as_ref().map(|x| x.as_str()),
        apns_collapse_id: req
          .options
          .apns_collapse_id
          .as_ref()
          .and_then(|x| a2::request::notification::CollapseId::new(x).ok()),
      },
      device_token: &req.device_token,
      aps: a2::request::payload::APS {
        alert: req.aps.alert.as_ref().map(|x| match x {
          ApnsAlert::Plain { content } => a2::request::payload::APSAlert::Plain(content),
        }),
        badge: req.aps.badge,
        sound: req.aps.sound.as_ref().map(|x| x.as_str()),
        content_available: req.aps.content_available.then(|| 1),
        category: req.aps.category.as_ref().map(|x| x.as_str()),
        mutable_content: req.aps.mutable_content.then(|| 1),
      },
      data: req
        .data
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect(),
    };
    let res = client.send(a2_req).await.map(|x| ApnsResponse {
      error: x.error.map(|x| ApnsError {
        reason: format!("{:?}", x.reason),
        timestamp: x.timestamp,
      }),
      apns_id: x.apns_id,
      code: x.code,
    });
    Executor::enter(&exec, |scope| {
      let res = res
        .map_err(anyhow::Error::from)
        .and_then(|x| v8_serialize(scope, &x));
      v8_invoke_callback("apns_send", scope, res, &callback);
    });
  });
  Ok(())
}
