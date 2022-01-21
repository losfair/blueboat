use anyhow::Result;
use v8;

use crate::registry::SymbolRegistry;

pub fn api_host_object_remove(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let registry = SymbolRegistry::current(scope);
  let sym = v8::Local::<v8::Symbol>::try_from(args.get(1))?;
  registry.remove(sym);
  Ok(())
}
