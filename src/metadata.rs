use std::collections::HashMap;

use base64_serde::base64_serde_type;
use serde::{Deserialize, Serialize};

base64_serde_type!(Base64Standard, base64::STANDARD);

#[derive(Serialize, Deserialize, Clone)]
pub struct Metadata {
  #[serde(skip)]
  pub path: String,

  pub version: String,
  pub package: String,
  pub env: HashMap<String, String>,

  #[serde(default)]
  pub mysql: HashMap<String, MysqlMetadata>,

  #[serde(default)]
  pub apns: HashMap<String, ApnsMetadata>,

  #[serde(default)]
  pub kv_namespaces: HashMap<String, KvNamespaceMetadata>,

  #[serde(default)]
  pub pubsub: HashMap<String, PubsubMetadata>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PubsubMetadata {
  /// Must be a hex-encoded [u8; 16].
  pub namespace: String,

  #[serde(skip)]
  pub namespace_bytes: [u8; 16],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KvNamespaceMetadata {
  pub shard: String,
  pub prefix: String,

  #[serde(default)]
  pub raw: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MysqlMetadata {
  pub url: String,
  pub root_certificate: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApnsMetadata {
  pub endpoint: ApnsEndpointMetadata,

  #[serde(with = "Base64Standard")]
  pub cert: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ApnsEndpointMetadata {
  #[serde(rename = "production")]
  Production,

  #[serde(rename = "sandbox")]
  Sandbox,
}
