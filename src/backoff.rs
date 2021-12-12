use std::time::Duration;

use rand::Rng;

pub async fn random_backoff() {
  let duration = Duration::from_millis(rand::thread_rng().gen_range(1000..5000));
  tokio::time::sleep(duration).await;
}

pub async fn random_backoff_fast() {
  let duration = Duration::from_millis(rand::thread_rng().gen_range(10..100));
  tokio::time::sleep(duration).await;
}
