use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Serialize;

#[derive(Serialize, JsonSchema)]
pub struct BlueboatBootstrapData {
  pub mysql: Vec<String>,
  pub apns: Vec<String>,
  pub env: HashMap<String, String>,
}

pub static JSLAND_SNAPSHOT: &'static [u8] = include_bytes!("../jsland.snapshot");
