use anyhow::Result;

use crate::v8util::{create_uint8array_from_bytes, LocalValueExt};

pub fn api_encode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let s = v8::Local::<v8::String>::try_from(args.get(1))?
    .to_rust_string_lossy(scope)
    .into_bytes();
  let buf = create_uint8array_from_bytes(scope, &s);
  retval.set(buf.into());
  Ok(())
}

pub fn api_decode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let s = unsafe { args.get(1).read_string_assume_noalias(scope) }?;
  retval.set(
    v8::String::new(scope, &s)
      .expect("api_decode: failed to create v8 string")
      .into(),
  );
  Ok(())
}

#[cfg(test)]
mod tests {
  use crate::api::testutil::ApiTester;

  #[test]
  fn test_encode() {
    let mut tester = ApiTester::new();

    // Encode something
    let out: i32 = tester.run_script(r#"new TextEncoder().encode("a")[0];"#);
    assert_eq!(out, 97);

    // Encode nothing
    let out: i32 = tester.run_script(r#"new TextEncoder().encode().length;"#);
    assert_eq!(out, 0);
  }

  #[test]
  fn test_decode() {
    let mut tester = ApiTester::new();

    // Decode Uint8Array
    let out: String = tester.run_script(r#"new TextDecoder().decode(new Uint8Array([97]));"#);
    assert_eq!(out.as_str(), "a");

    // Decode buffer
    let out: String =
      tester.run_script(r#"new TextDecoder().decode(new Uint8Array([98]).buffer);"#);
    assert_eq!(out.as_str(), "b");

    // Decode nothing
    let out: String = tester.run_script(r#"new TextDecoder().decode();"#);
    assert_eq!(out.as_str(), "");
  }
}
