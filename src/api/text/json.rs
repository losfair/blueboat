use anyhow::Result;
use v8;

use crate::{
  api::util::{v8_deserialize, v8_serialize},
  v8util::{create_uint8array_from_bytes, LocalValueExt},
};

pub fn api_text_json_parse(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let text = unsafe { args.get(1).read_string_assume_noalias(scope)? };
  let value: serde_json::Value = serde_json::from_str(&text)?;
  retval.set(v8_serialize(scope, &value)?);
  Ok(())
}

pub fn api_text_json_to_uint8array(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let value = args.get(1);
  if value.is_undefined() {
    retval.set(v8::undefined(scope).into());
    return Ok(());
  }

  let value: serde_json::Value = v8_deserialize(scope, value)?;
  let text = serde_json::to_string(&value)?;
  let buf = create_uint8array_from_bytes(scope, text.as_bytes());
  retval.set(buf.into());
  Ok(())
}
