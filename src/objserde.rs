use std::{cell::RefCell, rc::Rc};

use anyhow::Result;
use rusty_v8 as v8;
use thiserror::Error;

pub fn serialize_v8_value<'s>(
  scope: &mut v8::HandleScope<'s>,
  value: v8::Local<v8::Value>,
) -> Result<Vec<u8>> {
  #[derive(Error, Debug)]
  #[error("data clone error: {0}")]
  struct DcError(String);

  #[derive(Clone)]
  struct I(Rc<RefCell<Option<String>>>);
  impl v8::ValueSerializerImpl for I {
    fn throw_data_clone_error<'s>(
      &mut self,
      scope: &mut v8::HandleScope<'s>,
      message: v8::Local<'s, v8::String>,
    ) {
      let mut inner = self.0.borrow_mut();
      if inner.is_none() {
        *inner = Some(message.to_rust_string_lossy(scope));
      }
    }
  }

  let i = I(Rc::new(RefCell::new(None)));
  let ctx = scope.get_current_context();
  let mut vs = v8::ValueSerializer::new(scope, Box::new(i.clone()));
  vs.write_value(ctx, value);
  let out = vs.release();

  if let Some(e) = i.0.borrow_mut().take() {
    return Err(DcError(e).into());
  }

  Ok(out)
}

pub fn deserialize_v8_value<'s>(
  scope: &mut v8::HandleScope<'s>,
  bytes: &[u8],
) -> Result<v8::Local<'s, v8::Value>> {
  #[derive(Error, Debug)]
  #[error("v8 value deserialization error")]
  struct Generic;

  struct I;
  impl v8::ValueDeserializerImpl for I {}

  let ctx = scope.get_current_context();
  let mut vds = v8::ValueDeserializer::new(scope, Box::new(I), bytes);
  let out = vds.read_value(ctx).ok_or(Generic)?;
  Ok(out)
}
