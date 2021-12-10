use std::{
  collections::{BTreeMap, HashMap},
  sync::{Arc, Weak},
  time::SystemTime,
};

use crate::{
  mds::{
    get_mds,
    raw::{PrefixListOptions, TriStateCheck, TriStateSet},
  },
  util::random_backoff,
};
use anyhow::Result;
use either::Either;
use futures::StreamExt;
use itertools::Itertools;
use parking_lot::Mutex;
use rand::prelude::SliceRandom;
use rdkafka::producer::FutureProducer;
use rdkafka::{
  consumer::{Consumer, StreamConsumer},
  message::BorrowedMessage,
  ClientConfig, Message,
};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;

pub struct TaskCompletion {
  msg: TaskCompletionMessage,
  tx: tokio::sync::mpsc::UnboundedSender<TaskCompletionMessage>,
}

impl TaskCompletion {
  pub fn notify(self) {
    let _ = self.tx.send(self.msg);
  }
}

struct TaskCompletionMessage {
  partition: i32,
  offset: i64,
}

#[derive(Serialize, Deserialize)]
pub struct KafkaChannelConfig {
  pub servers: String,
  pub topic: String,
  pub consumer_group: String,
}

#[derive(Serialize, Deserialize)]
struct KafkaChannelLock {
  identity: String,
  ts: u64,
}

#[derive(Clone)]
pub struct ChannelRefresher {
  channels: Arc<Mutex<Vec<(String, Arc<ChannelSender>)>>>,
}

pub struct ChannelSender {
  pub config: KafkaChannelConfig,
  pub producer: FutureProducer,
}

pub static CHANNEL_REFRESHER: OnceCell<ChannelRefresher> = OnceCell::const_new();

pub fn get_channel_refresher() -> &'static ChannelRefresher {
  CHANNEL_REFRESHER
    .get()
    .expect("channel refresher not initialized")
}

impl ChannelRefresher {
  pub async fn init() -> Self {
    let channels = loop {
      match Self::fetch_channels().await {
        Ok(channels) => break channels,
        Err(e) => {
          log::error!("Failed to fetch channels: {}", e);
          random_backoff().await;
        }
      }
    };
    let channels = Arc::new(Mutex::new(channels));
    tokio::spawn(Self::refresh_forever(Arc::downgrade(&channels)));
    Self { channels }
  }

  pub fn get_random(&self) -> Result<Arc<ChannelSender>> {
    let channels = self.channels.lock();
    channels
      .choose(&mut rand::thread_rng())
      .map(|x| x.1.clone())
      .ok_or_else(|| anyhow::anyhow!("No channels available"))
  }

  async fn refresh_forever(me: Weak<Mutex<Vec<(String, Arc<ChannelSender>)>>>) {
    loop {
      random_backoff().await;
      let me = match me.upgrade() {
        Some(me) => me,
        None => {
          return;
        }
      };
      let channels = match Self::fetch_channels().await {
        Ok(channels) => channels,
        Err(e) => {
          log::error!("Failed to fetch channels: {}", e);
          continue;
        }
      };
      *me.lock() = channels;
    }
  }

  async fn fetch_channels() -> Result<Vec<(String, Arc<ChannelSender>)>> {
    let rows = load_channel_rows().await?;
    let channels = normalize_channel_rows(&rows);
    let mut out: Vec<(String, Arc<ChannelSender>)> = vec![];
    for (k, v) in channels {
      if let Some(c) = v.get("config") {
        if let Ok(c) = serde_json::from_slice::<KafkaChannelConfig>(*c) {
          let mut client_config = ClientConfig::new();
          client_config.set("bootstrap.servers", &c.servers);
          let producer: FutureProducer = match client_config.create() {
            Ok(producer) => producer,
            Err(e) => {
              log::error!("Failed to create producer (channel {}): {}", k, e);
              continue;
            }
          };

          out.push((
            k.to_string(),
            Arc::new(ChannelSender {
              config: c,
              producer,
            }),
          ));
        }
      }
    }
    Ok(out)
  }
}

pub fn spawn_preemptive_task_acceptor<T: for<'x> Deserialize<'x> + Send + 'static>(
) -> Option<tokio::sync::mpsc::Receiver<(T, TaskCompletion)>> {
  get_mds().ok()?;

  let (tx, rx): (
    tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
    tokio::sync::mpsc::Receiver<(T, TaskCompletion)>,
  ) = tokio::sync::mpsc::channel(8);
  tokio::spawn(preemptive_task_acceptor_worker(tx));

  Some(rx)
}

async fn load_channel_rows() -> Result<Vec<(String, Vec<u8>)>> {
  let mds = match get_mds() {
    Ok(mds) => mds,
    Err(_) => return Ok(vec![]),
  };
  let sess = match mds.get_metadata_shard_session() {
    Some(sess) => sess,
    None => return Ok(vec![]),
  };
  let region_scheduler_prefix = format!("task-scheduler/{}", get_mds().unwrap().get_local_region());
  sess
    .prefix_list(
      &region_scheduler_prefix,
      &PrefixListOptions {
        limit: 1000,
        want_value: true,
        reverse: false,
        cursor: None,
      },
      true,
    )
    .await
    .map_err(|e| e.context("cannot list task scheduler channels"))
}

fn normalize_channel_rows<'a>(rows: &[(String, Vec<u8>)]) -> Vec<(&str, HashMap<&str, &[u8]>)> {
  rows
    .iter()
    .filter(|(k, _)| k.chars().filter(|&c| c == '/').count() == 1)
    .map(|(k, v)| {
      let mut parts = k.split('/');
      let key = parts.next().unwrap();
      let prop = parts.next().unwrap();
      (key, prop, v.as_slice())
    })
    .group_by(|(k, _, _)| *k)
    .into_iter()
    .map(|(k, group)| {
      let mut props: HashMap<&str, &[u8]> = HashMap::new();
      for k in group {
        props.insert(k.1, k.2);
      }
      (k, props)
    })
    .collect()
}

async fn preemptive_task_acceptor_worker<T: for<'x> Deserialize<'x> + Send + 'static>(
  mut tx: tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
) {
  let identity = Uuid::new_v4().to_string();
  let mut first = true;
  let region_scheduler_prefix = format!("task-scheduler/{}", get_mds().unwrap().get_local_region());
  log::info!("preemptive_task_acceptor_worker starting");
  loop {
    if !first {
      random_backoff().await;
    }
    first = false;

    let mds = get_mds().unwrap();

    let sess = match mds.get_metadata_shard_session() {
      Some(x) => x,
      None => continue,
    };
    log::debug!("attempting to acquire lock with identity {}", identity);
    let rows = match sess
      .prefix_list(
        &region_scheduler_prefix,
        &PrefixListOptions {
          limit: 1000,
          want_value: true,
          reverse: false,
          cursor: None,
        },
        true,
      )
      .await
    {
      Ok(x) => x,
      Err(_) => continue,
    };
    let now = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_millis() as u64;
    let mut channels = normalize_channel_rows(&rows);

    channels.shuffle(&mut rand::thread_rng());

    for (channel, props) in &channels {
      let original_lock_raw = props.get("lock").copied();
      let lock: Option<KafkaChannelLock> =
        original_lock_raw.and_then(|x| serde_json::from_slice(x).ok());
      match &lock {
        Some(x) if now.saturating_sub(x.ts) < 25 * 1000 => continue,
        _ => {}
      }

      let lock = KafkaChannelLock {
        identity: identity.clone(),
        ts: now,
      };
      let mut lock_json = serde_json::to_vec(&lock).unwrap();
      let lock_key = format!("{}/{}/lock", region_scheduler_prefix, channel);
      match sess
        .compare_and_set_many([(
          &lock_key,
          if let Some(x) = original_lock_raw {
            TriStateCheck::Value(x)
          } else {
            TriStateCheck::Absent
          },
          TriStateSet::Value(&lock_json),
        )])
        .await
      {
        Ok(x) => {
          if x {
            log::info!("acquired lock for channel {}", channel);
            match channel_worker(*channel, props, &mut lock_json, &mut tx).await {
              Ok(()) => {}
              Err(e) => {
                log::error!("error while running task for channel {}: {}", channel, e);
              }
            }
            match sess
              .compare_and_set_many([(
                &lock_key,
                TriStateCheck::Value(&lock_json),
                TriStateSet::<&[u8]>::Delete,
              )])
              .await
            {
              Ok(true) => {
                log::info!("released lock for channel {}", channel);
              }
              e => {
                log::error!("failed to release lock for channel {}: {:?}", channel, e);
              }
            }
            break;
          } else {
            log::warn!("failed to acquire lock for channel {}", channel);
          }
        }
        Err(e) => {
          log::warn!("failed to acquire lock for channel {}: {}", *channel, e);
        }
      };
    }
  }
}

async fn channel_worker<T: for<'x> Deserialize<'x> + Send + 'static>(
  channel: &str,
  props: &HashMap<&str, &[u8]>,
  lock_json: &mut Vec<u8>,
  tx: &mut tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
) -> Result<()> {
  let region_scheduler_prefix = format!("task-scheduler/{}", get_mds().unwrap().get_local_region());
  let config_json = *props
    .get("config")
    .ok_or_else(|| anyhow::anyhow!("missing config"))?;
  let config: KafkaChannelConfig = serde_json::from_slice(config_json)?;

  let mut client_config = ClientConfig::new();
  client_config.set("bootstrap.servers", &config.servers);
  client_config.set("group.id", &config.consumer_group);
  client_config.set("auto.offset.reset", "earliest");
  client_config.set("enable.auto.offset.store", "false");
  let consumer: StreamConsumer = client_config.create()?;
  consumer.subscribe(&[config.topic.as_str()])?;

  struct Guard(tokio::task::JoinHandle<()>);
  impl Drop for Guard {
    fn drop(&mut self) {
      self.0.abort();
    }
  }
  let _g = Guard(tokio::spawn(real_channel_worker(
    consumer,
    config.topic.clone(),
    tx.clone(),
  )));

  loop {
    random_backoff().await;

    let mds = get_mds().unwrap();
    let sess = mds
      .get_metadata_shard_session()
      .ok_or_else(|| anyhow::anyhow!("no metadata shard session"))?;

    let new_config = sess
      .get_many(
        &[format!("{}/{}/config", region_scheduler_prefix, channel)],
        true,
      )
      .await?
      .into_iter()
      .next()
      .unwrap();
    if new_config.as_ref().map(|x| x.as_slice()) != Some(config_json) {
      log::warn!("config changed for channel {}", channel);
      return Ok(());
    }

    // Renew
    let mut lock: KafkaChannelLock = serde_json::from_slice(lock_json.as_slice())?;
    lock.ts = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_millis() as u64;
    let new_lock_json = serde_json::to_vec(&lock).unwrap();
    if !sess
      .compare_and_set_many([(
        &format!("{}/{}/lock", region_scheduler_prefix, channel),
        TriStateCheck::Value(lock_json.as_slice()),
        TriStateSet::Value(&new_lock_json),
      )])
      .await?
    {
      log::warn!("failed to renew lock for channel {}", channel);
      return Ok(());
    }
    *lock_json = new_lock_json;
  }
}

async fn real_channel_worker<T: for<'x> Deserialize<'x> + Send + 'static>(
  consumer: StreamConsumer,
  topic: String,
  tx: tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
) {
  const THRESHOLD: usize = 100;
  let mut stream = consumer.stream();
  let mut backlog: HashMap<i32, BTreeMap<i64, bool>> = HashMap::new(); // partition -> (offset, completed)
  let (completion_tx, mut completion_rx): (
    tokio::sync::mpsc::UnboundedSender<TaskCompletionMessage>,
    tokio::sync::mpsc::UnboundedReceiver<TaskCompletionMessage>,
  ) = tokio::sync::mpsc::unbounded_channel();
  loop {
    let v: Either<BorrowedMessage, TaskCompletionMessage> = tokio::select! {
      msg = stream.next() => {
        let msg = match msg {
          Some(x) => x,
          None => break,
        };
        let msg = match msg {
          Ok(msg) => msg,
          Err(e) => {
            log::error!("real_channel_worker: error while consuming: {}", e);
            continue;
          }
        };
        Either::Left(msg)
      }
      msg = completion_rx.recv() => {
        Either::Right(msg.expect("completion msg cannot be None"))
      }
    };
    match v {
      Either::Left(msg) => {
        let payload = match msg.payload() {
          Some(x) => x,
          None => continue,
        };
        let partition = msg.partition();
        let offset = msg.offset();

        let partition_backlog = backlog.entry(partition).or_insert_with(BTreeMap::new);
        partition_backlog.insert(offset, false);
        if partition_backlog.len() > THRESHOLD {
          log::warn!(
            "real_channel_worker: backlog is too large ({}), partition {}",
            partition_backlog.len(),
            partition
          );
        }

        let value: T = match rmp_serde::from_read(payload) {
          Ok(x) => x,
          Err(e) => {
            log::error!("real_channel_worker: error while deserializing: {}", e);
            continue;
          }
        };
        let completion = TaskCompletion {
          tx: completion_tx.clone(),
          msg: TaskCompletionMessage { partition, offset },
        };
        let _ = tx.send((value, completion)).await;
      }
      Either::Right(msg) => {
        let partition_backlog = backlog
          .get_mut(&msg.partition)
          .expect("partition must exist");
        *partition_backlog
          .get_mut(&msg.offset)
          .expect("offset must exist") = true;
        let mut first_unused_offset: i64 = 0;
        for (offset, completed) in partition_backlog.iter_mut() {
          if !*completed {
            break;
          }
          let res = consumer.store_offset(&topic, msg.partition, *offset);
          if let Err(e) = res {
            log::error!("real_channel_worker: failed to store offset: {}", e);
            break;
          }
          first_unused_offset = *offset + 1;
        }
        log::debug!("first unused offset: {}", first_unused_offset);
        if first_unused_offset != 0 {
          *partition_backlog = partition_backlog.split_off(&first_unused_offset);
        }
      }
    }
  }

  log::warn!("consumer stream ended");
}
