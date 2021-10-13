use anyhow::Result;
use rusty_v8 as v8;

use crate::{
  api::util::{mk_v8_string, v8_deserialize, v8_serialize},
  v8util::LocalValueExt,
};

pub fn api_text_yaml_parse(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let text = args.get(1).read_string(scope)?;
  let value: serde_json::Value = serde_yaml::from_str(&text)?;
  retval.set(v8_serialize(scope, &value)?);
  Ok(())
}

pub fn api_text_yaml_stringify(
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
  let text = serde_yaml::to_string(&value)?;
  retval.set(mk_v8_string(scope, &text)?.into());
  Ok(())
}
