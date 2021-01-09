use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppConfig {
    pub id: AppId,
    pub routes: Vec<AppRoute>,
    pub bundle: String,

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
