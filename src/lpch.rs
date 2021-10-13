use serde::{Deserialize, Serialize};
use time::PrimitiveDateTime;

use crate::package::PackageKey;

#[derive(Serialize, Deserialize)]
pub enum LowPriorityMsg {
  Log(LogEntry),
  Background(BackgroundEntry),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LogEntry {
  pub app: PackageKey,
  pub request_id: String,
  pub message: String,
  pub logseq: i32,
  pub time: PrimitiveDateTime,
}

#[derive(Serialize, Deserialize)]
pub struct BackgroundEntry {
  pub app: PackageKey,
  pub request_id: String,
  pub wire_bytes: Vec<u8>,
}
