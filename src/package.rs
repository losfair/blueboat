use serde::{Deserialize, Serialize};
use std::fmt::Display;

use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  io::Read,
};

use itertools::Itertools;
use tar::{Archive, EntryType};
use v8;

use crate::v8util::create_uint8array_from_bytes;
use std::convert::TryFrom;

#[derive(Clone, Hash, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct PackageKey {
  pub path: String,
  pub version: String,
}

impl PackageKey {
  pub fn unknown() -> Self {
    Self {
      path: "?".to_string(),
      version: "?".to_string(),
    }
  }
}

impl Display for PackageKey {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}[{}]", self.path, self.version)
  }
}

pub struct Package {
  root: VfsNode,
}

enum VfsNode {
  Internal(HashMap<String, VfsNode>),
  Leaf(Vec<u8>),
}

impl Default for VfsNode {
  fn default() -> Self {
    Self::Internal(HashMap::new())
  }
}

impl VfsNode {
  fn insert(&mut self, path: &str, mut r: impl Read) {
    let segs = Package::split_path(path).collect_vec();
    let mut n = self;
    for (i, s) in segs.iter().enumerate() {
      let s = *s;
      let map = match n {
        Self::Internal(x) => x,
        _ => panic!("bad archive: inserting into leaf"),
      };
      if map.get_mut(s).is_some() {
      } else if i + 1 == segs.len() {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).unwrap();
        map.insert(s.to_string(), VfsNode::Leaf(buf));
      } else {
        map.insert(s.to_string(), VfsNode::Internal(HashMap::new()));
      }

      n = map.get_mut(s).unwrap();
    }
  }

  fn trim(&mut self) {
    loop {
      match self {
        Self::Internal(x) => {
          if x.len() == 1 {
            if matches!(&x.iter().next().unwrap().1, VfsNode::Internal(_)) {
              *self = std::mem::replace(x, HashMap::new())
                .into_iter()
                .next()
                .unwrap()
                .1;
              continue;
            }
          }
          break;
        }
        _ => break,
      }
    }
  }
}

impl Package {
  pub fn load(obj: impl Read) -> Self {
    let mut root = VfsNode::default();
    let mut a = Archive::new(obj);
    for ent in a.entries().unwrap() {
      let ent = ent.unwrap();
      if !matches!(ent.header().entry_type(), EntryType::Regular) {
        continue;
      }
      let path = ent.path().unwrap().to_string_lossy().into_owned();
      root.insert(&path, ent);
    }
    root.trim();
    Self { root }
  }

  fn split_path(raw: &str) -> impl Iterator<Item = &str> {
    raw.split("/").filter(|x| !x.is_empty() && *x != ".")
  }

  pub fn pack<'s>(&self, scope: &mut v8::HandleScope<'s>) -> v8::Local<'s, v8::Value> {
    let obj = v8::Object::new(scope);
    Self::collect_all(&self.root, &mut vec![], &mut |k, v| {
      let k = v8::String::new(scope, k).unwrap();
      let v = create_uint8array_from_bytes(scope, v);
      obj.set(scope, k.into(), v.into());
    });
    obj.into()
  }

  fn collect_all<'a, F: FnMut(&str, &[u8])>(n: &'a VfsNode, path: &mut Vec<&'a str>, f: &mut F) {
    match n {
      VfsNode::Leaf(x) => {
        let p = path.join("/");
        f(&p, x);
      }
      VfsNode::Internal(m) => {
        for (k, v) in m {
          path.push(k.as_str());
          Self::collect_all(v, path, f);
          path.pop().unwrap();
        }
      }
    }
  }

  pub fn resolve_abs(&self, raw: &str) -> Option<&[u8]> {
    let mut n = &self.root;
    for s in Self::split_path(raw) {
      n = match n {
        VfsNode::Internal(x) => x.get(s)?,
        _ => return None,
      };
    }
    match n {
      VfsNode::Leaf(x) => Some(x.as_slice()),
      _ => None,
    }
  }

  fn transform_rel_path<'a>(current: &'a str, target: &'a str) -> String {
    let mut cur_segs = Self::split_path(current).collect_vec();

    // Get dirname
    cur_segs.pop();

    for t in Self::split_path(target) {
      if t == ".." {
        cur_segs.pop();
        continue;
      }
      cur_segs.push(t);
    }

    cur_segs.join("/")
  }

  fn load_single_module<'s>(
    &self,
    scope: &mut v8::HandleScope<'s>,
    abs_path: &str,
    script_id: i32,
  ) -> Option<v8::Local<'s, v8::Module>> {
    let module_text = std::str::from_utf8(
      self
        .resolve_abs(&abs_path)
        .unwrap_or_else(|| panic!("unable to resolve module path: {}", abs_path)),
    )
    .unwrap();

    let undef = v8::undefined(scope);
    let code = v8::String::new(scope, module_text).unwrap();
    let name = v8::String::new(scope, &abs_path).unwrap();
    let origin = v8::ScriptOrigin::new(
      scope,
      name.into(),
      0,
      0,
      false,
      script_id,
      undef.into(),
      false,
      false,
      true,
    );
    let source = v8::script_compiler::Source::new(code, Some(&origin));
    let m = v8::script_compiler::compile_module(scope, source)?;
    Some(m)
  }

  fn normalize_path(&self, src: &str) -> String {
    for suffix in &["", ".mjs", ".js", "/index.mjs", "/index.js"] {
      let s = format!("{}{}", src, *suffix);
      if self.resolve_abs(&s).is_some() {
        return Self::split_path(&s).join("/");
      }
    }
    panic!("cannot resolve dependency '{}'", src);
  }

  pub fn load_module_with_dependencies<'s>(
    &self,
    scope: &mut v8::HandleScope<'s>,
    index_path: &str,
  ) -> Option<v8::Local<'s, v8::Module>> {
    let index_path = self.normalize_path(index_path);
    let mut m: HashMap<String, v8::Global<v8::Module>> = HashMap::new();
    let mut q: Vec<(String, String)> = vec![("?".into(), index_path.clone())];
    let mut script_id = 2i32;

    while let Some((current, target)) = q.pop() {
      let path = Self::transform_rel_path(&current, &target);
      let normalized_path = self.normalize_path(&path);

      // Two cases:
      //
      // - We reached the same node.
      // - Aliases to the same normalized path.
      if let Some(x) = m.get(&normalized_path).cloned() {
        m.insert(path, x);
        continue;
      }

      drop(path);
      log::debug!("loading module: {}", normalized_path);

      let module = self.load_single_module(scope, &normalized_path, script_id)?;
      script_id += 1;
      m.insert(normalized_path.clone(), v8::Global::new(scope, module));

      let mreq = module.get_module_requests();
      let n = mreq.length();
      for i in 0..n {
        let mreq = v8::Local::<v8::ModuleRequest>::try_from(mreq.get(scope, i).unwrap()).unwrap();
        let spec = mreq.get_specifier().to_rust_string_lossy(scope);
        q.push((normalized_path.clone(), spec))
      }
    }

    let init = v8::Local::new(scope, m.get(&index_path).unwrap());

    REFERRER_PATHS.with(move |x| {
      *x.borrow_mut() = Some(m.iter().map(|(k, v)| (v.clone(), k.clone())).collect());
      let ret = MODULES.with(move |x| {
        *x.borrow_mut() = Some(m);
        let ret = init.instantiate_module(scope, module_resolve_callback);
        *x.borrow_mut() = None;
        ret
      });
      *x.borrow_mut() = None;
      ret
    })?;

    Some(init)
  }
}

thread_local! {
  static MODULES: RefCell<Option<HashMap<String, v8::Global<v8::Module>>>> = RefCell::new(None);
  static REFERRER_PATHS: RefCell<Option<HashMap<v8::Global<v8::Module>, String>>> = RefCell::new(None);
  static CURRENT_PACKAGE: Cell<Option<&'static Package>> = Cell::new(None);
}

fn module_resolve_callback<'a>(
  context: v8::Local<'a, v8::Context>,
  specifier: v8::Local<'a, v8::String>,
  _import_assertions: v8::Local<'a, v8::FixedArray>,
  referrer: v8::Local<'a, v8::Module>,
) -> Option<v8::Local<'a, v8::Module>> {
  let scope = unsafe { &mut v8::CallbackScope::new(context) };
  let referrer_path = REFERRER_PATHS.with(|x| {
    x.borrow()
      .as_ref()
      .unwrap()
      .get(&*referrer)
      .expect("referrer not found")
      .clone()
  });
  let specifier = specifier.to_rust_string_lossy(scope);
  let path = Package::transform_rel_path(&referrer_path, &specifier);
  log::debug!(
    "module_resolve: {:?} {:?} -> {}",
    referrer_path,
    specifier,
    path
  );
  let module = MODULES
    .with(|x| x.borrow().as_ref().unwrap().get(&path).cloned())
    .unwrap_or_else(|| {
      panic!(
        "module_resolve_callback: cannot resolve module '{}' referenced by '{}'",
        specifier, referrer_path
      )
    });
  Some(v8::Local::new(scope, module))
}
