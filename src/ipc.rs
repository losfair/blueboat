use std::{collections::HashMap, str::FromStr};

use anyhow::Result;
use bytes::Bytes;
use hyper::{
  header::{HeaderName, HeaderValue},
  Body, StatusCode,
};
use itertools::Itertools;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smr::types::{BaseRequest, Request, Response};
use std::convert::TryFrom;
use thiserror::Error;
use tokio::sync::watch;
use v8;

use crate::{
  api::util::v8_serialize,
  ctx::{BlueboatCtx, BlueboatInitData},
  exec::Executor,
  objserde::deserialize_v8_value,
  v8util::{create_arraybuffer_from_bytes, set_up_v8_globally, ObjectExt},
};

#[derive(Serialize, Deserialize)]
pub struct BlueboatIpcReq {
  pub v: BlueboatIpcReqV,
  pub id: String,
}

#[derive(Serialize, Deserialize)]
pub enum BlueboatIpcReqV {
  Http(BlueboatRequest),
  Background(Vec<u8>),
  SseAuth(BlueboatRequest),
}

impl BaseRequest for BlueboatIpcReq {
  type Res = BlueboatIpcRes;
  type InitData = BlueboatInitData;
  type Context = BlueboatCtx;
}

#[async_trait::async_trait(?Send)]
impl Request for BlueboatIpcReq {
  async fn init(init_data: Self::InitData) -> &'static Self::Context {
    set_up_v8_globally();
    BlueboatCtx::init(init_data)
  }

  async fn handle(mut self, _: &'static Self::Context) -> Result<Self::Res> {
    unimplemented!()
  }

  async fn handle_with_cancellation(
    mut self,
    ctx: &'static Self::Context,
    cancel: watch::Receiver<()>,
  ) -> Result<Self::Res> {
    #[derive(Error, Debug)]
    #[error("entry error")]
    struct EntryError;

    #[derive(Error, Debug)]
    #[error("completion error")]
    struct CompletionError;

    let (exec, spawn_activity_owner) = Executor::new(ctx, self.id.clone(), cancel)?;
    let v = self.v;
    Executor::enter(&exec.downgrade(), move |scope| {
      let (entry_key, args) = v.build_invocation(scope)?;
      let context = scope.get_current_context();
      let global_obj = context.global(scope);
      let entry_fn = global_obj.get_ext(scope, entry_key);
      let entry_fn = v8::Local::<v8::Function>::try_from(entry_fn)?;
      let undef = v8::undefined(scope);
      entry_fn
        .call(scope, undef.into(), &args)
        .ok_or_else(|| EntryError)?;
      Ok::<(), anyhow::Error>(())
    })
    .ok_or(EntryError)??;
    drop(spawn_activity_owner);

    let res = exec
      .wait_for_completion()
      .await
      .ok_or_else(|| CompletionError)?;
    Ok(res)
  }
}

impl BlueboatIpcReqV {
  fn build_invocation<'s>(
    self,
    scope: &mut v8::HandleScope<'s>,
  ) -> Result<(&'static str, Vec<v8::Local<'s, v8::Value>>)> {
    match self {
      Self::Http(mut req) => {
        let body = std::mem::replace(&mut req.body, vec![]);
        let req = v8_serialize(scope, &req)?;
        let body = create_arraybuffer_from_bytes(scope, &body);
        Ok(("__blueboat_app_entry", vec![req, body.into()]))
      }
      Self::Background(wire_bytes) => {
        let value = deserialize_v8_value(scope, &wire_bytes)?;
        Ok(("__blueboat_app_background_entry", vec![value]))
      }
      Self::SseAuth(req) => {
        let req = v8_serialize(scope, &req)?;
        Ok(("__blueboat_app_sse_auth_entry", vec![req]))
      }
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct BlueboatIpcRes {
  pub response: BlueboatResponse,
  pub body: Bytes,
}

impl Response for BlueboatIpcRes {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct BlueboatRequest {
  pub method: String,
  pub uri: String,
  pub headers: HashMap<String, Vec<String>>,
  pub body: Vec<u8>,
}

impl BlueboatRequest {
  pub async fn from_hyper(that: hyper::Request<Body>) -> Result<Self> {
    let headers = decode_hyper_header_map(that.headers());
    Ok(Self {
      method: that.method().to_string(),
      uri: that.uri().to_string(),
      headers,
      body: hyper::body::to_bytes(that.into_body()).await?.to_vec(),
    })
  }
  pub fn from_hyper_no_body<T>(that: &hyper::Request<T>) -> Result<Self> {
    let headers = decode_hyper_header_map(that.headers());
    Ok(Self {
      method: that.method().to_string(),
      uri: that.uri().to_string(),
      headers,
      body: vec![],
    })
  }

  pub fn into_reqwest(self) -> Result<reqwest::Request> {
    let mut req = reqwest::Request::new(
      reqwest::Method::from_str(&self.method)?,
      reqwest::Url::from_str(&self.uri)?,
    );
    encode_hyper_header_map(req.headers_mut(), &self.headers);
    *req.body_mut() = Some(reqwest::Body::from(self.body));
    Ok(req)
  }
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct BlueboatResponse {
  pub status: u16,
  pub headers: HashMap<String, Vec<String>>,
}

impl BlueboatResponse {
  pub async fn from_reqwest(that: reqwest::Response) -> Result<(Self, Bytes)> {
    let headers = decode_hyper_header_map(that.headers());
    Ok((
      Self {
        status: that.status().as_u16(),
        headers,
      },
      that.bytes().await?,
    ))
  }

  pub fn into_hyper(self, body: Bytes) -> Result<hyper::Response<Body>> {
    let mut res = hyper::Response::new(Body::from(body));
    *res.status_mut() = StatusCode::from_u16(self.status)?;
    encode_hyper_header_map(res.headers_mut(), &self.headers);
    Ok(res)
  }
}

fn decode_hyper_header_map(m: &hyper::HeaderMap) -> HashMap<String, Vec<String>> {
  m.iter()
    .filter_map(|(k, v)| {
      std::str::from_utf8(v.as_bytes())
        .ok()
        .map(|v| (k.as_str(), v))
    })
    .group_by(|x| x.0)
    .into_iter()
    .map(|(k, v)| {
      (
        k.to_string(),
        v.into_iter()
          .map(|(_, v)| v.to_string())
          .collect::<Vec<_>>(),
      )
    })
    .collect()
}

fn encode_hyper_header_map(m: &mut hyper::HeaderMap, source: &HashMap<String, Vec<String>>) {
  let mut source = source.iter().collect::<Vec<_>>();
  source.sort_by_key(|x| x.0);
  for (k, v) in source {
    let k = if let Ok(k) = HeaderName::from_str(k) {
      k
    } else {
      continue;
    };
    for x in v {
      if let Ok(x) = HeaderValue::from_str(&x) {
        m.append(k.clone(), x);
      }
    }
  }
}
