use std::cell::Cell;

use anyhow::Result;
use std::convert::TryFrom;
use std::ops::Deref;
use thiserror::Error;
use v8;

use crate::{
  api::util::{v8_deref_typed_array_assuming_noalias, TypedArrayView},
  ctx::BlueboatInitData,
};

pub trait FunctionCallbackArgumentsExt<'s> {
  fn load_function_at(&self, i: i32) -> Result<v8::Local<'s, v8::Function>>;
}

impl<'s> FunctionCallbackArgumentsExt<'s> for v8::FunctionCallbackArguments<'s> {
  fn load_function_at(&self, i: i32) -> Result<v8::Local<'s, v8::Function>> {
    #[derive(Error, Debug)]
    #[error("argument at position {0} is not a function")]
    struct NotFunction(i32);
    Ok(v8::Local::<v8::Function>::try_from(self.get(i)).map_err(|_| NotFunction(i))?)
  }
}

pub fn create_arraybuffer_from_bytes<'s>(
  scope: &mut v8::HandleScope<'s>,
  v: &[u8],
) -> v8::Local<'s, v8::ArrayBuffer> {
  let buf = v8::ArrayBuffer::new(scope, v.len());
  {
    let backing = buf.get_backing_store();
    let backing: &[Cell<u8>] = &*backing;
    assert_eq!(backing.len(), v.len());
    for (dst, src) in backing.iter().zip(v.iter()) {
      dst.set(*src);
    }
  }
  buf
}

pub fn create_uint8array_from_bytes<'s>(
  scope: &mut v8::HandleScope<'s>,
  v: &[u8],
) -> v8::Local<'s, v8::Uint8Array> {
  let buf = create_arraybuffer_from_bytes(scope, v);
  let view = v8::Uint8Array::new(scope, buf, 0, v.len()).unwrap();
  view
}

pub trait ObjectExt<'s> {
  fn get_ext(&self, scope: &mut v8::HandleScope<'s>, key: &str) -> v8::Local<'s, v8::Value>;
  fn set_ext(&self, scope: &mut v8::HandleScope<'s>, key: &str, value: v8::Local<'s, v8::Value>);
  fn delete_ext(&self, scope: &mut v8::HandleScope<'s>, key: &str);
}

impl<'s> ObjectExt<'s> for v8::Object {
  fn get_ext(&self, scope: &mut v8::HandleScope<'s>, key: &str) -> v8::Local<'s, v8::Value> {
    let key = v8::String::new(scope, key).unwrap();
    self
      .get(scope, key.into())
      .unwrap_or_else(|| v8::undefined(scope).into())
  }
  fn set_ext(&self, scope: &mut v8::HandleScope<'s>, key: &str, value: v8::Local<'s, v8::Value>) {
    let key = v8::String::new(scope, key).unwrap();
    self.set(scope, key.into(), value);
  }
  fn delete_ext(&self, scope: &mut v8::HandleScope<'s>, key: &str) {
    let key = v8::String::new(scope, key).unwrap();
    self.delete(scope, key.into());
  }
}

pub struct GenericStringView {
  inner: GenericStringViewInner,
}

enum GenericStringViewInner {
  Owned(String),
  View(TypedArrayView),
}

impl Deref for GenericStringView {
  type Target = str;
  fn deref(&self) -> &str {
    match &self.inner {
      GenericStringViewInner::Owned(s) => s,
      GenericStringViewInner::View(v) => {
        let v: &[u8] = &v[..];
        unsafe { std::str::from_utf8_unchecked(v) }
      }
    }
  }
}

pub trait LocalValueExt<'s> {
  fn read_string<'t>(self, scope: &mut v8::HandleScope<'t>) -> Result<String>;
  unsafe fn read_string_assume_noalias<'t>(
    self,
    scope: &mut v8::HandleScope<'t>,
  ) -> Result<GenericStringView>;
}

impl<'s> LocalValueExt<'s> for v8::Local<'s, v8::Value> {
  unsafe fn read_string_assume_noalias<'t>(
    self,
    scope: &mut v8::HandleScope<'t>,
  ) -> Result<GenericStringView> {
    if let Ok(x) = v8::Local::<v8::String>::try_from(self) {
      Ok(GenericStringView {
        inner: GenericStringViewInner::Owned(x.to_rust_string_lossy(scope)),
      })
    } else if let Ok(x) = v8::Local::<v8::TypedArray>::try_from(self) {
      let arr = v8_deref_typed_array_assuming_noalias(scope, x);
      std::str::from_utf8(&arr[..])?;
      Ok(GenericStringView {
        inner: GenericStringViewInner::View(arr),
      })
    } else {
      Err(anyhow::anyhow!(
        "this value cannot be interpreted as a string"
      ))
    }
  }

  fn read_string<'t>(self, scope: &mut v8::HandleScope<'t>) -> Result<String> {
    unsafe {
      self.read_string_assume_noalias(scope).map(|x| match x {
        GenericStringView {
          inner: GenericStringViewInner::Owned(s),
        } => s,
        GenericStringView {
          inner: GenericStringViewInner::View(v),
        } => {
          let v: &[u8] = &v[..];
          std::str::from_utf8_unchecked(v).to_string()
        }
      })
    }
  }
}

pub trait IsolateInitDataExt {
  fn get_init_data(&self) -> &'static BlueboatInitData;
}
impl IsolateInitDataExt for v8::Isolate {
  fn get_init_data(&self) -> &'static BlueboatInitData {
    self.get_slot().copied().expect("missing init data slot")
  }
}
