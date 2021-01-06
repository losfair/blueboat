use serde::{Serialize, Deserialize};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub runtime_cluster: Vec<SocketAddr>,

    pub apps: Vec<AppConfig>,

    #[serde(default = "default_instance_expiration_time_ms")]
    pub instance_expiration_time_ms: u64,

    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,

    #[serde(default = "default_max_request_body_size_bytes")]
    pub max_request_body_size_bytes: u64,
}

fn default_instance_expiration_time_ms() -> u64 { 540000 } // 9 minutes
fn default_request_timeout_ms() -> u64 { 30000 } // 30 seconds
fn default_max_request_body_size_bytes() -> u64 { 2 * 1024 * 1024 } // 2M

impl Default for Config {
    fn default() -> Self {
        Config {
            runtime_cluster: Default::default(),
            apps: Default::default(),
            instance_expiration_time_ms: default_instance_expiration_time_ms(),
            request_timeout_ms: default_request_timeout_ms(),
            max_request_body_size_bytes: default_max_request_body_size_bytes(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppConfig {
    pub id: AppId,
    pub routes: Vec<AppRoute>,
    pub script: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct AppRoute {
    pub domain: String,
    pub path_prefix: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[serde(transparent)]
pub struct AppId(pub String);
