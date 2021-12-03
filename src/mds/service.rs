use std::collections::HashMap;

use super::raw::{RawMds, RawMdsHandle};
use anyhow::Result;
use ed25519_dalek::Keypair;

const MUX_WIDTH: u32 = 8;

#[derive(Clone)]
pub struct MdsServiceState {
  bootstrap: ServerState,
  regions: HashMap<String, Region>,
}

#[derive(Clone)]
struct Region {
  name: String,
  servers: Vec<ServerState>,
}

#[derive(Clone)]
struct ServerState {
  url: String,
  handle: RawMdsHandle,
}

impl ServerState {
  pub async fn open(url: &str, keypair: &Keypair) -> Result<Self> {
    let url = reqwest::Url::parse(url).map_err(|_| anyhow::anyhow!("invalid url"))?;
    let ws_url = format!("{}/mds", url.origin().ascii_serialization());
    let store = url.path().strip_prefix("/").unwrap_or("");
    if store.contains('/') {
      return Err(anyhow::anyhow!("invalid store name"));
    }
    let mut raw = RawMds::open(&ws_url, MUX_WIDTH).await?;
    raw.authenticate(store, keypair).await?;
    let handle = raw.start();
    Ok(ServerState {
      url: url.to_string(),
      handle,
    })
  }
}

impl MdsServiceState {
  pub async fn bootstrap(url: &str, keypair: &Keypair) -> Result<Self> {
    let bootstrap = ServerState::open(url, keypair).await?;
    Ok(Self {
      bootstrap,
      regions: HashMap::new(),
    })
  }
}
