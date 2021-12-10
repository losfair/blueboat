use std::time::Duration;

use anyhow::Result;
use lazy_static::lazy_static;
use rand::Rng;
use rdkafka::{producer::FutureProducer, ClientConfig};
use regex::Regex;

lazy_static! {
  static ref KAFKA_CONFIG_MATCHER: Regex = Regex::new("^([a-zA-Z0-9._-]+):([0-9]+)@(.+)$").unwrap();
}

pub struct KafkaProducerService {
  pub producer: FutureProducer,
  pub topic: String,
  pub partition: i32,
}

impl KafkaProducerService {
  pub fn open(s: &str) -> Result<Self> {
    let caps = match KAFKA_CONFIG_MATCHER.captures(s) {
      Some(x) => x,
      None => anyhow::bail!("invalid kafka config string"),
    };

    let topic = &caps[1];
    let partition: i32 = caps[2].parse()?;
    let servers = &caps[3];

    let mut client_config = ClientConfig::new();
    client_config.set("bootstrap.servers", servers);

    let producer = client_config.create()?;
    Ok(Self {
      producer,
      topic: topic.into(),
      partition,
    })
  }
}

pub async fn random_backoff() {
  let duration = Duration::from_millis(rand::thread_rng().gen_range(1000..5000));
  tokio::time::sleep(duration).await;
}

pub async fn random_backoff_fast() {
  let duration = Duration::from_millis(rand::thread_rng().gen_range(10..100));
  tokio::time::sleep(duration).await;
}
