use crate::{
    app::{AppConfig, AppId},
    types::*,
    util::current_millis,
};
use mysql_async::{params, prelude::Queryable, Pool};
use rand::Rng;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use std::{
    collections::BTreeMap,
    time::{Duration, UNIX_EPOCH},
};
use tikv_client::{BoundRange, CheckLevel, Transaction, TransactionOptions};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Semaphore;

pub static PREFIX_WORKER_DATA_V2: &'static [u8] = b"W\x00V2\x00W\x00";

const MAX_LOCKS_PER_WORKER_DATA_TRANSACTION: usize = 256;

pub struct DataClient {
    transactional: tikv_client::TransactionClient,
    txn_collector_tx: Sender<Transaction>,
    db: Pool,
}

pub struct WorkerDataTransaction {
    protected: ProtectedTransaction,
    num_locks: usize,
}

impl WorkerDataTransaction {
    pub async fn get(
        &mut self,
        namespace_id: &[u8; 16],
        key: &[u8],
    ) -> GenericResult<Option<Vec<u8>>> {
        self.protected
            .get(make_worker_data_key(namespace_id, key))
            .await
            .map_err(tikv_error_to_generic)
    }

    pub async fn lock_keys(
        &mut self,
        namespace_id: &[u8; 16],
        keys: impl Iterator<Item = &[u8]>,
    ) -> GenericResult<bool> {
        let keys: Vec<_> = keys
            .map(|key| make_worker_data_key(namespace_id, key))
            .collect();
        let new_num_locks = self.num_locks.saturating_add(keys.len());
        if new_num_locks > MAX_LOCKS_PER_WORKER_DATA_TRANSACTION {
            return Ok(false);
        }
        self.num_locks = new_num_locks;

        self.protected
            .lock_keys(keys)
            .await
            .map_err(tikv_error_to_generic)
            .map(|_| true)
    }

    pub async fn delete(&mut self, namespace_id: &[u8; 16], key: &[u8]) -> GenericResult<()> {
        self.protected
            .delete(make_worker_data_key(namespace_id, key))
            .await
            .map_err(tikv_error_to_generic)
    }

    pub async fn put(
        &mut self,
        namespace_id: &[u8; 16],
        key: &[u8],
        value: Vec<u8>,
    ) -> GenericResult<()> {
        self.protected
            .put(make_worker_data_key(namespace_id, key), value)
            .await
            .map_err(tikv_error_to_generic)
    }

    /// Scans keys from `start` to `end`.
    ///
    /// We only provide `scan_keys` but not `scan` because values can be large and I don't want to deal
    /// with the complexity there.
    pub async fn scan_keys(
        &mut self,
        namespace_id: &[u8; 16],
        start: &[u8],
        end: Option<&[u8]>,
        limit: u32,
    ) -> GenericResult<Vec<Vec<u8>>> {
        let prefix = worker_data_key_prefix(namespace_id);
        let start = make_worker_data_key(namespace_id, start);
        let end = end.map(|x| make_worker_data_key(namespace_id, x));
        let range: BoundRange = if let Some(end) = end {
            (start..end).into()
        } else {
            (start..).into()
        };
        self.protected
            .scan_keys(range, limit)
            .await
            .map_err(tikv_error_to_generic)
            .map(|x| x.map(|x| Vec::from(x)[prefix.len()..].to_vec()).collect())
    }

    pub async fn commit(self) -> GenericResult<bool> {
        self.protected.commit().await
    }

    pub async fn rollback(self) -> GenericResult<()> {
        self.protected.rollback().await
    }
}

/// A protected transaction is automatically rolled back when dropped.
///
/// This is important as asynchronous tasks can be cancelled.
struct ProtectedTransaction {
    txn: Option<Transaction>,
    txn_collector_tx: Sender<Transaction>,
}

impl Drop for ProtectedTransaction {
    fn drop(&mut self) {
        if let Some(txn) = self.txn.take() {
            drop(self.txn_collector_tx.try_send(txn));
        }
    }
}

impl Deref for ProtectedTransaction {
    type Target = Transaction;
    fn deref(&self) -> &Self::Target {
        self.txn.as_ref().unwrap()
    }
}

impl DerefMut for ProtectedTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.txn.as_mut().unwrap()
    }
}

impl ProtectedTransaction {
    async fn commit(mut self) -> GenericResult<bool> {
        let mut txn = self.txn.take().unwrap();
        txn.commit().await.map(|_| true).or_else(|e| match e {
            tikv_client::Error::KeyError(e) if e.conflict.is_some() => Ok(false),
            e => Err(tikv_error_to_generic(e)),
        })
    }

    async fn rollback(mut self) -> GenericResult<()> {
        let mut txn = self.txn.take().unwrap();
        txn.rollback()
            .await
            .map(|_| ())
            .map_err(tikv_error_to_generic)
    }
}

impl DataClient {
    pub async fn new<S: Into<String> + Clone>(
        pd_endpoints: Vec<S>,
        db_url: &str,
    ) -> GenericResult<Self> {
        let transactional = tikv_client::TransactionClient::new(pd_endpoints)
            .await
            .map_err(|e| {
                GenericError::Other(format!(
                    "tikv (transactional) initialization failed: {:?}",
                    e
                ))
            })?;
        let db = Pool::from_url(db_url)
            .map_err(|e| GenericError::Other(format!("db connection failed: {:?}", e,)))?;
        let (txn_collector_tx, txn_collector_rx) = channel(1000);
        tokio::spawn(async move {
            txn_collector_worker(txn_collector_rx).await;
        });
        Ok(Self {
            transactional,
            txn_collector_tx,
            db,
        })
    }

    /// Creates a "protected" transaction that rolls back automatically if neither committed nor rolled back.
    ///
    /// Asynchronous tasks can be cancelled. So this is important.
    async fn new_protected_transaction(
        &self,
        opts: TransactionOptions,
    ) -> GenericResult<ProtectedTransaction> {
        // If we run out of space in `txn_collector_tx` the transaction may be dropped without being committed or
        // rolled back. Let's print a warning in this case.
        let opts = opts.drop_check(CheckLevel::Warn);

        let txn = self
            .transactional
            .begin_with_options(opts)
            .await
            .map_err(tikv_error_to_generic)?;
        Ok(ProtectedTransaction {
            txn: Some(txn),
            txn_collector_tx: self.txn_collector_tx.clone(),
        })
    }

    pub async fn worker_data_get(
        &self,
        namespace_id: &[u8; 16],
        key: &[u8],
    ) -> GenericResult<Option<Vec<u8>>> {
        // From tikv_client documentation: "While it is possible to use both APIs at the same time,
        // doing so is unsafe and unsupported."
        //
        // So here we use transactional API for all worker data operations.
        let mut txn = self
            .transactional
            .begin_with_options(TransactionOptions::new_optimistic().read_only())
            .await
            .map_err(tikv_error_to_generic)?;
        txn.get(make_worker_data_key(namespace_id, key))
            .await
            .map_err(|e| GenericError::Other(format!("worker_data_get: {:?}", e)))
    }

    pub async fn worker_data_put(
        &self,
        namespace_id: &[u8; 16],
        key: &[u8],
        value: Vec<u8>,
    ) -> GenericResult<()> {
        let mut txn = self
            .new_protected_transaction(TransactionOptions::new_optimistic())
            .await?;
        let result = txn
            .put(make_worker_data_key(namespace_id, key), value)
            .await
            .map_err(tikv_error_to_generic);
        if let Err(e) = result {
            drop(txn.rollback().await);
            Err(e)
        } else {
            txn.commit().await.map(|_| ()) // TODO: conflicts?
        }
    }

    pub async fn worker_data_scan_keys(
        &self,
        namespace_id: &[u8; 16],
        start: &[u8],
        end: Option<&[u8]>,
        limit: u32,
    ) -> GenericResult<Vec<Vec<u8>>> {
        let mut txn = self
            .transactional
            .begin_with_options(TransactionOptions::new_optimistic().read_only())
            .await
            .map_err(tikv_error_to_generic)?;
        let prefix = worker_data_key_prefix(namespace_id);
        let start = make_worker_data_key(namespace_id, start);
        let end = end
            .map(|x| make_worker_data_key(namespace_id, x))
            .unwrap_or_else(|| join_slices(&[PREFIX_WORKER_DATA_V2, namespace_id, b"\x01"]));
        txn.scan_keys(start..end, limit)
            .await
            .map_err(tikv_error_to_generic)
            .map(|x| x.map(|x| Vec::from(x)[prefix.len()..].to_vec()).collect())
    }

    pub async fn worker_data_delete(
        &self,
        namespace_id: &[u8; 16],
        key: &[u8],
    ) -> GenericResult<()> {
        let mut txn = self
            .new_protected_transaction(TransactionOptions::new_optimistic())
            .await?;
        let result = txn
            .delete(make_worker_data_key(namespace_id, key))
            .await
            .map_err(tikv_error_to_generic);
        if let Err(e) = result {
            drop(txn.rollback().await);
            Err(e)
        } else {
            txn.commit().await.map(|_| ()) // TODO: conflicts?
        }
    }

    pub async fn worker_data_begin_transaction(&self) -> GenericResult<WorkerDataTransaction> {
        // Only support optimistic mode for now.
        let txn = self
            .new_protected_transaction(TransactionOptions::new_optimistic())
            .await?;

        Ok(WorkerDataTransaction {
            protected: txn,
            num_locks: 0,
        })
    }

    pub async fn route_mapping_delete_domain(&self, domain: &str) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop("delete from routes where `domain` = ?", (domain,))
            .await?;
        Ok(())
    }

    pub async fn route_mapping_list_for_domain(
        &self,
        domain: &str,
    ) -> GenericResult<BTreeMap<String, String>> {
        let mut conn = self.db.get_conn().await?;
        let items: Vec<(String, String)> = conn
            .exec(
                "select path, appid from routes where `domain` = ?",
                (domain,),
            )
            .await?;

        Ok(items.into_iter().collect())
    }

    pub async fn route_mapping_lookup(
        &self,
        domain: &str,
        path: &str,
    ) -> GenericResult<Option<String>> {
        let mut conn = self.db.get_conn().await?;
        let appid: Option<String> = conn.exec_first(
            "select appid from routes where `domain` = ? and ? like concat(`path`, '%') order by length(`path`) desc limit 1",
            (domain, path)
        ).await?;
        // Most specific match
        Ok(appid)
    }

    pub async fn route_mapping_insert(
        &self,
        domain: &str,
        path: &str,
        appid: String,
    ) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop(
            "insert into routes (domain, path, appid, createtime) values(?, ?, ?, ?)",
            (domain, path, appid, current_millis()),
        )
        .await?;
        Ok(())
    }

    pub async fn route_mapping_delete(&self, domain: &str, path: &str) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop(
            "delete from routes where domain = ? and path = ?",
            (domain, path),
        )
        .await?;
        Ok(())
    }

    pub async fn app_metadata_get(&self, appid: &str) -> GenericResult<Option<AppConfig>> {
        let mut conn = self.db.get_conn().await?;
        let (bundle_id, env, kv_namespaces): (String, String, String) = match conn
            .exec_first(
                "select bundle_id, env, kv_namespaces from apps where id = ?",
                (appid,),
            )
            .await?
        {
            Some(x) => x,
            None => return Ok(None),
        };

        let config = AppConfig {
            id: AppId(appid.to_string()),
            bundle_id,
            env: serde_json::from_str(&env)?,
            kv_namespaces: serde_json::from_str(&kv_namespaces)?,
        };

        Ok(Some(config))
    }

    pub async fn app_metadata_put(&self, config: &AppConfig) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop(
            format!(
                "{} on duplicate key {}",
                "insert into apps (id, bundle_id, env, kv_namespaces, createtime) values(:id, :bundle_id, :env, :kv_namespaces, :createtime)",
                "update bundle_id = :bundle_id, env = :env, kv_namespaces = :kv_namespaces",
            ),
            params! {
                "id" => &config.id.0,
                "bundle_id" => &config.bundle_id,
                "env" => serde_json::to_string(&config.env)?,
                "kv_namespaces" => serde_json::to_string(&config.kv_namespaces)?,
                "createtime" => current_millis(),
            },
        ).await?;
        Ok(())
    }

    pub async fn app_metadata_delete(&self, appid: &str) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop("delete from apps where id = ?", (appid,))
            .await?;
        Ok(())
    }

    pub async fn app_bundle_get(&self, id: &str) -> GenericResult<Option<Vec<u8>>> {
        let mut conn = self.db.get_conn().await?;
        let bundle: Option<Vec<u8>> = conn
            .exec_first("select bundle from bundles where id = ?", (id,))
            .await?;
        Ok(bundle)
    }

    pub async fn app_bundle_put(&self, id: &str, value: &[u8]) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop(
            "insert into bundles (id, bundle, createtime) values(?, ?, ?)",
            (id, value, current_millis()),
        )
        .await?;
        Ok(())
    }

    pub async fn applog_write(
        &self,
        appid: &str,
        logtime: SystemTime,
        logcontent: &str,
    ) -> GenericResult<()> {
        let subid: u32 = rand::thread_rng().gen();
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop(
            "insert into applog (appid, logtime, subid, logcontent) values(?, ?, ?, ?)",
            (
                appid,
                logtime
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_else(|_| Duration::from_millis(0))
                    .as_millis() as u64,
                subid,
                logcontent,
            ),
        )
        .await?;
        Ok(())
    }
}

fn worker_data_key_prefix(namespace_id: &[u8; 16]) -> Vec<u8> {
    join_slices(&[PREFIX_WORKER_DATA_V2, namespace_id, b"\x00"])
}

fn make_worker_data_key(namespace_id: &[u8; 16], key: &[u8]) -> Vec<u8> {
    join_slices(&[PREFIX_WORKER_DATA_V2, namespace_id, b"\x00", key])
}

fn join_slices(slices: &[&[u8]]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(slices.iter().map(|x| x.len()).sum());
    for s in slices {
        buf.extend_from_slice(s);
    }
    buf
}

async fn txn_collector_worker(mut rx: Receiver<Transaction>) {
    // Concurrency control: Don't blow up.
    let sem = Arc::new(Semaphore::new(8));

    loop {
        let mut txn = match rx.recv().await {
            Some(x) => x,
            None => return,
        };
        let permit = sem.clone().acquire_owned();
        tokio::spawn(async move {
            // Guard
            let _permit = permit;

            let start_time = Instant::now();
            let res = txn.rollback().await;
            let end_time = Instant::now();
            debug!(
                "rolled back dropped transaction in {:?}: {:?}",
                end_time.duration_since(start_time),
                res
            );
        });
    }
}

fn tikv_error_to_generic(e: tikv_client::Error) -> GenericError {
    GenericError::Other(format!("tikv error: {:?}", e))
}
