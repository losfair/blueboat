use anyhow::Result;
use rusty_v8 as v8;
use std::{any::Any, cell::RefCell, collections::HashMap, convert::TryFrom, rc::Rc};
use thiserror::Error;

pub struct SymbolRegistry {
  symbols: RefCell<HashMap<v8::Global<v8::Symbol>, Rc<dyn Any>>>,
}

impl SymbolRegistry {
  pub fn new() -> Rc<Self> {
    Rc::new(Self {
      symbols: RefCell::new(HashMap::new()),
    })
  }

  pub fn current(isolate: &mut v8::Isolate) -> Rc<Self> {
    isolate
      .get_slot::<Rc<Self>>()
      .expect("no current symbol registry")
      .clone()
  }

  pub fn clear(&self) {
    self.symbols.borrow_mut().clear();
  }

  pub fn put<'s>(
    &self,
    scope: &mut v8::HandleScope<'s, ()>,
    k: v8::Local<'s, v8::Symbol>,
    v: Rc<dyn Any>,
  ) {
    self
      .symbols
      .borrow_mut()
      .insert(v8::Global::new(scope, k), v);
  }

  pub fn put_new<'s>(
    &self,
    scope: &mut v8::HandleScope<'s, ()>,
    v: Rc<dyn Any>,
  ) -> v8::Local<'s, v8::Symbol> {
    let desc = v8::String::new(scope, "native_symbol").unwrap();
    let k = v8::Symbol::new(scope, Some(desc));
    self.put(scope, k, v);
    k
  }

  pub fn cast_and_get<'s, T: 'static>(&self, k: v8::Local<'s, v8::Value>) -> Result<Rc<T>> {
    let k = v8::Local::<v8::Symbol>::try_from(k)?;
    self.get(&*k)
  }

  pub fn get<T: 'static>(&self, k: &v8::Symbol) -> Result<Rc<T>> {
    #[derive(Error, Debug)]
    #[error("symbol not registered")]
    struct NotRegistered;

    #[derive(Error, Debug)]
    #[error("symbol type mismatch")]
    struct TypeMismatch;

    let sym = self.symbols.borrow().get(k).ok_or(NotRegistered)?.clone();
    Ok(sym.downcast().map_err(|_| TypeMismatch)?)
  }
}
