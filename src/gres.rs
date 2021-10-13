use std::collections::BTreeMap;

use byteorder::{BigEndian, ReadBytesExt};
use fontdue::{Font, FontSettings};
use once_cell::sync::OnceCell;
use ttf_parser::Face;

pub static FONTS: OnceCell<BTreeMap<String, Font>> = OnceCell::new();

pub const FONT_DIR_ENV_NAME: &str = "SMRAPP_BLUEBOAT_FONT_DIR";

pub fn load_global_resources_single_threaded() {
  let mut fonts: BTreeMap<String, Font> = BTreeMap::new();
  let mut total_size = 0usize;
  if let Ok(x) = std::env::var(FONT_DIR_ENV_NAME) {
    if let Ok(dir) = std::fs::read_dir(&x) {
      for entry in dir {
        if let Ok(entry) = entry {
          if let Ok(file_type) = entry.file_type() {
            if file_type.is_file() || file_type.is_symlink() {
              if let Ok(font_bytes) = std::fs::read(entry.path()) {
                if let Ok(face) = Face::from_slice(&font_bytes, 0) {
                  match get_face_name(&face) {
                    Some(name) => {
                      if let Ok(font) =
                        Font::from_bytes(font_bytes.as_slice(), FontSettings::default())
                      {
                        let name = name.to_lowercase();
                        log::info!("Loaded font: {} ({} bytes)", name, font_bytes.len());
                        total_size += font_bytes.len();
                        fonts.insert(name, font);
                      } else {
                        log::warn!(
                          "Cannot load font from file `{}`.",
                          entry.file_name().to_string_lossy()
                        );
                      }
                    }
                    None => {
                      log::warn!(
                        "Cannot decode font name from file `{}`.",
                        entry.file_name().to_string_lossy()
                      );
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
  log::info!(
    "Loaded {} fonts. Total size: {} bytes",
    fonts.len(),
    total_size
  );
  let _ = FONTS.set(fonts);
}

fn get_face_name(face: &Face) -> Option<String> {
  for name in face.names() {
    if name.name_id() == 4 && name.is_unicode() {
      let mut name = name.name();
      if name.len() % 2 == 0 {
        let mut buf: Vec<u16> = Vec::with_capacity(name.len() / 2);
        for _ in 0..buf.capacity() {
          buf.push(name.read_u16::<BigEndian>().unwrap());
        }
        if let Ok(x) = String::from_utf16(&buf) {
          return Some(x);
        }
      }
    }
  }
  None
}
