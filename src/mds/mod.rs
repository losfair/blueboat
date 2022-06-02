use std::sync::Arc;

use anyhow::Result;
use indexmap::IndexMap;
use tokio::sync::OnceCell;

use self::config_v2::{MdsClusterConfig, MdsConfig};
use foundationdb::Database;

pub mod config_v2;

pub static MDS: OnceCell<Option<MdsService>> = OnceCell::const_new();

#[derive(Clone)]
pub struct MdsService {
  pub fdb: Arc<IndexMap<String, MdsCluster>>,
}

pub struct MdsCluster {
  pub db: Database,
  pub config: MdsClusterConfig,
}

impl MdsCluster {
  pub fn pack_user_key(&self, ns_prefix: &str, path: &str) -> Vec<u8> {
    let path = std::iter::once(self.config.prefix.as_str())
      .chain(ns_prefix.split('/').filter(|x| !x.is_empty()))
      .chain(path.split('/').filter(|x| !x.is_empty()))
      .collect::<Vec<_>>();
    foundationdb::tuple::pack(&path)
  }
  pub fn unpack_user_key(&self, ns_prefix: &str, key: &[u8]) -> Option<String> {
    let prefix_to_strip = foundationdb::tuple::pack(
      &std::iter::once(self.config.prefix.as_str())
        .chain(ns_prefix.split('/').filter(|x| !x.is_empty()))
        .collect::<Vec<_>>(),
    );
    let key = key.strip_prefix(prefix_to_strip.as_slice())?;
    let segs: Vec<String> = foundationdb::tuple::unpack(key).ok()?;
    Some(segs.join("/"))
  }
}

impl MdsService {
  pub fn open(config: &MdsConfig) -> Result<Self> {
    let mut fdb: IndexMap<String, MdsCluster> = IndexMap::new();
    for (k, cluster_config) in &config.clusters {
      let inst = Database::new(Some(cluster_config.path.as_str()))
        .map_err(|e| anyhow::Error::from(e).context(format!("failed to open cluster '{}'", k)))?;
      fdb.insert(
        k.clone(),
        MdsCluster {
          db: inst,
          config: cluster_config.clone(),
        },
      );
    }
    Ok(Self { fdb: Arc::new(fdb) })
  }
}

pub fn get_mds() -> Result<MdsService> {
  MDS
    .get()
    .expect("mds not initialized")
    .clone()
    .ok_or_else(|| anyhow::anyhow!("mds not available"))
}
