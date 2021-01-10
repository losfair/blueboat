use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalConfig {
    pub runtime_cluster: Vec<SocketAddr>,
    pub max_ready_instances_per_app: usize,
    pub ready_instance_expiration_ms: u64,
    pub request_timeout_ms: u64,
    pub max_request_body_size_bytes: u64,
    pub dropout_rate: f32,
    pub route_cache_size: usize,
    pub app_cache_size: usize,
}
