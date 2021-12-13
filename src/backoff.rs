use std::time::Duration;

use rand::Rng;

use crate::consts::{BACKOFF_FAST_RANGE_MS, BACKOFF_RANGE_MS};

pub async fn random_backoff() {
  let duration = Duration::from_millis(rand::thread_rng().gen_range(BACKOFF_RANGE_MS));
  tokio::time::sleep(duration).await;
}

pub async fn random_backoff_fast() {
  let duration = Duration::from_millis(rand::thread_rng().gen_range(BACKOFF_FAST_RANGE_MS));
  tokio::time::sleep(duration).await;
}
