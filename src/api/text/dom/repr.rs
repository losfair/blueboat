use std::rc::Rc;

use anyhow::Result;
use html5ever::{Attribute, LocalName, QualName};
use markup5ever_rcdom::NodeData;
use serde::{Deserialize, Serialize};
use v8;

use crate::{
  api::util::{v8_deserialize, v8_serialize},
  registry::SymbolRegistry,
};

use super::Dom;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum JsNodeData {
  Text {
    text: String,
  },
  Element {
    name: String,
    attrs: Vec<JsElemAttr>,
  },
  Other,
}

impl From<&NodeData> for JsNodeData {
  fn from(that: &NodeData) -> Self {
    match that {
      NodeData::Element { name, attrs, .. } => Self::Element {
        name: name.local.to_string(),
        attrs: attrs
          .try_borrow()
          .ok()
          .map(|x| {
            x.iter()
              .map(|x| JsElemAttr {
                name: x.name.local.to_string(),
                value: x.value.to_string(),
              })
              .collect()
          })
          .unwrap_or_else(|| vec![]),
      },
      NodeData::Text { contents } => Self::Text {
        text: contents
          .try_borrow()
          .ok()
          .map(|x| x.to_string())
          .unwrap_or_else(|| String::new()),
      },
      _ => Self::Other,
    }
  }
}

impl JsNodeData {
  fn update(self, that: &NodeData) -> Result<()> {
    match (self, that) {
      (JsNodeData::Text { text }, NodeData::Text { contents }) => {
        if let Ok(mut x) = contents.try_borrow_mut() {
          *x = text.into();
        }
      }
      (
        JsNodeData::Element {
          name: js_name,
          attrs: js_attrs,
        },
        NodeData::Element { name, attrs, .. },
      ) => {
        if &name.local != js_name.as_str() {
          anyhow::bail!("cannot modify the name of an element");
        }
        if let Ok(mut x) = attrs.try_borrow_mut() {
          *x = js_attrs
            .into_iter()
            .map(|x| Attribute {
              name: QualName::new(
                None,
                html5ever::Namespace::from(""),
                LocalName::from(x.name.as_str()),
              ),
              value: x.value.into(),
            })
            .collect();
        }
      }
      _ => anyhow::bail!("cannot apply the provided JsNodeData to NodeData"),
    }
    Ok(())
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsElemAttr {
  name: String,
  value: String,
}

pub fn api_dom_get(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let dom: Rc<Dom> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  let data = JsNodeData::from(&dom.node.data);
  retval.set(v8_serialize(scope, &data)?);
  Ok(())
}

pub fn api_dom_update(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let dom: Rc<Dom> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  let data: JsNodeData = v8_deserialize(scope, args.get(2))?;
  data.update(&dom.node.data)?;
  Ok(())
}

pub fn api_dom_remove(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let mut removed = false;
  let dom: Rc<Dom> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  if let Some(x) = dom.node.parent.take() {
    if let Some(x) = x.upgrade() {
      if let Ok(mut children) = x.children.try_borrow_mut() {
        for (i, child) in children.iter().enumerate() {
          if Rc::ptr_eq(child, &dom.node) {
            children.remove(i);
            removed = true;
            break;
          }
        }
      }
    }
  }
  retval.set(v8::Boolean::new(scope, removed).into());
  Ok(())
}

#[cfg(test)]
mod tests {
  use crate::api::testutil::ApiTester;

  #[test]
  fn test_get_and_update() {
    let mut tester = ApiTester::new();
    let out: String = tester.run_script(
      r#"
{
  let dom = TextUtil.DOM.HTML.parse('<div><p class="a">A<span class="a"></span></p></div>', { fragment: true });
  dom.queryWithFilter({type: "hasClass", className: "a"}, elem => {
    const props = elem.get();
    props.attrs.push({name: "data-test", value: "42"});
    elem.update(props);
    return true;
  });
  new TextDecoder().decode(dom.serialize());
}
    "#,
    );
    assert_eq!(out.as_str(), "<html><div><p class=\"a\" data-test=\"42\">A<span class=\"a\" data-test=\"42\"></span></p></div></html>");
  }

  #[test]
  fn test_remove() {
    let mut tester = ApiTester::new();
    let out: String = tester.run_script(
      r#"
{
  let dom = TextUtil.DOM.HTML.parse('<div><p class="a">A<span class="a"></span></p><p class="b">something else</p></div>', { fragment: true });
  dom.queryWithFilter({type: "hasClass", className: "a"}, elem => {
    elem.remove();
    return true;
  });
  new TextDecoder().decode(dom.serialize());
}
    "#,
    );
    assert_eq!(
      out.as_str(),
      "<html><div><p class=\"b\">something else</p></div></html>"
    );
  }
}
