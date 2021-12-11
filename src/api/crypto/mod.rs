pub mod curve25519;
pub mod jwt;

use anyhow::Result;
use rand::Rng;
use std::convert::TryFrom;
use thiserror::Error;
use v8;

use crate::{
  api::util::v8_deref_typed_array_assuming_noalias, v8util::create_uint8array_from_bytes,
};

pub fn api_crypto_digest(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("invalid algorithm")]
  struct InvalidAlg;

  let alg = v8::Local::<v8::String>::try_from(args.get(1))?.to_rust_string_lossy(scope);
  let alg: &'static ring::digest::Algorithm = match alg.as_str() {
    "sha1" => &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
    "sha256" => &ring::digest::SHA256,
    "sha384" => &ring::digest::SHA384,
    "sha512" => &ring::digest::SHA512,
    _ => return Err(InvalidAlg.into()),
  };
  let data = v8::Local::<v8::TypedArray>::try_from(args.get(2))?;
  let data = unsafe { v8_deref_typed_array_assuming_noalias(scope, data) };
  let mut ctx = ring::digest::Context::new(alg);
  ctx.update(&data);
  let output = ctx.finish();
  let output = output.as_ref();
  let output = create_uint8array_from_bytes(scope, output);
  retval.set(output.into());
  Ok(())
}

pub fn api_crypto_getrandom(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let out = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let mut view = unsafe { v8_deref_typed_array_assuming_noalias(scope, out) };
  rand::thread_rng().fill(&mut view[..]);
  retval.set(out.into());
  Ok(())
}

pub fn api_crypto_random_uuid(
  scope: &mut v8::HandleScope,
  _args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let uuid = uuid::Uuid::new_v4();
  let uuid = v8::String::new(scope, &uuid.to_string()).unwrap();
  retval.set(uuid.into());
  Ok(())
}
