use serde::Deserialize;

use crate::{
  bootstrap::JSLAND_SNAPSHOT,
  ctx::{native_invoke_entry, NI_ENTRY_KEY},
  registry::SymbolRegistry,
  v8util::set_up_v8_globally,
};

use super::util::v8_deserialize;

pub struct ApiTester {
  isolate: v8::OwnedIsolate,
  global_ctx: v8::Global<v8::Context>,
}

fn ensure_init() {
  static ONCE: std::sync::Once = std::sync::Once::new();
  ONCE.call_once(|| {
    set_up_v8_globally();
  });
}

impl ApiTester {
  pub fn new() -> Self {
    ensure_init();

    let mut isolate = v8::Isolate::new(v8::CreateParams::default().snapshot_blob(JSLAND_SNAPSHOT));
    isolate.set_slot(SymbolRegistry::new());

    let global_ctx: v8::Global<v8::Context>;
    {
      let scope = &mut v8::HandleScope::new(&mut isolate);
      let obj_t = v8::ObjectTemplate::new(scope);

      let blueboat_host_invoke = v8::FunctionTemplate::new(scope, native_invoke_entry);
      obj_t.set(
        v8::String::new(scope, NI_ENTRY_KEY).unwrap().into(),
        blueboat_host_invoke.into(),
      );

      let ctx = v8::Context::new_from_template(scope, obj_t);
      global_ctx = v8::Global::new(scope, ctx);
    }

    ApiTester {
      isolate,
      global_ctx,
    }
  }

  pub fn run<F: FnOnce(&mut v8::HandleScope) -> R, R>(&mut self, f: F) -> R {
    let scope = &mut v8::HandleScope::new(&mut self.isolate);
    let local_ctx = v8::Local::new(scope, &self.global_ctx);
    let mut scope = v8::ContextScope::new(scope, local_ctx);
    f(&mut scope)
  }

  pub fn run_script<T: for<'a> Deserialize<'a>>(&mut self, text: &str) -> T {
    let scope = &mut v8::HandleScope::new(&mut self.isolate);
    let local_ctx = v8::Local::new(scope, &self.global_ctx);
    let scope = &mut v8::ContextScope::new(scope, local_ctx);
    let text = v8::String::new(scope, text).expect("string construction failed");
    let script = v8::Script::compile(scope, text, None).expect("compile failed");
    let out = script.run(scope).expect("run failed");
    v8_deserialize(scope, out).unwrap()
  }
}
