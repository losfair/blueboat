use lazy_static::lazy_static;
use std::{
  collections::HashSet,
  ffi::c_void,
  ops::{Deref, DerefMut},
  ptr::NonNull,
  sync::atomic::{AtomicI32, Ordering},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use smr::ipc_channel::ipc::IpcSender;
use thiserror::Error;
use time::PrimitiveDateTime;
use uuid::Uuid;
use v8;

use crate::{
  ctx::BlueboatInitData,
  exec::Executor,
  lpch::{AppLogEntry, LowPriorityMsg},
  package::PackageKey,
};

pub fn v8_deserialize<'s, 't, T: for<'de> Deserialize<'de>>(
  scope: &mut v8::HandleScope<'s>,
  value: v8::Local<'t, v8::Value>,
) -> Result<T> {
  Ok(serde_v8::from_v8(scope, value)?)
}

pub fn v8_serialize<'s, T: Serialize>(
  scope: &mut v8::HandleScope<'s>,
  value: &T,
) -> Result<v8::Local<'s, v8::Value>> {
  Ok(serde_v8::to_v8(scope, value)?)
}

pub fn v8_error<'s>(
  api_name: &str,
  scope: &mut v8::HandleScope<'s>,
  e: &anyhow::Error,
) -> v8::Local<'s, v8::Value> {
  log::error!(
    "app {}: api `{}` is throwing an asynchronous exception: {:?}",
    Executor::try_current()
      .map(|x| format!("{}", x.upgrade().unwrap().ctx.key))
      .unwrap_or_else(|| "<unknown>".to_string()),
    api_name,
    e
  );
  let msg = v8::String::new(scope, &format!("{}", e)).unwrap();
  let exc = v8::Exception::error(scope, msg);
  exc
}

pub fn v8_invoke_callback<'s>(
  api_name: &str,
  scope: &mut v8::HandleScope<'s>,
  res: Result<v8::Local<'s, v8::Value>>,
  callback: &v8::Global<v8::Function>,
) {
  let callback = v8::Local::new(scope, callback);
  let undef = v8::undefined(scope);
  match res {
    Ok(res) => {
      callback.call(scope, undef.into(), &[undef.into(), res]);
    }
    Err(e) => {
      let e = v8_error(api_name, scope, &e);
      callback.call(scope, undef.into(), &[e, undef.into()]);
    }
  }
}

pub fn mk_v8_string<'s>(
  scope: &mut v8::HandleScope<'s>,
  src: &str,
) -> Result<v8::Local<'s, v8::String>> {
  #[derive(Error, Debug)]
  #[error("failed to build a v8 string (source length: {0})")]
  struct StringBuildError(usize);

  Ok(v8::String::new(scope, src).ok_or(StringBuildError(src.len()))?)
}

pub struct TypedArrayView {
  _store: Option<v8::SharedRef<v8::BackingStore>>,
  slice: &'static mut [u8],
}

impl Deref for TypedArrayView {
  type Target = [u8];
  fn deref(&self) -> &Self::Target {
    self.slice
  }
}

impl DerefMut for TypedArrayView {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.slice
  }
}

pub fn ensure_typed_arrays_have_distinct_backing_stores<'s, 't>(
  scope: &mut v8::HandleScope<'s>,
  values: &[v8::Local<'t, v8::TypedArray>],
) -> Result<()> {
  if typed_arrays_have_shared_backing_stores(scope, values) {
    anyhow::bail!("the provided typed arrays have shared backing stores");
  }

  Ok(())
}

pub fn typed_arrays_have_shared_backing_stores<'s, 't>(
  scope: &mut v8::HandleScope<'s>,
  values: &[v8::Local<'t, v8::TypedArray>],
) -> bool {
  let mut backing_store_pointers: HashSet<NonNull<c_void>> = HashSet::new();
  for &v in values {
    let buffer = match v.buffer(scope) {
      Some(x) => x,
      None => continue,
    };

    let store = buffer.get_backing_store();
    let ptr = store.data();
    let ptr = match ptr {
      Some(x) => x,
      None => continue,
    };
    if backing_store_pointers.contains(&ptr) {
      return true;
    }
    backing_store_pointers.insert(ptr);
  }
  false
}

pub fn truncate_typed_array_to_uint8array<'s, 't>(
  scope: &mut v8::HandleScope<'s>,
  value: v8::Local<'t, v8::TypedArray>,
  new_byte_length: usize,
) -> Result<v8::Local<'s, v8::Uint8Array>> {
  let view_offset = value.byte_offset();
  let view_length = value.byte_length();
  let buffer = value
    .buffer(scope)
    .unwrap_or_else(|| v8::ArrayBuffer::new(scope, 0));
  let arr = v8::Uint8Array::new(scope, buffer, view_offset, new_byte_length.min(view_length))
    .ok_or_else(|| anyhow::anyhow!("failed to construct uint8array"))?;
  Ok(arr)
}

pub unsafe fn v8_deref_typed_array_assuming_noalias<'s, 't>(
  scope: &mut v8::HandleScope<'s>,
  value: v8::Local<'t, v8::TypedArray>,
) -> TypedArrayView {
  let view_offset = value.byte_offset();
  let view_length = value.byte_length();
  let buf = match value.buffer(scope) {
    Some(x) => x,
    None => {
      return TypedArrayView {
        _store: None,
        slice: &mut [],
      }
    }
  };
  let store = buf.get_backing_store();
  let view = store
    .data()
    .map(|x| std::slice::from_raw_parts_mut(x.as_ptr() as *mut u8, store.byte_length()))
    .unwrap_or(&mut []);
  let view = &mut view[view_offset..view_offset + view_length];
  TypedArrayView {
    _store: Some(store),
    slice: view,
  }
}

pub unsafe fn v8_deref_arraybuffer_assuming_noalias<'t>(
  buf: v8::Local<'t, v8::ArrayBuffer>,
) -> TypedArrayView {
  let store = buf.get_backing_store();
  let view = store
    .data()
    .map(|x| std::slice::from_raw_parts_mut(x.as_ptr() as *mut u8, store.byte_length()))
    .unwrap_or(&mut []);
  TypedArrayView {
    _store: Some(store),
    slice: view,
  }
}

pub struct ArrayBufferBuilder<'s> {
  buf: v8::Local<'s, v8::ArrayBuffer>,
  _store: v8::SharedRef<v8::BackingStore>,
  slice: &'static mut [u8],
}

impl<'s> Deref for ArrayBufferBuilder<'s> {
  type Target = [u8];
  fn deref(&self) -> &Self::Target {
    self.slice
  }
}

impl<'s> DerefMut for ArrayBufferBuilder<'s> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.slice
  }
}

impl<'s> ArrayBufferBuilder<'s> {
  pub fn new(scope: &mut v8::HandleScope<'s>, len: usize) -> Self {
    let buf = v8::ArrayBuffer::new(scope, len);
    let store = buf.get_backing_store();
    assert_eq!(store.byte_length(), len);
    let slice = unsafe {
      store
        .data()
        .map(|x| std::slice::from_raw_parts_mut(x.as_ptr() as *mut u8, store.byte_length()))
        .unwrap_or(&mut [])
    };
    Self {
      buf,
      _store: store,
      slice,
    }
  }

  pub fn build(self) -> v8::Local<'s, v8::ArrayBuffer> {
    self.buf
  }

  pub fn build_uint8array(
    self,
    scope: &mut v8::HandleScope<'s>,
    length: Option<usize>,
  ) -> v8::Local<'s, v8::Uint8Array> {
    let buf = self.build();
    let view = v8::Uint8Array::new(
      scope,
      buf,
      0,
      buf.byte_length().min(length.unwrap_or(usize::MAX)),
    )
    .unwrap();
    view
  }
}

pub fn write_applog2(
  message: String,
  key: &PackageKey,
  request_id: &str,
  logseq: i32,
  lp_tx: &IpcSender<LowPriorityMsg>,
) {
  log::debug!("applog({})<{}>: {}", key, request_id, message);
  let _ = lp_tx.send(LowPriorityMsg::Log(AppLogEntry {
    app: key.clone(),
    request_id: request_id.to_string(),
    message,
    logseq,
    time: yes_i_want_to_use_now(),
  }));
}

pub fn write_applog(isolate: &mut v8::Isolate, message: String) {
  static INIT_LOGSEQ: AtomicI32 = AtomicI32::new(0);

  lazy_static! {
    static ref INIT_UUID: String = Uuid::new_v4().to_string();
  }

  if let Some(e) = Executor::try_current() {
    let e = e.upgrade().unwrap();
    write_applog2(
      message,
      e.ctx.key,
      &e.request_id,
      e.allocate_logseq(),
      e.ctx.lp_tx,
    );
  } else if let Some(&init_data) = isolate.get_slot::<&'static BlueboatInitData>() {
    write_applog2(
      message,
      &init_data.key,
      &format!("s:init+{}", *INIT_UUID),
      INIT_LOGSEQ.fetch_add(1, Ordering::Relaxed),
      &init_data.lp_tx,
    )
  } else {
    log::warn!("applog: {}", message);
  }
}

#[allow(deprecated)]
fn yes_i_want_to_use_now() -> PrimitiveDateTime {
  PrimitiveDateTime::now()
}
