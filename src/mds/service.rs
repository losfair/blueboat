use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  sync::Arc,
};

use crate::mds::config::ShardConfig;

use super::raw::{PrefixListOptions, RawMds, RawMdsHandle};
use anyhow::Result;
use ed25519_dalek::Keypair;
use parking_lot::Mutex as PMutex;
use rand::prelude::SliceRandom;

const MUX_WIDTH: u32 = 16;

#[derive(Clone)]
pub struct MdsServiceState {
  keypair: Arc<Keypair>,
  local_region: String,
  bootstrap: ServerState,
  shards: HashMap<String, Shard>,
  metadata_shard: Option<String>,
}

#[derive(Clone)]
struct Shard {
  name: String,
  servers: Vec<ServerState>,
}

#[derive(Clone)]
struct ServerState {
  url: String,
  region: String,
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
    let login_info = raw.authenticate(store, keypair).await?;
    let handle = raw.start();
    log::info!("opened mds server {} in region {}", url, login_info.region);
    Ok(ServerState {
      url: url.to_string(),
      region: login_info.region,
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
        region: "".to_string(),
        handle: RawMdsHandle::new_broken(),
      }
    })
  }
}

impl MdsServiceState {
  pub async fn bootstrap(url: &str, local_region: &str, keypair: Arc<Keypair>) -> Result<Self> {
    let bootstrap = ServerState::try_open(url, &keypair).await?;
    let mut me = Self {
      keypair: keypair,
      local_region: local_region.to_string(),
      bootstrap,
      shards: HashMap::new(),
      metadata_shard: None,
    };
    if !me.load_shards(&mut false).await {
      anyhow::bail!("failed to load shards");
    }
    Ok(me)
  }

  pub fn print_status(&self) {
    eprintln!("--- begin mds status ---");
    eprintln!(
      "public key: {}",
      hex::encode(&self.keypair.public.as_bytes())
    );
    eprintln!(
      "bootstrap server url {}, broken {}",
      self.bootstrap.url,
      self.bootstrap.handle.is_broken()
    );
    for (_, shard) in &self.shards {
      eprintln!("shard {}", shard.name);
      for server in &shard.servers {
        eprintln!(
          "\tserver url {}, broken {}",
          server.url,
          server.handle.is_broken()
        );
      }
    }
    eprintln!("--- end mds status ---");
  }

  pub fn get_shard_session(&self, name: &str) -> Option<&RawMdsHandle> {
    let shard = self.shards.get(name)?;
    let mut servers = shard.servers.iter().collect::<Vec<_>>();
    servers.shuffle(&mut rand::thread_rng());

    for &s in &servers {
      if !s.handle.is_broken() && s.region == self.local_region {
        return Some(&s.handle);
      }
    }
    for &s in &servers {
      if !s.handle.is_broken() {
        return Some(&s.handle);
      }
    }
    None
  }

  pub fn has_metadata_shard(&self) -> bool {
    self.metadata_shard.is_some()
  }

  pub fn get_metadata_shard_session(&self) -> Option<&RawMdsHandle> {
    self.get_shard_session(&self.metadata_shard.as_ref()?)
  }

  pub fn get_local_region(&self) -> &str {
    &self.local_region
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
    self.load_shards(changed).await;
    self.load_config(changed).await;
    self.scan_for_broken_servers(changed).await;
  }

  fn all_servers_mut(&mut self) -> impl Iterator<Item = &mut ServerState> {
    std::iter::once(&mut self.bootstrap).chain(
      self
        .shards
        .values_mut()
        .map(|r| r.servers.iter_mut())
        .flatten(),
    )
  }

  async fn load_config(&mut self, changed: &mut bool) {
    match self
      .bootstrap
      .handle
      .get_many(&["config/metadata-shard"], false)
      .await
    {
      Ok(x) => {
        let metadata_shard: Option<String> =
          x[0].as_ref().map(|x| String::from_utf8_lossy(x).into());
        if metadata_shard != self.metadata_shard {
          log::info!(
            "metadata shard changed from {:?} to {:?}",
            self.metadata_shard,
            metadata_shard
          );
          self.metadata_shard = metadata_shard;
          *changed = true;
        }
      }
      Err(e) => {
        log::error!("failed to load config/metadata-shard: {}", e);
      }
    }
  }

  async fn load_shards(&mut self, changed: &mut bool) -> bool {
    let all_shards: BTreeMap<String, Vec<u8>> = match self
      .bootstrap
      .handle
      .prefix_list(
        "shards",
        PrefixListOptions {
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
        log::error!("failed to list shards: {}", e);
        return false;
      }
    };
    let current_keys = self.shards.keys().cloned().collect::<Vec<_>>();
    for key in &current_keys {
      if !all_shards.contains_key(key.as_str()) {
        log::warn!("shard {} removed", key);
        self.shards.remove(key);
        *changed = true;
      }
    }

    for (shard_name, shard_config) in all_shards {
      let mut config = match serde_json::from_slice::<ShardConfig>(&shard_config) {
        Ok(config) => config,
        Err(err) => {
          log::error!("invalid shard config (shard {}): {}", shard_name, err);
          continue;
        }
      };

      let mut shard = Shard {
        name: shard_name.clone(),
        servers: Vec::new(),
      };

      if let Some(current_shard) = self.shards.get(&shard_name) {
        let current_urls: BTreeSet<&str> = current_shard
          .servers
          .iter()
          .map(|s| s.url.as_str())
          .collect();
        let current_servers: HashMap<&str, &ServerState> = current_shard
          .servers
          .iter()
          .map(|s| (s.url.as_str(), s))
          .collect();
        let new_urls: BTreeSet<&str> = config.servers.iter().map(|s| s.url.as_str()).collect();
        if current_urls == new_urls {
          continue;
        }

        let mut index = 0usize;
        while index < config.servers.len() {
          let server = &config.servers[index];
          if let Some(x) = current_servers.get(server.url.as_str()) {
            shard.servers.push((*x).clone());
            config.servers.swap_remove(index);
          } else {
            index += 1;
          }
        }
      }

      for s in &config.servers {
        log::info!("connecting to server {} in shard {}", s.url, shard_name);
        let handle = ServerState::open(&s.url, &self.keypair).await;
        shard.servers.push(handle);
      }
      log::info!("updated shard {}", shard_name);
      self.shards.insert(shard_name, shard);
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
