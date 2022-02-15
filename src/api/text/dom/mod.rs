use std::rc::Rc;

use anyhow::Result;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{RcDom, SerializableHandle};
use v8;

use crate::{
  registry::SymbolRegistry,
  v8util::{create_uint8array_from_bytes, LocalValueExt},
};

pub struct Dom {
  rcd: RcDom,
}

pub fn api_dom_html_parse(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let text = unsafe { args.get(1).read_string_assume_noalias(scope)? };
  let rcd = html5ever::parse_document(
    RcDom::default(),
    html5ever::ParseOpts {
      ..Default::default()
    },
  )
  .from_utf8()
  .read_from(&mut text.as_bytes())
  .map_err(|e| anyhow::Error::from(e).context("failed to parse html document"))?;
  let value = Dom { rcd };
  let sym = SymbolRegistry::current(scope).put_new(scope, Rc::new(value));
  retval.set(sym.into());
  Ok(())
}

pub fn api_dom_xml_parse(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let text = unsafe { args.get(1).read_string_assume_noalias(scope)? };
  let rcd = xml5ever::driver::parse_document(
    RcDom::default(),
    xml5ever::driver::XmlParseOpts {
      ..Default::default()
    },
  )
  .from_utf8()
  .read_from(&mut text.as_bytes())
  .map_err(|e| anyhow::Error::from(e).context("failed to parse xml document"))?;
  let value = Dom { rcd };
  let sym = SymbolRegistry::current(scope).put_new(scope, Rc::new(value));
  retval.set(sym.into());
  Ok(())
}

pub fn api_dom_html_serialize(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let dom: Rc<Dom> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  let mut bytes: Vec<u8> = vec![];
  html5ever::serialize::serialize(
    &mut bytes,
    &SerializableHandle::from(dom.rcd.document.clone()),
    html5ever::serialize::SerializeOpts::default(),
  )
  .map_err(|e| anyhow::Error::from(e).context("html serialization failed"))?;
  retval.set(create_uint8array_from_bytes(scope, &bytes).into());
  Ok(())
}

pub fn api_dom_xml_serialize(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let dom: Rc<Dom> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  let mut bytes: Vec<u8> = vec![];
  xml5ever::serialize::serialize(
    &mut bytes,
    &SerializableHandle::from(dom.rcd.document.clone()),
    xml5ever::serialize::SerializeOpts::default(),
  )
  .map_err(|e| anyhow::Error::from(e).context("xml serialization failed"))?;
  retval.set(create_uint8array_from_bytes(scope, &bytes).into());
  Ok(())
}

#[cfg(test)]
mod tests {
  use crate::api::testutil::ApiTester;

  #[test]
  fn test_html_roundtrip() {
    let mut tester = ApiTester::new();
    let out: String = tester.run_script(
      r#"
let dom = new TextUtil.DOM.HtmlDOM("<!DOCTYPE html><html><body></body></html>");
new TextDecoder().decode(dom.serialize());
    "#,
    );
    assert_eq!(
      out.as_str(),
      "<!DOCTYPE html><html><head></head><body></body></html>"
    );
  }
}
