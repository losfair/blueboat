use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::OnceCell;

use self::service::MdsServiceState;
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
