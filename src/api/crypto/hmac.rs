use sha2::Sha256;
use v8;

use crate::{
  api::util::{v8_deref_typed_array_assuming_noalias, v8_deserialize},
  v8util::create_uint8array_from_bytes,
};
use anyhow::Result;
use hmac::{Hmac, Mac};
use serde::Deserialize;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
pub struct HmacSha256Params<'s> {
  key: serde_v8::Value<'s>,
  data: serde_v8::Value<'s>,
}

pub fn api_crypto_hmac_sha256(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let params: HmacSha256Params = v8_deserialize(scope, args.get(1))?;
  let key = v8::Local::<v8::TypedArray>::try_from(params.key.v8_value)?;
  let key = unsafe { v8_deref_typed_array_assuming_noalias(scope, key) };
  let data = v8::Local::<v8::TypedArray>::try_from(params.data.v8_value)?;
  let data = unsafe { v8_deref_typed_array_assuming_noalias(scope, data) };
  let mut mac = HmacSha256::new_from_slice(&key[..])?;
  mac.update(&data[..]);
  let result = mac.finalize();
  retval.set(create_uint8array_from_bytes(scope, &result.into_bytes()[..]).into());
  Ok(())
}
