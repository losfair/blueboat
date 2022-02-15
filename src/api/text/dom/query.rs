use std::rc::Rc;

use anyhow::Result;
use markup5ever_rcdom::{Handle, Node, NodeData};
use serde::Deserialize;
use v8;

use crate::{
  api::util::v8_deserialize, registry::SymbolRegistry, v8util::FunctionCallbackArgumentsExt,
};

use super::Dom;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum FilterExpr {
  And {
    left: Box<FilterExpr>,
    right: Box<FilterExpr>,
  },
  Or {
    left: Box<FilterExpr>,
    right: Box<FilterExpr>,
  },
  Tag {
    tag: String,
  },
  HasClass {
    #[serde(rename = "className")]
    class_name: String,
  },
  Text,
  True,
  False,
}

impl FilterExpr {
  fn eval(&self, node: &Node) -> bool {
    match self {
      Self::And { left, right } => left.eval(node) && right.eval(node),
      Self::Or { left, right } => left.eval(node) || right.eval(node),
      Self::Tag { tag } => {
        if let NodeData::Element { name, .. } = &node.data {
          // TODO: prefix
          if &name.local == tag.as_str() {
            return true;
          }
        }
        return false;
      }
      Self::HasClass { class_name } => {
        if let NodeData::Element { attrs, .. } = &node.data {
          if let Ok(attrs) = attrs.try_borrow() {
            for attr in &*attrs {
              // TODO: prefix
              if &attr.name.local == "class" {
                if attr
                  .value
                  .split(' ')
                  .filter(|x| !x.is_empty())
                  .any(|x| x == class_name)
                {
                  return true;
                }
              }
            }
          }
        }
        return false;
      }
      Self::Text => matches!(&node.data, NodeData::Text { .. }),
      Self::True => true,
      Self::False => false,
    }
  }
}

pub fn api_dom_query_with_filter(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let dom: Rc<Dom> = SymbolRegistry::current(scope).cast_and_get(args.get(1))?;
  let expr: FilterExpr = v8_deserialize(scope, args.get(2))?;
  let callback = args.load_function_at(3)?;
  {
    let scope = &mut v8::TryCatch::new(scope);
    let no_exc = walk(scope, &expr, callback, &dom.root, &dom.node);
    if !no_exc {
      scope.rethrow();
      return Ok(());
    }
  }
  Ok(())
}

fn walk(
  scope: &mut v8::TryCatch<v8::HandleScope>,
  expr: &FilterExpr,
  callback: v8::Local<v8::Function>,
  root: &Handle,
  node: &Handle,
) -> bool {
  if expr.eval(&**node) {
    let node = Dom {
      root: root.clone(),
      node: node.clone(),
    };
    let sym = SymbolRegistry::current(scope).put_new(scope, Rc::new(node));

    let undef = v8::undefined(scope);
    let recursive = callback.call(scope, undef.into(), &[sym.into()]);
    if scope.has_caught() {
      return false;
    }

    // Should we recurse?
    if !recursive.map(|x| x.boolean_value(scope)).unwrap_or(true) {
      return true;
    }
  }

  let children = match node.children.try_borrow() {
    Ok(x) => x,
    Err(_) => return true,
  };

  for child in &*children {
    if !walk(scope, expr, callback, root, child) {
      return false;
    }
  }

  true
}

#[cfg(test)]
mod tests {
  use crate::api::testutil::ApiTester;
  use serde::Deserialize;

  #[test]
  fn test_query_count() {
    #[derive(Deserialize)]
    struct Data {
      html: String,
      count1: i32,
      count2: i32,
      count3: i32,
      count4: i32,
      count5: i32,
      count6: i32,
    }
    let mut tester = ApiTester::new();
    let out: Data = tester.run_script(
      r#"
{
  let data = { count1: 0, count2: 0, count3: 0, count4: 0, count5: 0, count6: 0 };
  let dom = TextUtil.DOM.HTMLDOMNode.parse('<div><p class="a">A<span class="a"></span></p><p class="b">B</p></div>');
  dom.queryWithFilter({type: "hasClass", className: "a"}, () => { data.count1++; return true; });
  dom.queryWithFilter({type: "hasClass", className: "a"}, () => { data.count2++; return false; });
  dom.queryWithFilter({type: "hasClass", className: "b"}, () => { data.count3++; return true; });
  dom.queryWithFilter({type: "or", left: {type: "hasClass", className: "a"}, right: {type: "hasClass", className: "b"}}, () => { data.count4++; return true; });
  dom.queryWithFilter({type: "text"}, () => { data.count5++; return true; });
  dom.queryWithFilter({type: "tag", tag: "span"}, () => { data.count6++; return true; });
  data.html = new TextDecoder().decode(dom.serialize());
  data;
}
    "#,
    );
    println!("{}", out.html);
    assert_eq!(out.count1, 2);
    assert_eq!(out.count2, 1);
    assert_eq!(out.count3, 1);
    assert_eq!(out.count4, 3);
    assert_eq!(out.count5, 2);
    assert_eq!(out.count6, 1);
  }
}
