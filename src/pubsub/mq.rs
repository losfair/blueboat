use std::{
  collections::HashSet,
  sync::{Arc, Mutex, Weak},
  time::Duration,
};

use anyhow::Result;
use byteorder::{ByteOrder, LittleEndian};
use foundationdb::{
  options::{MutationType, StreamingMode},
  Database, RangeOption,
};
use itertools::Itertools;

pub struct MessageQueue {
  db: Database,
  config: MessageQueueConfig,
  hot_topics: Mutex<HashSet<([u8; 16], String)>>,
}

pub struct MessageQueueConfig {
  pub fdb_cluster_file: String,
  pub prefix: String,
}

impl MessageQueue {
  pub fn open(config: MessageQueueConfig) -> Result<Arc<Self>> {
    let db = Database::new(Some(config.fdb_cluster_file.as_str()))
      .map_err(|e| anyhow::Error::from(e).context(format!("mq: failed to open cluster")))?;
    let me = Arc::new(Self {
      db,
      config,
      hot_topics: Mutex::new(Default::default()),
    });

    {
      let me = Arc::downgrade(&me);
      std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
          .enable_all()
          .build()
          .unwrap()
          .block_on(Self::flush_thread(me));
      });
    }

    Ok(me)
  }

  pub fn namespace(self: &Arc<Self>, ns: [u8; 16]) -> MessageQueueNamespace {
    MessageQueueNamespace {
      mq: self.clone(),
      ns,
    }
  }

  async fn flush_thread(me: Weak<Self>) {
    tracing::warn!("mq flush thread started");
    loop {
      tokio::time::sleep(Duration::from_secs(1)).await;
      let me = if let Some(x) = me.upgrade() {
        x
      } else {
        break;
      };
      if let Err(e) = me.flush_hot_topics().await {
        tracing::error!(error = %e, "mq flush failed");
      }
    }
    tracing::warn!("mq flush thread exiting");
  }

  pub async fn flush_hot_topics(&self) -> Result<()> {
    let hot_topics = std::mem::replace(&mut *self.hot_topics.lock().unwrap(), Default::default());
    if hot_topics.is_empty() {
      return Ok(());
    }

    for batch in &hot_topics.iter().chunks(100) {
      let batch = batch.collect::<Vec<_>>();
      let mut txn = self.db.create_trx()?;
      loop {
        for (ns, topic) in &batch {
          let mut key = foundationdb::tuple::pack(&self.config.prefix.as_str());
          key.push(b'h'); // "hot"
          key.extend_from_slice(ns);
          key.extend_from_slice(&foundationdb::tuple::pack(&topic.as_str())[..]);
          txn.set(&key, b"1");
        }
        match txn.commit().await {
          Ok(_) => {
            break;
          }
          Err(e) => {
            txn = e.on_error().await?;
          }
        }
      }
    }

    Ok(())
  }
}

pub struct MessageQueueNamespace {
  mq: Arc<MessageQueue>,
  ns: [u8; 16],
}

impl MessageQueueNamespace {
  pub fn topic(&self, topic: String) -> MessageQueueTopic {
    let mut common_prefix = foundationdb::tuple::pack(&self.mq.config.prefix.as_str());
    common_prefix.push(b'q'); // "queue"
    common_prefix.extend_from_slice(&self.ns);
    common_prefix.extend_from_slice(&foundationdb::tuple::pack(&topic.as_str())[..]);
    let mut data_key_template = common_prefix
      .iter()
      .copied()
      .chain(std::iter::once(b'd')) // "data"
      .chain([0u8; 14].into_iter())
      .collect::<Vec<_>>();
    {
      let pos_offset = data_key_template.len() - 4;
      let versionstamp_offset = data_key_template.len() - 14;
      LittleEndian::write_u32(
        &mut data_key_template[pos_offset..],
        versionstamp_offset as u32,
      );
    }
    let counter_key = common_prefix
      .iter()
      .copied()
      .chain(std::iter::once(b'c')) // "counter"
      .collect::<Vec<_>>();
    MessageQueueTopic {
      mq: self.mq.clone(),
      ns: self.ns,
      topic,
      data_key_template,
      counter_key,
    }
  }
}

pub struct MessageQueueTopic {
  mq: Arc<MessageQueue>,
  ns: [u8; 16],
  topic: String,
  data_key_template: Vec<u8>,
  counter_key: Vec<u8>,
}

impl MessageQueueTopic {
  pub async fn push(&self, data: &[u8]) -> Result<[u8; 10]> {
    let counter_buf = [0u8; 14];
    let mut txn = self.mq.db.create_trx()?;
    loop {
      txn.atomic_op(
        &self.data_key_template,
        data,
        MutationType::SetVersionstampedKey,
      );
      txn.atomic_op(
        &self.counter_key,
        &counter_buf,
        MutationType::SetVersionstampedValue,
      );
      let versionstamp_fut = txn.get_versionstamp();
      match txn.commit().await {
        Ok(_) => {
          let versionstamp = versionstamp_fut.await?;
          self
            .mq
            .hot_topics
            .lock()
            .unwrap()
            .insert((self.ns, self.topic.clone()));
          return Ok(versionstamp[..].try_into().map_err(|e| {
            anyhow::Error::from(e).context("cannot resolve committed versionstamp")
          })?);
        }
        Err(e) => {
          txn = e.on_error().await?;
        }
      }
    }
  }

  pub async fn seq(&self) -> Result<[u8; 10]> {
    let txn = self.mq.db.create_trx()?;
    let counter = txn.get(&self.counter_key, false).await?;
    let out: [u8; 10] = match counter {
      Some(x) if x.len() == 10 => x[..].try_into().unwrap(),
      Some(_) => anyhow::bail!("invalid counter"),
      None => [0u8; 10],
    };
    Ok(out)
  }

  pub async fn pop(
    &self,
    from_seq: [u8; 10],
    n: usize,
    wait: bool,
  ) -> Result<Vec<([u8; 10], Vec<u8>)>> {
    let mut txn = self.mq.db.create_trx()?;
    loop {
      let counter = txn.get(&self.counter_key, false).await?;
      match counter {
        Some(x) if x.len() == 10 => {
          let current_seq: [u8; 10] = x[..].try_into().unwrap();
          if current_seq > from_seq {
            break;
          }

          if current_seq < from_seq {
            anyhow::bail!("current_seq is less than from_seq");
          }
        }
        Some(_) => anyhow::bail!("invalid counter"),
        None => {}
      }

      if !wait {
        return Ok(vec![]);
      }

      let watch = txn.watch(&self.counter_key);
      match txn.commit().await {
        Ok(res) => {
          txn = res.reset();
          match watch.await {
            Ok(()) => {}
            Err(e) => {
              txn = txn.on_error(e).await?;
            }
          }
        }
        Err(e) => {
          txn = e.on_error().await?;
        }
      }
    }

    let mut data_key_start = self.data_key_template.clone();
    {
      let versionstamp_offset = data_key_start.len() - 14;
      data_key_start[versionstamp_offset..versionstamp_offset + 10].copy_from_slice(&from_seq[..]);
      // the last 4 bytes are dirty but they don't matter
    }
    let strip_len = data_key_start.len() - 14;
    let mut data_key_end = data_key_start[..strip_len].to_vec();
    assert_eq!(data_key_end[data_key_end.len() - 1], b'd');
    *data_key_end.last_mut().unwrap() = b'e';

    let mut opt = RangeOption::from((data_key_start, data_key_end));
    opt.mode = StreamingMode::WantAll;
    opt.limit = Some(n);
    let range = txn.get_range(&opt, 0, false).await?;

    if range.is_empty() {
      anyhow::bail!("counter advanced but no more data");
    }

    let mut output: Vec<([u8; 10], Vec<u8>)> = vec![];
    for kv in &range {
      let key = kv.key();
      if key.len() != strip_len + 10 {
        anyhow::bail!("got invalid key length in result");
      }
      output.push((key[strip_len..].try_into().unwrap(), kv.value().to_vec()));
    }
    Ok(output)
  }
}
