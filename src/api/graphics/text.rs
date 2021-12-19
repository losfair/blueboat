use anyhow::Result;
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use v8;

use crate::api::util::{v8_deserialize, v8_serialize};

use super::{font_util, fonts::search_font};

#[derive(Deserialize, JsonSchema, Clone)]
pub struct GraphicsTextMeasureSettings {
  text: String,
  font: String,
  max_width: f32,
}

#[derive(Serialize, JsonSchema, Clone)]
pub struct GraphicsTextMeasureOutput {
  height: f32,
  lines: u32,
  glyphs: Vec<GraphicsTextMeasureGlyph>,
}

#[derive(Serialize, JsonSchema, Clone)]
pub struct GraphicsTextMeasureGlyph {
  x: f32,
  y: f32,
  width: usize,
  height: usize,
}

pub fn api_graphics_text_measure(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let settings: GraphicsTextMeasureSettings = v8_deserialize(scope, args.get(1))?;
  let mut layout: Layout<()> = Layout::new(CoordinateSystem::PositiveYDown);
  layout.reset(&LayoutSettings {
    max_width: Some(settings.max_width),
    ..Default::default()
  });
  let font = font_util::Font::new(&settings.font)?;
  let fonts = search_font(&font);
  if fonts.len() == 0 {
    anyhow::bail!("No available fonts");
  }

  let mut sbuf = String::new();
  for ch in settings.text.chars() {
    sbuf.clear();
    sbuf.push(ch);
    let font_index = fonts
      .iter()
      .copied()
      .enumerate()
      .find(|(_, x)| x.lookup_glyph_index(ch) != 0)
      .map(|(i, _)| i)
      .unwrap_or_else(|| 0);
    layout.append(&fonts, &TextStyle::new(&sbuf, font.size, font_index));
  }

  let out = GraphicsTextMeasureOutput {
    height: layout.height(),
    lines: layout.lines() as u32,
    glyphs: layout
      .glyphs()
      .iter()
      .map(|x| GraphicsTextMeasureGlyph {
        x: x.x,
        y: x.y,
        width: x.width,
        height: x.height,
      })
      .collect(),
  };
  retval.set(v8_serialize(scope, &out)?);

  Ok(())
}
