use std::time::Instant;

use blueboat::{
  api::testutil::ApiTester,
  v8util::{create_uint8array_from_bytes, set_up_v8_globally, ObjectExt},
};
use criterion::{black_box, criterion_group, criterion_main, Bencher, Criterion};
use rand::RngCore;

pub fn run(c: &mut Criterion) {
  c.bench_function("b64 encode 32", |b| {
    run_b64_encode(b, 32);
  });
  c.bench_function("b64 encode 4096", |b| {
    run_b64_encode(b, 4096);
  });
}

criterion_group!(api_benchmark, run);
criterion_main!(api_benchmark);

fn run_b64_encode(b: &mut Bencher, buffer_size: usize) {
  b.iter_custom(|n| {
    let mut api = ApiTester::new();
    api.run(|scope| {
      let g = scope.get_current_context().global(scope);
      let n = v8::Number::new(scope, n as f64);
      g.set_ext(scope, "n", n.into());
      let mut buf = vec![0u8; buffer_size];
      rand::thread_rng().fill_bytes(&mut buf);
      let data = create_uint8array_from_bytes(scope, &buf);
      g.set_ext(scope, "data", data.into());
    });
    let start = Instant::now();
    let _: () = api.run_script(
      r#"
  for(let i = 0; i < n; i++) {
    Codec.b64encode(data);
  }
  "#,
    );
    start.elapsed()
  })
}
