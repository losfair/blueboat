use anyhow::Result;
use jtd::{Schema, SerdeSchema, ValidateOptions};
use rusty_v8 as v8;
use std::rc::Rc;

use crate::{
  api::util::{mk_v8_string, v8_deserialize},
  registry::SymbolRegistry,
};

pub fn api_jtd_load_schema(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let schema: SerdeSchema = v8_deserialize(scope, args.get(1))?;
  let schema = Rc::new(Schema::from_serde_schema(schema)?);
  let sym = SymbolRegistry::current(scope).put_new(scope, schema);
  retval.set(sym.into());
  Ok(())
}

pub fn api_jtd_validate(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let schema: Rc<Schema> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  let value: serde_json::Value = v8_deserialize(scope, args.get(2))?;
  match jtd::validate(
    &schema,
    &value,
    ValidateOptions::new()
      .with_max_depth(128)
      .with_max_errors(1),
  ) {
    Ok(errors) => {
      if !errors.is_empty() {
        let desc = mk_v8_string(scope, &format!("{:?}", errors[0]))?;
        retval.set(desc.into());
      }
    }
    Err(e) => {
      let desc = mk_v8_string(scope, &format!("{}", e))?;
      retval.set(desc.into());
    }
  }
  Ok(())
}
