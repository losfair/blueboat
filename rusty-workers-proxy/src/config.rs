use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct Config {
    pub apps: Vec<AppConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            apps: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppConfig {
    pub id: AppId,
    pub routes: Vec<AppRoute>,
    pub bundle: String,

    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppRoute {
    pub domain: String,
    pub path_prefix: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[serde(transparent)]
pub struct AppId(pub String);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalConfig {
    pub runtime_cluster: Vec<SocketAddr>,
    pub max_ready_instances_per_app: usize,
    pub ready_instance_expiration_ms: u64,
    pub request_timeout_ms: u64,
    pub max_request_body_size_bytes: u64,
    pub dropout_rate: f32,
}
