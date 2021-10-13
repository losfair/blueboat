pub mod codec;
pub mod draw;
mod font_util;
pub mod fonts;
pub mod layout;
pub mod svg;

use anyhow::Result;
use fontdue::{
  layout::{CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle},
  Font,
};
use rusty_v8 as v8;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{
  AlphaType, BlendMode, Canvas, Color, Color4f, ColorType, Data, ISize, Image, ImageInfo, Matrix,
  OwnedCanvas, Paint, PaintStyle, Path, PathFillType, PixelGeometry, Point, Rect, SurfaceProps,
  SurfacePropsFlags, Vector,
};
use std::convert::TryFrom;
use thiserror::Error;

use crate::api::{
  graphics::fonts::search_font,
  util::{v8_deref_typed_array_assuming_noalias, write_applog},
};

use super::util::v8_deserialize;

#[macro_export]
macro_rules! impl_idenum {
  ($src: ident, $dst: ident, $($variant: ident,)*) => {
    #[derive(Serialize, Deserialize, JsonSchema, Copy, Clone)]
    pub enum $src {
      $($variant,)*
    }
    impl From<$src> for $dst {
      fn from(src: $src) -> Self {
        match src {
          $($src::$variant => Self::$variant,)*
        }
      }
    }
  }
}

#[macro_export]
macro_rules! impl_idstruct {
  ($src: ident, $dst: ident, $($field:ident : $field_ty:ty,)*) => {
    #[derive(Serialize, Deserialize, JsonSchema, Copy, Clone)]
    pub struct $src {
      $(pub $field: $field_ty,)*
    }
    impl From<$src> for $dst {
      fn from(src: $src) -> Self {
        Self {
          $($field: src.$field,)*
        }
      }
    }
  }
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CanvasConfig {
  pub dimensions: CanvasDimensions,
  pub color_type: CanvasColorType,
  pub alpha_type: CanvasAlphaType,
  pub pixel_geometry: CanvasPixelGeometry,
}

impl CanvasConfig {
  pub fn build_image_info(&self) -> ImageInfo {
    ImageInfo::new(
      ISize {
        width: self.dimensions.width,
        height: self.dimensions.height,
      },
      ColorType::from(self.color_type),
      AlphaType::from(self.alpha_type),
      None,
    )
  }

  pub fn build_canvas<'a>(&self, fb: &'a mut [u8]) -> Result<OwnedCanvas<'a>> {
    #[derive(Error, Debug)]
    #[error("canvas creation failed")]
    struct CanvasCreationError;

    let image_info = self.build_image_info();

    let cvs = Canvas::from_raster_direct(
      &image_info,
      fb,
      None,
      Some(&SurfaceProps::new(
        SurfacePropsFlags::default(),
        self.pixel_geometry.into(),
      )),
    )
    .ok_or_else(|| CanvasCreationError)?;
    Ok(cvs)
  }
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CanvasDimensions {
  pub width: i32,
  pub height: i32,
}

impl_idenum!(CanvasPixelGeometry, PixelGeometry, RGBH, BGRH, RGBV, BGRV,);

impl_idenum!(
  CanvasColorType,
  ColorType,
  Alpha8,
  RGB565,
  ARGB4444,
  RGBA8888,
  RGB888x,
  BGRA8888,
  RGBA1010102,
  BGRA1010102,
  RGB101010x,
  BGR101010x,
  Gray8,
  RGBAF16Norm,
  RGBAF16,
  RGBAF32,
  R8G8UNorm,
  A16Float,
  R16G16Float,
  A16UNorm,
  R16G16UNorm,
  R16G16B16A16UNorm,
);

impl_idenum!(CanvasAlphaType, AlphaType, Opaque, Premul, Unpremul,);

impl_idstruct!(CanvasColor4f, Color4f, r: f32, g: f32, b: f32, a: f32,);
impl_idstruct!(
  CanvasRect,
  Rect,
  left: f32,
  top: f32,
  right: f32,
  bottom: f32,
);

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CanvasMatrix {
  pub a: f32,
  pub b: f32,
  pub c: f32,
  pub d: f32,
  pub e: f32,
  pub f: f32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op")]
pub enum CanvasPathOp {
  AddPath {
    subpath: Vec<CanvasPathOp>,
    matrix: Option<CanvasMatrix>,
  },
  MoveTo {
    x: f32,
    y: f32,
  },
  LineTo {
    x: f32,
    y: f32,
  },
  BezierCurveTo {
    cp1x: f32,
    cp1y: f32,
    cp2x: f32,
    cp2y: f32,
    x: f32,
    y: f32,
  },
  QuadraticCurveTo {
    cpx: f32,
    cpy: f32,
    x: f32,
    y: f32,
  },
  Arc {
    x: f32,
    y: f32,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    counterclockwise: bool,
  },
  ArcTo {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    radius: f32,
  },
  Ellipse {
    x: f32,
    y: f32,
    radius_x: f32,
    radius_y: f32,
    rotation: f32,
    start_angle: f32,
    end_angle: f32,
    counterclockwise: bool,
  },
  Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
  },
  Close,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op")]
pub enum CanvasOp {
  SetStrokeStyleColor {
    color: String,
  },
  SetStrokeLineWidth {
    width: f32,
  },
  SetFillStyleColor {
    color: String,
  },
  SetFont {
    font: String,
  },
  Stroke {
    path: Vec<CanvasPathOp>,
  },
  Fill {
    path: Vec<CanvasPathOp>,
    fill_rule: CanvasFillRule,
  },
  FillText {
    text: String,
    x: f32,
    y: f32,
  },
  ClearRect {
    rect: CanvasRect,
  },
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub enum CanvasFillRule {
  #[serde(rename = "nonzero")]
  NonZero,
  #[serde(rename = "evenodd")]
  EvenOdd,
}

pub fn api_graphics_canvas_commit(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("canvas commit error at op {0}: {1}")]
  struct CommitError(usize, anyhow::Error);

  let config: CanvasConfig = v8_deserialize(scope, args.get(1))?;
  let ops: Vec<CanvasOp> = v8_deserialize(scope, args.get(2))?;
  let fb = v8::Local::<v8::TypedArray>::try_from(args.get(3))?;

  let mut fb = unsafe { v8_deref_typed_array_assuming_noalias(scope, fb) };
  let mut cvs = config.build_canvas(&mut fb)?;

  {
    let mut applier = CommitApplier {
      cvs: &mut cvs,
      stroke_paint: Paint::default(),
      fill_paint: Paint::default(),
      clear_paint: Paint::default(),
      font: vec![],
      font_size: 10.0,
      text_align: HorizontalAlign::Left,
      layout: Layout::new(CoordinateSystem::PositiveYDown),
    };
    applier.stroke_paint.set_style(PaintStyle::Stroke);
    applier.fill_paint.set_style(PaintStyle::Fill);
    applier
      .clear_paint
      .set_style(PaintStyle::Fill)
      .set_color(Color::from_argb(0, 0, 0, 0))
      .set_stroke_miter(10.0)
      .set_blend_mode(BlendMode::Clear);

    for (i, op) in ops.iter().enumerate() {
      // Log application should not throw
      match applier.apply(op).map_err(|e| CommitError(i, e)) {
        Ok(()) => {}
        Err(e) => {
          write_applog(scope, format!("{}", e));
        }
      }
    }
  }

  Ok(())
}

struct CommitApplier<'p: 'q, 'q> {
  cvs: &'q mut OwnedCanvas<'p>,
  stroke_paint: Paint,
  fill_paint: Paint,
  clear_paint: Paint,
  font: Vec<&'static Font>,
  font_size: f32,
  text_align: HorizontalAlign,
  layout: Layout,
}

impl<'p, 'q> CommitApplier<'p, 'q> {
  fn build_path(&mut self, p: &[CanvasPathOp]) -> Path {
    use CanvasPathOp as V;
    let mut out = Path::new();
    for op in p {
      match *op {
        V::AddPath {
          ref subpath,
          ref matrix,
        } => {
          let matrix = if let Some(m) = matrix {
            Matrix::new_all(m.a, m.c, m.e, m.b, m.d, m.f, 0.0, 0.0, 1.0)
          } else {
            Matrix::new_identity()
          };
          let subpath = self.build_path(subpath);
          out.add_path_matrix(&subpath, &matrix, None);
        }
        V::LineTo { x, y } => {
          out.line_to(Point { x, y });
        }
        V::MoveTo { x, y } => {
          out.move_to(Point { x, y });
        }
        V::Arc {
          x,
          y,
          radius,
          start_angle,
          end_angle,
          counterclockwise,
        } => {
          let (start_angle, sweep_angle) = if counterclockwise {
            (end_angle, start_angle - end_angle)
          } else {
            (start_angle, end_angle - start_angle)
          };
          out.add_arc(
            Rect {
              top: y - radius,
              bottom: y + radius,
              left: x - radius,
              right: x + radius,
            },
            start_angle,
            sweep_angle,
          );
        }
        V::ArcTo {
          x1,
          y1,
          x2,
          y2,
          radius,
        } => {
          out.arc_to_tangent(Point { x: x1, y: y1 }, Point { x: x2, y: y2 }, radius);
        }
        V::BezierCurveTo {
          cp1x,
          cp1y,
          cp2x,
          cp2y,
          x,
          y,
        } => {
          out.cubic_to(
            Vector { x: cp1x, y: cp1y },
            Vector { x: cp2x, y: cp2y },
            Vector { x, y },
          );
        }
        V::QuadraticCurveTo { cpx, cpy, x, y } => {
          out.quad_to(Point { x: cpx, y: cpy }, Point { x, y });
        }
        V::Ellipse {
          x,
          y,
          radius_x,
          radius_y,
          rotation,
          start_angle,
          end_angle,
          counterclockwise,
        } => {
          // TODO: rotation
          let _ = rotation;
          let (start_angle, sweep_angle) = if counterclockwise {
            (end_angle, start_angle - end_angle)
          } else {
            (start_angle, end_angle - start_angle)
          };
          out.add_arc(
            Rect {
              top: y - radius_y,
              bottom: y + radius_y,
              left: x - radius_x,
              right: x + radius_x,
            },
            start_angle,
            sweep_angle,
          );
        }
        V::Rect {
          x,
          y,
          width,
          height,
        } => {
          out.add_rect(
            Rect {
              top: y,
              bottom: y + height,
              left: x,
              right: x + width,
            },
            None,
          );
        }
        V::Close => {
          out.close();
        }
      }
    }
    out
  }

  fn ensure_font(&mut self) -> Result<Vec<&'static Font>> {
    #[derive(Error, Debug)]
    #[error("font not found")]
    struct FontNotFound;

    if self.font.len() == 0 {
      let f = search_font(&font_util::Font::new("roboto, serif")?);
      if f.len() == 0 {
        return Err(FontNotFound.into());
      }
      self.font = f.clone();
      Ok(f)
    } else {
      Ok(self.font.clone())
    }
  }

  fn apply(&mut self, op: &CanvasOp) -> Result<()> {
    #[derive(Error, Debug)]
    #[error("svg parse error")]
    struct SvgParseError;

    use css_color_parser::Color as CssColor;
    use CanvasOp as V;

    match op {
      V::SetStrokeStyleColor { color } => {
        let css_color: CssColor = color.parse()?;
        let mut color = Color4f::from(Color::from_argb(1, css_color.r, css_color.g, css_color.b));
        color.a = css_color.a;
        self.stroke_paint.set_color4f(color, None);
      }
      V::SetStrokeLineWidth { width } => {
        self.stroke_paint.set_stroke_width(*width);
      }
      V::SetFillStyleColor { color } => {
        let css_color: CssColor = color.parse()?;
        let mut color = Color4f::from(Color::from_argb(1, css_color.r, css_color.g, css_color.b));
        color.a = css_color.a;
        self.fill_paint.set_color4f(color, None);
      }
      V::Stroke { path } => {
        let p = self.build_path(path);
        self.cvs.draw_path(&p, &self.stroke_paint);
      }
      V::Fill { path, fill_rule } => {
        let mut p = self.build_path(path);
        p.set_fill_type(match fill_rule {
          CanvasFillRule::NonZero => PathFillType::Winding,
          CanvasFillRule::EvenOdd => PathFillType::EvenOdd,
        });
        self.cvs.draw_path(&p, &self.fill_paint);
      }
      V::FillText { text, x, y } => {
        self.layout.reset(&LayoutSettings {
          horizontal_align: self.text_align,
          x: *x,
          y: *y,
          ..Default::default()
        });
        let fonts = self.ensure_font()?;

        {
          let mut sbuf = String::new();
          for ch in text.chars() {
            sbuf.clear();
            sbuf.push(ch);
            let font_index = fonts
              .iter()
              .copied()
              .enumerate()
              .find(|(_, x)| x.lookup_glyph_index(ch) != 0)
              .map(|(i, _)| i)
              .unwrap_or_else(|| 0);
            self
              .layout
              .append(&fonts, &TextStyle::new(&sbuf, self.font_size, font_index));
          }
        }

        for g in self.layout.glyphs() {
          let (_, pixels) = fonts[g.key.font_index].rasterize_config(g.key);
          if pixels.is_empty() {
            continue;
          }
          let pixels = Data::new_copy(&pixels);

          let image_info = ImageInfo::new(
            ISize {
              width: g.width as i32,
              height: g.height as i32,
            },
            ColorType::Alpha8,
            AlphaType::Opaque,
            None,
          );
          if let Some(image) = Image::from_raster_data(&image_info, pixels, g.width) {
            self
              .cvs
              .draw_image(&image, Point { x: g.x, y: g.y }, Some(&self.fill_paint));
          }
        }
      }
      V::ClearRect { rect } => {
        self.cvs.draw_rect(Rect::from(*rect), &self.clear_paint);
      }
      V::SetFont { font } => {
        let font_descriptor = font;
        let font = font_util::Font::new(font_descriptor)?;
        self.font_size = font.size;
        self.font = search_font(&font);
      }
    }
    Ok(())
  }
}
