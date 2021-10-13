pub mod multipart;

use super::util::{v8_deref_typed_array_assuming_noalias, v8_deserialize};
use crate::api::util::ArrayBufferBuilder;
use anyhow::Result;
use rusty_v8 as v8;
use schemars::JsonSchema;
use serde::Deserialize;
use std::convert::TryFrom;
use thiserror::Error;

#[derive(Deserialize, JsonSchema)]
pub enum CodecBase64Mode {
  #[serde(rename = "standard")]
  Standard,
  #[serde(rename = "standard-nopad")]
  StandardNopad,
  #[serde(rename = "urlsafe")]
  Urlsafe,
  #[serde(rename = "urlsafe-nopad")]
  UrlsafeNopad,
}

impl CodecBase64Mode {
  fn build_config(&self) -> base64::Config {
    match self {
      Self::Standard => base64::STANDARD,
      Self::StandardNopad => base64::STANDARD_NO_PAD,
      Self::Urlsafe => base64::URL_SAFE,
      Self::UrlsafeNopad => base64::URL_SAFE_NO_PAD,
    }
  }
}

pub fn api_codec_hexencode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let data = unsafe { v8_deref_typed_array_assuming_noalias(scope, data) };
  let out = hex::encode(&data[..]);
  retval.set(v8::String::new(scope, &out).unwrap().into());
  Ok(())
}

pub fn api_codec_hexdecode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("invalid hex string")]
  struct Invalid;

  let data = v8::Local::<v8::String>::try_from(args.get(1))?;
  if !data.contains_only_onebyte() || data.length() % 2 != 0 {
    return Err(Invalid.into());
  }

  let mut buf = vec![0u8; data.length()];
  data.write_one_byte(scope, &mut buf, 0, v8::WriteOptions::NO_NULL_TERMINATION);
  let mut out = ArrayBufferBuilder::new(scope, buf.len() / 2);

  hex::decode_to_slice(&buf, &mut out)?;
  retval.set(out.build_uint8array(scope, None).into());
  Ok(())
}

pub fn api_codec_b64encode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let data = unsafe { v8_deref_typed_array_assuming_noalias(scope, data) };
  let mode: CodecBase64Mode = v8_deserialize(scope, args.get(2))?;
  let out = base64::encode_config(&data[..], mode.build_config());
  retval.set(v8::String::new(scope, &out).unwrap().into());
  Ok(())
}

pub fn api_codec_b64decode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("invalid base64 string")]
  struct Invalid;

  let data = v8::Local::<v8::String>::try_from(args.get(1))?;
  if !data.contains_only_onebyte() {
    return Err(Invalid.into());
  }
  let mode: CodecBase64Mode = v8_deserialize(scope, args.get(2))?;

  let mut buf = vec![0u8; data.length()];
  data.write_one_byte(scope, &mut buf, 0, v8::WriteOptions::NO_NULL_TERMINATION);
  let mut out = ArrayBufferBuilder::new(scope, (buf.len() + 3) / 4 * 3);

  let n = base64::decode_config_slice(&buf, mode.build_config(), &mut out)?;
  retval.set(out.build_uint8array(scope, Some(n)).into());
  Ok(())
}
