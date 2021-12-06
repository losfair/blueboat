use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShardConfig {
  pub servers: Vec<ServerConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
  pub url: String,
}
