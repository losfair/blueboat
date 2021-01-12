use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppConfig {
    pub id: AppId,

    #[serde(default)]
    pub bundle_id: String,

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

pub fn decode_id128(raw: &str) -> Option<[u8; 16]> {
    base64::decode(raw).ok().filter(|x| x.len() == 16).map(|x| {
        let mut slice = [0u8; 16];
        slice.copy_from_slice(&x);
        slice
    })
}

pub fn encode_id128(raw: &[u8; 16]) -> String {
    base64::encode(raw)
}
