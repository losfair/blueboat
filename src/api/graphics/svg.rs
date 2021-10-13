use std::convert::TryFrom;

use anyhow::Result;
use rusty_v8 as v8;
use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;
use tiny_skia::PixmapMut;
use usvg::{FitTo, Options, Tree};

use crate::api::util::{v8_deref_typed_array_assuming_noalias, v8_deserialize};

use super::CanvasDimensions;

#[derive(Deserialize, JsonSchema)]
pub struct CanvasRenderSvgConfig {
  pub dimensions: CanvasDimensions,
  pub fit_to: CanvasRenderSvgFitTo,
}

#[derive(Deserialize, JsonSchema, Copy, Clone)]
#[serde(tag = "type")]
pub enum CanvasRenderSvgFitTo {
  Original,
  Width { width: u32 },
  Height { height: u32 },
  Size { width: u32, height: u32 },
  Zoom { zoom: f32 },
}

impl From<CanvasRenderSvgFitTo> for FitTo {
  fn from(that: CanvasRenderSvgFitTo) -> Self {
    use CanvasRenderSvgFitTo as V;
    match that {
      V::Original => Self::Original,
      V::Width { width } => Self::Width(width),
      V::Height { height } => Self::Height(height),
      V::Size { width, height } => Self::Size(width, height),
      V::Zoom { zoom } => Self::Zoom(zoom),
    }
  }
}

pub fn api_graphics_canvas_render_svg(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("svg render parameter error")]
  struct ParamError;

  #[derive(Error, Debug)]
  #[error("svg render error")]
  struct RenderError;

  let svg = args.get(1).to_rust_string_lossy(scope);
  let cfg: CanvasRenderSvgConfig = v8_deserialize(scope, args.get(2))?;
  let fb = v8::Local::<v8::TypedArray>::try_from(args.get(3))?;

  let mut fb = unsafe { v8_deref_typed_array_assuming_noalias(scope, fb) };
  let tree_opt = Options::default();
  let tree = Tree::from_str(&svg, &tree_opt.to_ref())?;
  let pixmap = PixmapMut::from_bytes(
    &mut fb,
    cfg.dimensions.width as u32,
    cfg.dimensions.height as u32,
  )
  .ok_or(ParamError)?;
  resvg::render(&tree, cfg.fit_to.into(), pixmap).ok_or(RenderError)?;
  Ok(())
}
