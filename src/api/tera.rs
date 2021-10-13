use anyhow::Result;
use rusty_v8 as v8;
use tera::{Context, Tera};

use super::util::{mk_v8_string, v8_deserialize};

pub fn api_tera_render(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let template: String = args.get(1).to_rust_string_lossy(scope);
  let context: serde_json::Value = v8_deserialize(scope, args.get(2))?;
  let disable_autoescape = args.get(3).boolean_value(scope);
  let context = Context::from_value(context)?;
  let output = Tera::one_off(&template, &context, !disable_autoescape)?;
  retval.set(mk_v8_string(scope, &output)?.into());
  Ok(())
}
