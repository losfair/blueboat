use std::collections::HashSet;

use fontdue::Font;
use itertools::Itertools;

use crate::{api::graphics::font_util::FontStyle, gres::FONTS};

pub fn search_font(spec: &super::font_util::Font) -> Vec<&'static Font> {
  let mut ret = vec![];
  for candidate in spec.family.split(",").map(|x| x.trim()) {
    if let Some(m) = match_font_family(candidate, spec) {
      ret.push(m);
    }
  }
  ret
}

fn match_font_family(candidate: &str, spec: &super::font_util::Font) -> Option<&'static Font> {
  let fonts = FONTS.get().unwrap();
  let candidate = candidate.to_lowercase().replace("-", " ");
  let candidate = candidate.split(" ").collect_vec();
  let mut out: Option<(u32, &'static Font)> = None;
  for (k, v) in fonts {
    let k_segs = k.split(" ").collect::<HashSet<&str>>();
    let mut score = 0u32;
    for seg in &candidate {
      if k_segs.contains(seg) {
        score += 1;
      }
    }

    // If the name does not match, let's stop here
    if score == 0 {
      continue;
    }

    if matches!(spec.style, FontStyle::Italic) && k_segs.contains("italic") {
      score += 1;
    }

    if (0..300).contains(&spec.weight) {
      if k_segs.contains("extralight") {
        score += 2;
      } else if k_segs.contains("light") {
        score += 1;
      }
    } else if (300..400).contains(&spec.weight) {
      if k_segs.contains("extralight") {
        score += 1;
      } else if k_segs.contains("light") {
        score += 2;
      }
    } else if (400..500).contains(&spec.weight) {
    } else if (500..600).contains(&spec.weight) {
      if k_segs.contains("medium") {
        score += 1;
      }
    } else if (600..700).contains(&spec.weight) {
      if k_segs.contains("semibold") {
        score += 2;
      } else if k_segs.contains("bold") {
        score += 1;
      }
    } else if (700..900).contains(&spec.weight) {
      if k_segs.contains("bold") {
        score += 2;
      } else if k_segs.contains("bold") {
        score += 1;
      }
    } else {
      if k_segs.contains("black") {
        score += 1;
      }
    }
    if out.is_none() || out.as_ref().unwrap().0 < score {
      out = Some((score, v));
    }
  }
  out.map(|x| x.1)
}
