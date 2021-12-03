use std::{collections::HashMap, sync::Arc};

use super::raw::{RawMds, RawMdsHandle};
use anyhow::Result;
use ed25519_dalek::Keypair;
use parking_lot::Mutex as PMutex;

const MUX_WIDTH: u32 = 8;

#[derive(Clone)]
pub struct MdsServiceState {
  keypair: Arc<Keypair>,
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
  async fn open_raw(url: &str, keypair: &Keypair) -> Result<Self> {
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

  async fn open(url: &str, keypair: &Keypair) -> Result<Self> {
    tokio::select! {
      _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => Err(anyhow::anyhow!("timeout")),
      res = Self::open_raw(url, keypair) => res
    }
  }
}

impl MdsServiceState {
  pub async fn bootstrap(url: &str, keypair: Arc<Keypair>) -> Result<Self> {
    let bootstrap = ServerState::open(url, &keypair).await?;
    Ok(Self {
      keypair: keypair,
      bootstrap,
      regions: HashMap::new(),
    })
  }

  pub fn start_refresh_task(me: Arc<PMutex<Arc<Self>>>) {
    tokio::spawn(async move {
      loop {
        let mut snapshot = (**me.lock()).clone();
        let mut changed = false;
        snapshot.scan_for_broken_servers(&mut changed).await;

        if changed {
          log::info!("mds service state changed");
          *me.lock() = Arc::new(snapshot);
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
      }
    });
  }

  async fn scan_for_broken_servers(&mut self, changed: &mut bool) {
    for (_, region) in self.regions.iter_mut() {
      for server in region.servers.iter_mut() {
        if server.handle.is_broken() {
          log::info!("reconnecting to broken server {}", server.url);
          *server = match ServerState::open(&server.url, &self.keypair).await {
            Ok(x) => x,
            Err(e) => {
              log::error!("reconnect failed: {}", e);
              continue;
            }
          };
          *changed = true;
        }
      }
    }
  }
}
