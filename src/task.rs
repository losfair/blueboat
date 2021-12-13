use std::{
  collections::{BTreeMap, HashMap},
  sync::{Arc, Weak},
  time::{Instant, SystemTime},
};

use crate::{
  backoff::random_backoff,
  consts::{TASK_LOCK_RENEWAL_TIMEOUT, TASK_LOCK_TTL},
  kvutil::group_kv_rows_by_prefix,
  lpch::BackgroundEntry,
  mds::{
    get_mds, get_shard_session_with_infinite_retry_assuming_mds_exists,
    raw::{PrefixListOptions, TriStateCheck, TriStateSet},
  },
};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use either::Either;
use futures::StreamExt;
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
  delayed_task_prefix: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ChannelConfig {
  pub servers: String,
  pub topic: String,
  pub consumer_group: String,
  pub delayed_task_shard: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ChannelLock {
  identity: String,
  ts: u64,
}

#[derive(Clone)]
pub struct ChannelRefresher {
  channels: Arc<Mutex<Vec<(String, Arc<ChannelSender>)>>>,
}

pub struct ChannelSender {
  pub config: ChannelConfig,
  pub kafka_producer: FutureProducer,
}

impl ChannelSender {
  pub async fn add_delayed_task(&self, ts_secs: i64, entry: BackgroundEntry) -> Result<String> {
    let delayed_task_shard = match &self.config.delayed_task_shard {
      Some(shard) => shard,
      None => anyhow::bail!("delayed task shard is not set"),
    };
    let mds = get_mds()?;
    let shard = mds
      .get_shard_session(delayed_task_shard)
      .ok_or_else(|| anyhow::anyhow!("delayed task shard not available"))?;

    let naive = NaiveDateTime::from_timestamp(ts_secs, 0);
    let utc: DateTime<Utc> = DateTime::from_utc(naive, Utc);
    let dt_id = format!("{}/{}", time_to_prefix(&utc), Uuid::new_v4());
    let entry = rmp_serde::to_vec(&entry)?;
    shard
      .compare_and_set_many([(
        dt_id.as_str(),
        TriStateCheck::<&[u8]>::Any,
        TriStateSet::Value(entry),
      )])
      .await?;

    Ok(dt_id)
  }
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
    let channels = group_kv_rows_by_prefix(&rows);
    let mut out: Vec<(String, Arc<ChannelSender>)> = vec![];
    for (k, v) in channels {
      if let Some(c) = v.get("config") {
        if let Ok(c) = serde_json::from_slice::<ChannelConfig>(*c) {
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
              kafka_producer: producer,
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
  if !mds.has_metadata_shard() {
    return Ok(vec![]);
  }
  let sess = match mds.get_metadata_shard_session() {
    Some(sess) => sess,
    None => anyhow::bail!("cannot get metadata shard session"),
  };
  let region_scheduler_prefix = format!("task-scheduler/{}", get_mds().unwrap().get_local_region());
  sess
    .prefix_list(
      &region_scheduler_prefix,
      PrefixListOptions {
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
        PrefixListOptions {
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
    let mut channels = group_kv_rows_by_prefix(&rows);

    channels.shuffle(&mut rand::thread_rng());

    for (channel, props) in &channels {
      let original_lock_raw = props.get("lock").copied();
      let lock: Option<ChannelLock> =
        original_lock_raw.and_then(|x| serde_json::from_slice(x).ok());
      match &lock {
        Some(x) if now.saturating_sub(x.ts) > TASK_LOCK_TTL.as_millis() as u64 => {
          tracing::info!(%channel, previous_owner = %x.identity, previous_ts = %x.ts, "previous lock expired");
        }
        Some(x) if identity == x.identity => {
          tracing::info!(%channel, previous_owner = %x.identity, previous_ts = %x.ts, "taking back our own lock");
        }
        Some(_) => continue,
        None => {}
      }

      let lock = ChannelLock {
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
            tracing::warn!(%identity, %channel, "channel lock acquired");
            match channel_worker(&identity, *channel, props, &mut lock_json, &mut tx).await {
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
  identity: &str,
  channel: &str,
  props: &HashMap<&str, &[u8]>,
  lock_json: &mut Vec<u8>,
  tx: &mut tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
) -> Result<()> {
  let config_json = *props
    .get("config")
    .ok_or_else(|| anyhow::anyhow!("missing config"))?;
  let config: ChannelConfig = serde_json::from_slice(config_json)?;

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
  let _g_delayed = config
    .delayed_task_shard
    .as_ref()
    .map(|x| Guard(tokio::spawn(delayed_task_worker(x.clone(), tx.clone()))));

  loop {
    let backoff_start = Instant::now();
    random_backoff().await;
    let work_start = Instant::now();

    tokio::select! {
      cont = renew_once(channel, config_json, lock_json) => {
        if !cont? {
          return Ok(());
        }
        let total_duration = backoff_start.elapsed();
        let work_duration = work_start.elapsed();
        tracing::info!(%identity, %channel, ?total_duration, ?work_duration, "lock renewal");
      }
      _ = tokio::time::sleep(TASK_LOCK_RENEWAL_TIMEOUT) => {
        tracing::error!("exceeded lock renewal timeout");
        anyhow::bail!("exceeded lock renewal timeout");
      }
    }
  }
}

async fn renew_once(channel: &str, old_config: &[u8], lock_json: &mut Vec<u8>) -> Result<bool> {
  let mds = get_mds().unwrap();
  let sess = mds
    .get_metadata_shard_session()
    .ok_or_else(|| anyhow::anyhow!("no metadata shard session"))?;
  let region_scheduler_prefix = format!("task-scheduler/{}", mds.get_local_region());

  let new_config = sess
    .get_many(
      &[format!("{}/{}/config", region_scheduler_prefix, channel)],
      true,
    )
    .await?
    .into_iter()
    .next()
    .unwrap();
  if new_config.as_ref().map(|x| x.as_slice()) != Some(old_config) {
    log::warn!("config changed for channel {}", channel);
    return Ok(false);
  }

  // Renew lock
  let mut lock: ChannelLock = serde_json::from_slice(lock_json.as_slice())?;
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
    return Ok(false);
  }
  *lock_json = new_lock_json;
  Ok(true)
}

async fn delayed_task_worker<T: for<'x> Deserialize<'x> + Send + 'static>(
  shard_name: String,
  mut tx: tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
) {
  loop {
    delayed_task_worker_once(&shard_name, &mut tx).await;
  }
}

async fn delayed_task_worker_once<T: for<'x> Deserialize<'x> + Send + 'static>(
  shard_name: &str,
  tx: &mut tokio::sync::mpsc::Sender<(T, TaskCompletion)>,
) {
  let (completion_tx, mut completion_rx): (
    tokio::sync::mpsc::UnboundedSender<TaskCompletionMessage>,
    tokio::sync::mpsc::UnboundedReceiver<TaskCompletionMessage>,
  ) = tokio::sync::mpsc::unbounded_channel();
  let now = time_to_prefix(&Utc::now());

  let tasks = match get_shard_session_with_infinite_retry_assuming_mds_exists(shard_name)
    .await
    .prefix_list(
      "",
      PrefixListOptions {
        reverse: false,
        want_value: true,
        limit: 100,
        cursor: None,
      },
      true,
    )
    .await
  {
    Ok(tasks) => tasks,
    Err(e) => {
      log::error!("failed to list tasks: {}", e);
      random_backoff().await;
      return;
    }
  };

  let mut processed_task_count = 0usize;

  for (k, payload) in &tasks {
    if k.as_str() > now.as_str() {
      break;
    }
    let value = match rmp_serde::from_read(&payload[..]) {
      Ok(x) => x,
      Err(e) => {
        log::error!("real_channel_worker: error while deserializing: {}", e);
        continue;
      }
    };
    let _ = tx
      .send((
        value,
        TaskCompletion {
          msg: TaskCompletionMessage {
            delayed_task_prefix: Some(k.to_string()),
            partition: -1,
            offset: -1,
          },
          tx: completion_tx.clone(),
        },
      ))
      .await;
    processed_task_count += 1;
  }
  drop(completion_tx);

  if processed_task_count == 0 {
    random_backoff().await;
    return;
  }

  loop {
    let msg = match completion_rx.recv().await {
      Some(x) => x,
      None => break,
    };
    loop {
      let shard = get_shard_session_with_infinite_retry_assuming_mds_exists(shard_name).await;
      if let Err(e) = shard
        .prefix_delete(msg.delayed_task_prefix.as_ref().unwrap())
        .await
      {
        log::warn!("failed to delete delayed task, retrying: {}", e);
        random_backoff().await;
      } else {
        break;
      }
    }
  }
  log::info!(
    "finished delayed task batch of size {}",
    processed_task_count
  );
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

        let value: T = match rmp_serde::from_read(payload) {
          Ok(x) => x,
          Err(e) => {
            log::error!("real_channel_worker: error while deserializing: {}", e);
            continue;
          }
        };

        let partition_backlog = backlog.entry(partition).or_insert_with(BTreeMap::new);
        partition_backlog.insert(offset, false);
        if partition_backlog.len() > THRESHOLD {
          log::warn!(
            "real_channel_worker: backlog is too large ({}), partition {}",
            partition_backlog.len(),
            partition
          );
        }
        let completion = TaskCompletion {
          tx: completion_tx.clone(),
          msg: TaskCompletionMessage {
            partition,
            offset,
            delayed_task_prefix: None,
          },
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

fn time_to_prefix(t: &DateTime<Utc>) -> String {
  t.format("%Y/%m/%d/%H/%M/%S").to_string()
}
