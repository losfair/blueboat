use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::OnceCell;

use crate::util::random_backoff;

use self::{raw::RawMdsHandle, service::MdsServiceState};
use anyhow::Result;

pub mod config;
pub mod keycodec;
pub mod raw;
pub mod service;

pub static MDS: OnceCell<Option<Arc<Mutex<Arc<MdsServiceState>>>>> = OnceCell::const_new();

pub fn get_mds() -> Result<Arc<MdsServiceState>> {
  MDS
    .get()
    .expect("mds not initialized")
    .as_ref()
    .map(|x| (*x.lock()).clone())
    .ok_or_else(|| anyhow::anyhow!("mds not available"))
}

pub async fn get_shard_session_with_infinite_retry_assuming_mds_exists(name: &str) -> RawMdsHandle {
  loop {
    let mds = get_mds().unwrap();
    let handle = mds.get_shard_session(name);
    match handle {
      Some(x) => return x.clone(),
      None => {
        random_backoff().await;
        continue;
      }
    }
  }
}
