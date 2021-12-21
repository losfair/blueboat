use std::sync::Arc;

use crate::{
  exec::Executor,
  mds::{
    get_mds,
    raw::{PrefixListOptions, TriStateCheck, TriStateSet},
  },
  metadata::Metadata,
  reliable_channel::RchReqBody,
  v8util::{create_uint8array_from_bytes, FunctionCallbackArgumentsExt},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use v8;

use super::util::{v8_deref_typed_array_assuming_noalias, v8_deserialize, v8_error, v8_serialize};

const MAX_KEYS_PER_OP: usize = 1000;
const MAX_KEY_SIZE: usize = 4096;
const MAX_VALUE_SIZE: usize = 80000;

#[derive(Serialize, Deserialize)]
struct KvGetManyRequest {
  namespace: String,
  keys: Vec<String>,
  primary: bool,
}

#[derive(Serialize, Deserialize)]
struct KvGetManyResponse {
  values: Vec<Option<Vec<u8>>>,
}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for KvGetManyRequest {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let mds = get_mds()?;
    let ns = md
      .kv_namespaces
      .get(&self.namespace)
      .ok_or_else(|| anyhow::anyhow!("namespace not found"))?;
    let shard = mds
      .get_shard_session(&ns.shard)
      .ok_or_else(|| anyhow::anyhow!("shard not found"))?;
    let values = shard
      .get_many(
        self
          .keys
          .iter()
          .map(|k| format!("{}/{}", ns.prefix, k.as_str())),
        self.primary,
      )
      .await?;
    Ok(Box::new(KvGetManyResponse { values }))
  }
}

#[derive(Serialize, Deserialize)]
pub struct KvCompareAndSetManyRequest<T> {
  namespace: String,
  keys: Vec<KvCompareAndSetManyRequestKey<T>>,
}

#[derive(Serialize, Deserialize)]
pub struct KvCompareAndSetManyRequestKey<T> {
  key: String,
  check: TriStateCheck<T>,
  set: TriStateSet<T>,
}

#[derive(Serialize, Deserialize)]
struct KvCompareAndSetManyResponse {
  ok: bool,
}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for KvCompareAndSetManyRequest<Vec<u8>> {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let mds = get_mds()?;
    let ns = md
      .kv_namespaces
      .get(&self.namespace)
      .ok_or_else(|| anyhow::anyhow!("namespace not found"))?;
    let shard = mds
      .get_shard_session(&ns.shard)
      .ok_or_else(|| anyhow::anyhow!("shard not found"))?;
    let ok = shard
      .compare_and_set_many(
        self
          .keys
          .into_iter()
          .map(|k| (format!("{}/{}", ns.prefix, k.key.as_str()), k.check, k.set)),
      )
      .await?;
    Ok(Box::new(KvCompareAndSetManyResponse { ok }))
  }
}

impl<'s> KvCompareAndSetManyRequest<serde_v8::Value<'s>> {
  fn encode<'t>(
    &self,
    scope: &mut v8::HandleScope<'t>,
  ) -> Result<KvCompareAndSetManyRequest<Vec<u8>>> {
    let mut uint8array_to_vec = |v: &serde_v8::Value<'s>| -> Result<Vec<u8>> {
      let x = v8::Local::<v8::TypedArray>::try_from(v.v8_value)?;
      Ok(unsafe { v8_deref_typed_array_assuming_noalias(scope, x) }.to_vec())
    };
    Ok(KvCompareAndSetManyRequest {
      namespace: self.namespace.clone(),
      keys: self
        .keys
        .iter()
        .map(|x| {
          Ok(KvCompareAndSetManyRequestKey {
            key: x.key.clone(),
            check: match &x.check {
              TriStateCheck::Value(x) => TriStateCheck::Value(uint8array_to_vec(x)?),
              TriStateCheck::Absent => TriStateCheck::Absent,
              TriStateCheck::Any => TriStateCheck::Any,
            },
            set: match &x.set {
              TriStateSet::Value(x) => TriStateSet::Value(uint8array_to_vec(x)?),
              TriStateSet::Delete => TriStateSet::Delete,
              TriStateSet::Preserve => TriStateSet::Preserve,
            },
          })
        })
        .collect::<Result<_>>()?,
    })
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KvPrefixListRequest {
  namespace: String,
  prefix: String,
  opts: PrefixListOptions,
  primary: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KvPrefixListResponse {
  key_value_pairs: Vec<(String, Vec<u8>)>,
}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for KvPrefixListRequest {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let mds = get_mds()?;
    let ns = md
      .kv_namespaces
      .get(&self.namespace)
      .ok_or_else(|| anyhow::anyhow!("namespace not found"))?;
    let shard = mds
      .get_shard_session(&ns.shard)
      .ok_or_else(|| anyhow::anyhow!("shard not found"))?;
    let key_value_pairs = shard
      .prefix_list(
        format!("{}/{}", ns.prefix, self.prefix),
        self.opts,
        self.primary,
      )
      .await?;
    Ok(Box::new(KvPrefixListResponse { key_value_pairs }))
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KvPrefixDeleteRequest {
  namespace: String,
  prefix: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KvPrefixDeleteResponse {}

#[async_trait::async_trait]
#[typetag::serde]
impl RchReqBody for KvPrefixDeleteRequest {
  async fn handle(self: Box<Self>, md: Arc<Metadata>) -> Result<Box<dyn erased_serde::Serialize>> {
    let mds = get_mds()?;
    let ns = md
      .kv_namespaces
      .get(&self.namespace)
      .ok_or_else(|| anyhow::anyhow!("namespace not found"))?;
    let shard = mds
      .get_shard_session(&ns.shard)
      .ok_or_else(|| anyhow::anyhow!("shard not found"))?;
    shard
      .prefix_delete(format!("{}/{}", ns.prefix, self.prefix))
      .await?;
    Ok(Box::new(KvPrefixDeleteResponse {}))
  }
}

fn api_kv_generic<
  'a,
  'b,
  ReqTy: for<'x> Deserialize<'x>,
  ReqTy1: for<'x> Deserialize<'x> + RchReqBody + 'static,
  RspTy: for<'x> Deserialize<'x> + 'static,
  TFRsp: for<'x> FnOnce(&mut v8::HandleScope<'x>, RspTy) -> Result<v8::Local<'x, v8::Value>> + 'static,
  TFReq: for<'x> FnOnce(&mut v8::HandleScope<'x>, ReqTy) -> Result<ReqTy1> + 'static,
>(
  scope: &mut v8::HandleScope<'a>,
  args: v8::FunctionCallbackArguments<'b>,
  name: &'static str,
  transform_rsp: TFRsp,
  transform_req: TFReq,
) -> Result<()> {
  let req: ReqTy = v8_deserialize(scope, args.get(1))?;
  let req = transform_req(scope, req)?;
  let callback = v8::Global::new(scope, args.load_function_at(2)?);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  Executor::spawn(&exec.clone(), async move {
    let out: Result<RspTy> = ctx.rch.call(req).await;
    Executor::enter(&exec, |scope| {
      let out = out.and_then(|rsp| transform_rsp(scope, rsp));
      let undef = v8::undefined(scope);
      let callback = v8::Local::new(scope, &callback);
      match out {
        Ok(rsp) => {
          callback.call(scope, undef.into(), &[undef.into(), rsp]);
        }
        Err(e) => {
          let e = v8_error(name, scope, &e);
          callback.call(scope, undef.into(), &[e, undef.into()]);
        }
      }
    });
  });
  Ok(())
}

pub fn api_kv_get_many<'a, 'b, 'c>(
  scope: &mut v8::HandleScope<'a>,
  args: v8::FunctionCallbackArguments<'b>,
  _retval: v8::ReturnValue<'c>,
) -> Result<()> {
  api_kv_generic::<KvGetManyRequest, _, KvGetManyResponse, _, _>(
    scope,
    args,
    "kv_get_many",
    |scope, rsp| {
      let out: Vec<serde_v8::Value> = rsp
        .values
        .iter()
        .map(|v| {
          v.as_ref()
            .map(|v| v8::Local::<v8::Value>::from(create_uint8array_from_bytes(scope, v)))
            .unwrap_or_else(|| v8::Local::<v8::Value>::from(v8::null(scope)))
        })
        .map(|v8_value| serde_v8::Value { v8_value })
        .collect();
      Ok(v8_serialize(scope, &out)?)
    },
    |_, req| {
      if req.keys.len() > MAX_KEYS_PER_OP {
        anyhow::bail!("too many keys");
      }
      for k in &req.keys {
        if k.as_bytes().len() > MAX_KEY_SIZE {
          anyhow::bail!("key too large");
        }
      }
      Ok(req)
    },
  )
}

pub fn api_kv_compare_and_set_many<'a, 'b, 'c>(
  scope: &mut v8::HandleScope<'a>,
  args: v8::FunctionCallbackArguments<'b>,
  _retval: v8::ReturnValue<'c>,
) -> Result<()> {
  api_kv_generic::<
    KvCompareAndSetManyRequest<serde_v8::Value>,
    KvCompareAndSetManyRequest<Vec<u8>>,
    KvCompareAndSetManyResponse,
    _,
    _,
  >(
    scope,
    args,
    "kv_compare_and_set_many<",
    |scope, rsp| Ok(v8::Boolean::new(scope, rsp.ok).into()),
    |scope, req| {
      if req.keys.len() > MAX_KEYS_PER_OP {
        anyhow::bail!("too many keys");
      }
      let req = req.encode(scope)?;
      for k in &req.keys {
        if k.key.as_bytes().len() > MAX_KEY_SIZE {
          anyhow::bail!("key too large");
        }
        if let TriStateCheck::Value(x) = &k.check {
          if x.len() > MAX_VALUE_SIZE {
            anyhow::bail!("TriStateCheck: value too large");
          }
        }
        if let TriStateSet::Value(x) = &k.set {
          if x.len() > MAX_VALUE_SIZE {
            anyhow::bail!("TriStateSet: value too large");
          }
        }
      }
      Ok(req)
    },
  )
}

pub fn api_kv_prefix_list<'a, 'b, 'c>(
  scope: &mut v8::HandleScope<'a>,
  args: v8::FunctionCallbackArguments<'b>,
  _retval: v8::ReturnValue<'c>,
) -> Result<()> {
  api_kv_generic::<KvPrefixListRequest, _, KvPrefixListResponse, _, _>(
    scope,
    args,
    "kv_prefix_list",
    |scope, rsp| {
      let kvp = rsp.key_value_pairs;
      let out = v8::Array::new(scope, kvp.len() as _);
      for (i, (k, v)) in kvp.into_iter().enumerate() {
        let k = v8::Local::<v8::Value>::from(
          v8::String::new(scope, &k)
            .ok_or_else(|| anyhow::anyhow!("failed to create v8 string"))?,
        );
        let v = v8::Local::<v8::Value>::from(create_uint8array_from_bytes(scope, &v));
        let pair = v8::Array::new(scope, 2);
        pair.set_index(scope, 0, k);
        pair.set_index(scope, 1, v);
        out.set_index(scope, i as u32, pair.into());
      }
      Ok(out.into())
    },
    |_, req| {
      if req.prefix.as_bytes().len() > MAX_KEY_SIZE {
        anyhow::bail!("prefix too large");
      }
      Ok(req)
    },
  )
}

pub fn api_kv_prefix_delete<'a, 'b, 'c>(
  scope: &mut v8::HandleScope<'a>,
  args: v8::FunctionCallbackArguments<'b>,
  _retval: v8::ReturnValue<'c>,
) -> Result<()> {
  api_kv_generic::<KvPrefixDeleteRequest, _, KvPrefixDeleteResponse, _, _>(
    scope,
    args,
    "kv_prefix_delete",
    |scope, _| Ok(v8::undefined(scope).into()),
    |_, req| {
      if req.prefix.as_bytes().len() > MAX_KEY_SIZE {
        anyhow::bail!("prefix too large");
      }
      Ok(req)
    },
  )
}
