pub mod multipart;

use super::util::v8_deserialize;
use crate::{api::util::ArrayBufferBuilder, v8util::LocalValueExt};
use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use v8;

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
  let data = unsafe { args.get(1).read_bytes_assume_noalias(scope) }?;
  let out = hex::encode(&data[..]);
  retval.set(v8::String::new(scope, &out).unwrap().into());
  Ok(())
}

pub fn api_codec_hexencode_to_uint8array(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = unsafe { args.get(1).read_bytes_assume_noalias(scope) }?;
  let data = &data[..];
  let mut builder = ArrayBufferBuilder::new(scope, data.len() * 2);
  hex::encode_to_slice(data, &mut builder).unwrap();
  retval.set(builder.build_uint8array(scope, None).into());
  Ok(())
}

pub fn api_codec_hexdecode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = unsafe { args.get(1).read_bytes_assume_noalias(scope)? };
  let data = &data[..];
  let mut out = ArrayBufferBuilder::new(scope, data.len() / 2);
  hex::decode_to_slice(data, &mut out)?;
  retval.set(out.build_uint8array(scope, None).into());
  Ok(())
}

pub fn api_codec_b64encode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = unsafe { args.get(1).read_bytes_assume_noalias(scope) }?;
  let mode: CodecBase64Mode = v8_deserialize(scope, args.get(2))?;
  let out = base64::encode_config(&data[..], mode.build_config());
  retval.set(v8::String::new(scope, &out).unwrap().into());
  Ok(())
}

pub fn api_codec_b64encode_to_uint8array(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = unsafe { args.get(1).read_bytes_assume_noalias(scope) }?;
  let data = &data[..];
  let mode: CodecBase64Mode = v8_deserialize(scope, args.get(2))?;
  let mut builder = ArrayBufferBuilder::new(scope, data.len() * 4 / 3 + 4);
  let n = base64::encode_config_slice(data, mode.build_config(), &mut builder);
  retval.set(builder.build_uint8array(scope, Some(n)).into());
  Ok(())
}

pub fn api_codec_b64decode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let data = unsafe { args.get(1).read_bytes_assume_noalias(scope)? };
  let data = &data[..];
  let mode: CodecBase64Mode = v8_deserialize(scope, args.get(2))?;
  let mut out = ArrayBufferBuilder::new(scope, (data.len() + 3) / 4 * 3);

  let n = base64::decode_config_slice(data, mode.build_config(), &mut out)?;
  retval.set(out.build_uint8array(scope, Some(n)).into());
  Ok(())
}
