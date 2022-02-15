use std::rc::Rc;

use anyhow::Result;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{RcDom, SerializableHandle};
use serde::Deserialize;
use v8;

use crate::{
  api::util::v8_deserialize,
  registry::SymbolRegistry,
  v8util::{create_uint8array_from_bytes, LocalValueExt},
};

pub struct Dom {
  rcd: RcDom,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HtmlParseOptions {
  #[serde(default)]
  fragment: bool,
}

pub fn api_dom_html_parse(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let text = unsafe { args.get(1).read_string_assume_noalias(scope)? };
  let opts = args.get(2);
  let opts: HtmlParseOptions = if opts.is_null_or_undefined() {
    Default::default()
  } else {
    v8_deserialize(scope, opts)?
  };
  let parse_opts = html5ever::ParseOpts {
    ..Default::default()
  };
  let rcd = if opts.fragment {
    html5ever::parse_fragment(
      RcDom::default(),
      parse_opts,
      markup5ever::QualName::new(
        None,
        html5ever::Namespace::from("http://www.w3.org/1999/xhtml"),
        html5ever::LocalName::from("body"),
      ),
      vec![],
    )
  } else {
    html5ever::parse_document(RcDom::default(), parse_opts)
  }
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
    html5ever::serialize::SerializeOpts {
      // prevent panic
      create_missing_parent: true,
      ..Default::default()
    },
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
{
  let dom = new TextUtil.DOM.HtmlDOM("<!DOCTYPE html><html><body></body></html>");
  new TextDecoder().decode(dom.serialize());
}
    "#,
    );
    assert_eq!(
      out.as_str(),
      "<!DOCTYPE html><html><head></head><body></body></html>"
    );
    let out: String = tester.run_script(
      r#"
{
  let dom = new TextUtil.DOM.HtmlDOM("<p>hello</p>", { fragment: true });
  new TextDecoder().decode(dom.serialize());
}
    "#,
    );
    assert_eq!(out.as_str(), "<html><p>hello</p></html>");
  }

  #[test]
  fn test_xml_roundtrip() {
    let mut tester = ApiTester::new();
    let out: String = tester.run_script(
      r#"
{
  let dom = new TextUtil.DOM.XmlDOM(`
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel><title>A</title></channel>
  <channel><title>B</title></channel>
</rss>
`);
  new TextDecoder().decode(dom.serialize());
}
    "#,
    );
    assert_eq!(
      out.as_str(),
      "<?xml version=\"1.0\" encoding=\"utf-8\"?><rss version=\"2.0\">\n  <channel><title>A</title></channel>\n  <channel><title>B</title></channel>\n</rss>"
    );
  }
}
