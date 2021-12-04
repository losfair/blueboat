use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  sync::Arc,
};

use crate::mds::config::RegionConfig;

use super::raw::{PrefixListOptions, RawMds, RawMdsHandle};
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

  async fn try_open(url: &str, keypair: &Keypair) -> Result<Self> {
    tokio::select! {
      _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => Err(anyhow::anyhow!("timeout")),
      res = Self::open_raw(url, keypair) => res
    }
  }

  async fn open(url: &str, keypair: &Keypair) -> Self {
    Self::try_open(url, keypair).await.unwrap_or_else(|e| {
      log::error!(
        "failed to connect to mds server, returning empty handle: {}",
        e
      );
      Self {
        url: url.to_string(),
        handle: RawMdsHandle::new_broken(),
      }
    })
  }
}

impl MdsServiceState {
  pub async fn bootstrap(url: &str, keypair: Arc<Keypair>) -> Result<Self> {
    let bootstrap = ServerState::try_open(url, &keypair).await?;
    let mut me = Self {
      keypair: keypair,
      bootstrap,
      regions: HashMap::new(),
    };
    if !me.load_regions(&mut false).await {
      anyhow::bail!("failed to load regions");
    }
    Ok(me)
  }

  pub fn start_refresh_task(me: Arc<PMutex<Arc<Self>>>) {
    tokio::spawn(async move {
      loop {
        let mut snapshot = (**me.lock()).clone();
        let mut changed = false;
        snapshot.refresh_once(&mut changed).await;

        if changed {
          log::info!("mds service state changed");
          *me.lock() = Arc::new(snapshot);
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
      }
    });
  }

  async fn refresh_once(&mut self, changed: &mut bool) {
    self.load_regions(changed).await;
    self.scan_for_broken_servers(changed).await;
  }

  fn all_servers_mut(&mut self) -> impl Iterator<Item = &mut ServerState> {
    std::iter::once(&mut self.bootstrap).chain(
      self
        .regions
        .values_mut()
        .map(|r| r.servers.iter_mut())
        .flatten(),
    )
  }

  async fn load_regions(&mut self, changed: &mut bool) -> bool {
    let all_regions: BTreeMap<String, Vec<u8>> = match self
      .bootstrap
      .handle
      .prefix_list(
        "regions",
        &PrefixListOptions {
          reverse: false,
          want_value: true,
          limit: 500,
          cursor: None,
        },
        false,
      )
      .await
    {
      Ok(x) => x.into_iter().collect(),
      Err(e) => {
        log::error!("failed to list regions: {}", e);
        return false;
      }
    };
    let current_keys = self.regions.keys().cloned().collect::<Vec<_>>();
    for key in &current_keys {
      if !all_regions.contains_key(key.as_str()) {
        log::warn!("region {} removed", key);
        self.regions.remove(key);
        *changed = true;
      }
    }

    for (region_name, region_config) in all_regions {
      let mut config = match serde_json::from_slice::<RegionConfig>(&region_config) {
        Ok(config) => config,
        Err(err) => {
          log::error!("invalid region config (region {}): {}", region_name, err);
          continue;
        }
      };

      let mut region = Region {
        name: region_name.clone(),
        servers: Vec::new(),
      };

      if let Some(current_region) = self.regions.get(&region_name) {
        let current_urls: BTreeSet<&str> = current_region
          .servers
          .iter()
          .map(|s| s.url.as_str())
          .collect();
        let current_servers: HashMap<&str, &RawMdsHandle> = current_region
          .servers
          .iter()
          .map(|s| (s.url.as_str(), &s.handle))
          .collect();
        let new_urls: BTreeSet<&str> = config.servers.iter().map(|s| s.url.as_str()).collect();
        if current_urls == new_urls {
          continue;
        }

        let mut index = 0usize;
        while index < config.servers.len() {
          let server = &config.servers[index];
          if let Some(x) = current_servers.get(server.url.as_str()) {
            region.servers.push(ServerState {
              url: server.url.clone(),
              handle: (*x).clone(),
            });
            config.servers.swap_remove(index);
          } else {
            index += 1;
          }
        }
      }

      for s in &config.servers {
        log::info!("connecting to server {} in region {}", s.url, region_name);
        let handle = ServerState::open(&s.url, &self.keypair).await;
        region.servers.push(handle);
      }
      log::info!("updated region {}", region_name);
      self.regions.insert(region_name, region);
      *changed = true;
    }
    true
  }

  async fn scan_for_broken_servers<'a>(&'a mut self, changed: &mut bool) {
    let keypair = self.keypair.clone();
    let it = self.all_servers_mut();
    for server in it {
      if server.handle.is_broken() {
        log::info!("reconnecting to broken server {}", server.url);
        *server = match ServerState::try_open(&server.url, &keypair).await {
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
