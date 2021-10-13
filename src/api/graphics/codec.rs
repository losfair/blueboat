use crate::api::util::v8_deserialize;
use crate::v8util::create_uint8array_from_bytes;
use crate::{api::util::v8_deref_typed_array_assuming_noalias, impl_idenum};
use anyhow::Result;
use rusty_v8 as v8;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::EncodedImageFormat;
use std::convert::TryFrom;
use thiserror::Error;

use super::CanvasConfig;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CanvasEncodeConfig {
  pub format: CanvasEncodedImageFormat,
  pub quality: u32,
}

impl_idenum!(
  CanvasEncodedImageFormat,
  EncodedImageFormat,
  BMP,
  GIF,
  ICO,
  JPEG,
  PNG,
  WBMP,
  WEBP,
  PKM,
  KTX,
  ASTC,
  DNG,
  HEIF,
);

pub fn api_graphics_canvas_encode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("canvas encode failed")]
  struct CanvasEncodeError;

  let config: CanvasConfig = v8_deserialize(scope, args.get(1))?;
  let encode_config: CanvasEncodeConfig = v8_deserialize(scope, args.get(2))?;
  let fb = v8::Local::<v8::TypedArray>::try_from(args.get(3))?;
  let mut fb = unsafe { v8_deref_typed_array_assuming_noalias(scope, fb) };
  let mut cvs = config.build_canvas(&mut fb)?;

  let pixels = cvs.peek_pixels().unwrap();
  let data = skia_safe::encode::pixmap(
    &pixels,
    encode_config.format.into(),
    encode_config.quality as usize,
  )
  .ok_or(CanvasEncodeError)?;
  let data = create_uint8array_from_bytes(scope, &data);
  retval.set(data.into());

  Ok(())
}
