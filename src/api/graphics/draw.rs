use std::convert::TryFrom;

use anyhow::Result;
use rusty_v8 as v8;
use schemars::JsonSchema;
use serde::Deserialize;
use skia_safe::{canvas::SrcRectConstraint, Data, Image, Paint, Rect};
use thiserror::Error;

use crate::api::util::{v8_deref_typed_array_assuming_noalias, v8_deserialize};

use super::CanvasConfig;

#[derive(Deserialize, JsonSchema)]
pub struct CanvasDrawConfig {
  sx: f32,
  sy: f32,
  sw: Option<f32>,
  sh: Option<f32>,
  dx: f32,
  dy: f32,
  dw: Option<f32>,
  dh: Option<f32>,
}

pub fn api_graphics_canvas_draw(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("canvas draw failed")]
  struct CanvasDrawError;

  let src_canvas: CanvasConfig = v8_deserialize(scope, args.get(1))?;
  let src = v8::Local::<v8::TypedArray>::try_from(args.get(2))?;
  let dst_canvas: CanvasConfig = v8_deserialize(scope, args.get(3))?;
  let dst = v8::Local::<v8::TypedArray>::try_from(args.get(4))?;

  let cfg: CanvasDrawConfig = v8_deserialize(scope, args.get(5))?;

  // Don't alias mutable data
  let src_image = {
    let src = unsafe { v8_deref_typed_array_assuming_noalias(scope, src) };
    let image_info = src_canvas.build_image_info();
    let row_bytes = image_info.min_row_bytes();
    Image::from_raster_data(&image_info, Data::new_copy(&src), row_bytes).ok_or(CanvasDrawError)?
  };
  let mut dst = unsafe { v8_deref_typed_array_assuming_noalias(scope, dst) };
  let mut dst = dst_canvas.build_canvas(&mut dst)?;
  let src_rect = cfg.sw.and_then(|sw| {
    cfg.sh.map(|sh| Rect {
      left: cfg.sx,
      right: cfg.sx + sw,
      top: cfg.sy,
      bottom: cfg.sy + sh,
    })
  });
  let dst_rect = cfg
    .dw
    .and_then(|dw| {
      cfg.dh.map(|dh| Rect {
        left: cfg.dx,
        right: cfg.dx + dw,
        top: cfg.dy,
        bottom: cfg.dy + dh,
      })
    })
    .unwrap_or_else(|| Rect {
      left: cfg.dx,
      right: cfg.dx + src_image.width() as f32,
      top: cfg.dy,
      bottom: cfg.dy + src_image.height() as f32,
    });
  dst.draw_image_rect(
    &src_image,
    src_rect.as_ref().map(|x| (x, SrcRectConstraint::Fast)),
    &dst_rect,
    &Paint::default(),
  );
  Ok(())
}
