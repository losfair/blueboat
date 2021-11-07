use anyhow::Result;
use lazy_static::lazy_static;
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
