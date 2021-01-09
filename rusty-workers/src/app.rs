use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppConfig {
    pub id: AppId,
    pub bundle_hash: String,

    #[serde(default)]
    pub env: BTreeMap<String, String>,

    #[serde(default)]
    pub kv_namespaces: Vec<KvNamespaceConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct KvNamespaceConfig {
    pub name: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppRoute {
    pub domain: String,
    pub path_prefix: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[serde(transparent)]
pub struct AppId(pub String);

pub fn decode_bundle_hash(raw: &str) -> Option<[u8; 32]> {
    base64::decode(raw)
        .ok()
        .filter(|x| x.len() == 32)
        .map(|x| {
            let mut slice = [0u8; 32];
            slice.copy_from_slice(&x);
            slice
        })
}

pub fn encode_bundle_hash(raw: &[u8; 32]) -> String {
    base64::encode(raw)
}
