use std::cell::Cell;

use anyhow::Result;
use rusty_v8 as v8;
use std::convert::TryFrom;
use thiserror::Error;

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

pub trait LocalValueExt<'s> {
  fn read_string<'t>(self, scope: &mut v8::HandleScope<'t>) -> Result<String>;
}

impl<'s> LocalValueExt<'s> for v8::Local<'s, v8::Value> {
  fn read_string<'t>(self, scope: &mut v8::HandleScope<'t>) -> Result<String> {
    let s = v8::Local::<v8::String>::try_from(self)?;
    Ok(s.to_rust_string_lossy(scope))
  }
}
