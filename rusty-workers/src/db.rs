use crate::types::*;
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use mysql_async::Pool;
use tikv_client::{BoundRange, CheckLevel, Key, KvPair, Transaction, TransactionOptions};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Semaphore;

macro_rules! impl_scan_prefix {
    ($name:ident, $cb_value_ty:ty, $scan_func:ident, $deref_key:ident, $deref_value:ident) => {
        async fn $name(
            &self,
            prefix: &[u8],
            mut cb: impl FnMut(&[u8], $cb_value_ty) -> bool,
        ) -> GenericResult<()> {
            assert!(prefix.len() > 0, "scan_prefix: prefix must be non-empty");
            assert!(
                *prefix.last().unwrap() == 0,
                "scan_prefix: prefix must end with zero"
            );

            let batch_size: u32 = 20;

            let mut start_prefix = prefix.to_vec();
            let mut end_prefix = prefix.to_vec();
            *end_prefix.last_mut().unwrap() = 1;

            loop {
                let batch = self
                    .raw
                    .$scan_func(start_prefix..end_prefix.clone(), batch_size)
                    .await
                    .map_err(|e| GenericError::Other(format!("scan_prefix: {:?}", e)))?;
                for item in batch.iter() {
                    let key: &[u8] = $deref_key(item);
                    if key.starts_with(prefix) {
                        if !cb(&key[prefix.len()..], $deref_value(item)) {
                            return Ok(());
                        }
                    } else {
                        error!("scan_prefix: invalid data from database");
                        return Ok(());
                    }
                }
                if batch.len() == batch_size as usize {
                    // The immediate next key
                    start_prefix = join_slices(&[$deref_key(batch.last().unwrap()), &[0u8]]);
                } else {
                    return Ok(());
                }
            }
        }
    };
}

pub static PREFIX_WORKER_DATA_V2: &'static [u8] = b"W\x00V2\x00W\x00";

pub static PREFIX_APP_METADATA_V1: &'static [u8] = b"W\x00V1\x00APPMD\x00";

pub static PREFIX_APP_BUNDLE_V1: &'static [u8] = b"W\x00V1\x00APPBUNDLE\x00";

pub static PREFIX_ROUTE_MAPPING_V1: &'static [u8] = b"W\x00V1\x00ROUTEMAP\x00";

pub static PREFIX_LOG_V1: &'static [u8] = b"W\x00V1\x00LOG\x00";

const MAX_LOCKS_PER_WORKER_DATA_TRANSACTION: usize = 256;

pub struct DataClient {
    raw: tikv_client::RawClient,
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
    pub async fn new<S: Into<String> + Clone>(pd_endpoints: Vec<S>, db_url: &str) -> GenericResult<Self> {
        let raw = tikv_client::RawClient::new(pd_endpoints.clone())
            .await
            .map_err(|e| {
                GenericError::Other(format!("tikv (raw) initialization failed: {:?}", e))
            })?;
        let transactional = tikv_client::TransactionClient::new(pd_endpoints)
            .await
            .map_err(|e| {
                GenericError::Other(format!(
                    "tikv (transactional) initialization failed: {:?}",
                    e
                ))
            })?;
        let db = Pool::from_url(db_url)
            .map_err(|e| {
                GenericError::Other(format!(
                    "db connection failed: {:?}",
                    e,
                ))
            })?;
        let (txn_collector_tx, txn_collector_rx) = channel(1000);
        tokio::spawn(async move {
            txn_collector_worker(txn_collector_rx).await;
        });
        Ok(Self {
            raw,
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

    async fn delete_prefix(&self, prefix: &[u8]) -> GenericResult<()> {
        assert!(prefix.len() > 0, "delete_prefix: prefix must be non-empty");
        assert!(
            *prefix.last().unwrap() == 0,
            "delete_prefix: prefix must end with zero"
        );
        let start_prefix = prefix.to_vec();
        let mut end_prefix = prefix.to_vec();
        *end_prefix.last_mut().unwrap() = 1;

        self.raw
            .delete_range(start_prefix..end_prefix)
            .await
            .map_err(|e| GenericError::Other(format!("delete_prefix: {:?}", e)))
    }

    impl_scan_prefix!(scan_prefix, &[u8], scan, kvp_deref_key, kvp_deref_value);
    impl_scan_prefix!(scan_prefix_keys, (), scan_keys, key_deref, mk_unit);

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
        let prefix = join_slices(&[PREFIX_ROUTE_MAPPING_V1, domain.as_bytes(), b"\x00"]);
        self.delete_prefix(&prefix).await
    }

    pub async fn route_mapping_for_each(
        &self,
        mut callback: impl FnMut(&str, &str, &str) -> bool,
    ) -> GenericResult<()> {
        self.scan_prefix(PREFIX_ROUTE_MAPPING_V1, |k, v| {
            use std::str::from_utf8;
            let mut parts = k.split(|x| *x == 0);
            let domain = parts.next().unwrap_or(b"");
            let path = parts.next().unwrap_or(b"");
            callback(
                from_utf8(domain).unwrap_or(""),
                from_utf8(path).unwrap_or(""),
                from_utf8(v).unwrap_or(""),
            )
        })
        .await?;
        Ok(())
    }

    pub async fn route_mapping_list_for_domain(
        &self,
        domain: &str,
        mut filter: impl FnMut(&[u8]) -> bool,
    ) -> GenericResult<BTreeMap<String, String>> {
        let prefix = join_slices(&[PREFIX_ROUTE_MAPPING_V1, domain.as_bytes(), b"\x00"]);
        let mut result = BTreeMap::new();
        self.scan_prefix(&prefix, |k, v| {
            if filter(k) {
                result.insert(
                    String::from_utf8_lossy(k).into_owned(),
                    String::from_utf8_lossy(v).into_owned(),
                );
            }
            true
        })
        .await?;
        Ok(result)
    }

    pub async fn route_mapping_lookup(
        &self,
        domain: &str,
        path: &str,
    ) -> GenericResult<Option<String>> {
        let matches = self
            .route_mapping_list_for_domain(domain, |x| path.as_bytes().starts_with(x))
            .await?;

        // Most specific match
        Ok(matches.into_iter().rev().next().map(|x| x.1))
    }

    pub async fn route_mapping_insert(
        &self,
        domain: &str,
        path: &str,
        appid: String,
    ) -> GenericResult<()> {
        let key = join_slices(&[
            PREFIX_ROUTE_MAPPING_V1,
            domain.as_bytes(),
            b"\x00",
            path.as_bytes(),
        ]);
        self.raw
            .put(key, Vec::from(appid))
            .await
            .map_err(|e| GenericError::Other(format!("route_mapping_insert: {:?}", e)))
    }

    pub async fn route_mapping_delete(&self, domain: &str, path: &str) -> GenericResult<()> {
        let key = join_slices(&[
            PREFIX_ROUTE_MAPPING_V1,
            domain.as_bytes(),
            b"\x00",
            path.as_bytes(),
        ]);
        self.raw
            .delete(key)
            .await
            .map_err(|e| GenericError::Other(format!("route_mapping_delete: {:?}", e)))
    }

    pub async fn app_metadata_for_each(
        &self,
        mut callback: impl FnMut(&str) -> bool,
    ) -> GenericResult<()> {
        self.scan_prefix_keys(PREFIX_APP_METADATA_V1, |k, ()| {
            let appid = std::str::from_utf8(k).unwrap_or("");
            callback(appid)
        })
        .await?;
        Ok(())
    }

    pub async fn app_metadata_get(&self, appid: &str) -> GenericResult<Option<Vec<u8>>> {
        let key = join_slices(&[PREFIX_APP_METADATA_V1, appid.as_bytes()]);
        self.raw
            .get(key)
            .await
            .map_err(|e| GenericError::Other(format!("app_metadata_get: {:?}", e)))
    }

    pub async fn app_metadata_put(&self, appid: &str, value: Vec<u8>) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_METADATA_V1, appid.as_bytes()]);
        self.raw
            .put(key, value)
            .await
            .map_err(|e| GenericError::Other(format!("app_metadata_put: {:?}", e)))
    }

    pub async fn app_metadata_delete(&self, appid: &str) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_METADATA_V1, appid.as_bytes()]);
        self.raw
            .delete(key)
            .await
            .map_err(|e| GenericError::Other(format!("app_metadata_delete: {:?}", e)))
    }

    pub async fn app_bundle_for_each(
        &self,
        mut callback: impl FnMut(&[u8]) -> bool,
    ) -> GenericResult<()> {
        self.scan_prefix_keys(PREFIX_APP_BUNDLE_V1, |k, ()| callback(k))
            .await?;
        Ok(())
    }

    pub async fn app_bundle_get(&self, id: &[u8; 16]) -> GenericResult<Option<Vec<u8>>> {
        let key = join_slices(&[PREFIX_APP_BUNDLE_V1, id]);
        self.raw
            .get(key)
            .await
            .map_err(|e| GenericError::Other(format!("app_bundle_get: {:?}", e)))
    }

    pub async fn app_bundle_put(&self, id: &[u8; 16], value: Vec<u8>) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_BUNDLE_V1, id]);
        self.raw
            .put(key, value)
            .await
            .map_err(|e| GenericError::Other(format!("app_bundle_put: {:?}", e)))
    }

    /// Deletes an app bundle.
    pub async fn app_bundle_delete(&self, id: &[u8; 16]) -> GenericResult<()> {
        self.app_bundle_delete_dirty(id).await
    }

    /// Deletes an app bundle.
    ///
    /// Argument is not restricted to [u8; 16] because we want to allow deleting "dirty" data.
    pub async fn app_bundle_delete_dirty(&self, id: &[u8]) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_BUNDLE_V1, id]);
        self.raw
            .delete(key)
            .await
            .map_err(|e| GenericError::Other(format!("app_bundle_delete: {:?}", e)))
    }

    pub async fn log_range(
        &self,
        topic: &str,
        range: std::ops::Range<SystemTime>,
        mut callback: impl FnMut(&str, &str) -> bool,
    ) -> GenericResult<()> {
        let batch_size: u32 = 20;
        let trim_prefix = join_slices(&[PREFIX_LOG_V1, topic.as_bytes(), b"\x00"]);
        let start_prefix = join_slices(&[
            PREFIX_LOG_V1,
            topic.as_bytes(),
            b"\x00",
            make_time_str(range.start).as_bytes(),
        ]);
        let end_prefix = join_slices(&[
            PREFIX_LOG_V1,
            topic.as_bytes(),
            b"\x00",
            make_time_str(range.end).as_bytes(),
        ]);
        let mut current_prefix = start_prefix.clone();

        loop {
            let batch = self
                .raw
                .scan(current_prefix..end_prefix.clone(), batch_size)
                .await
                .map_err(|e| GenericError::Other(format!("log_range: {:?}", e)))?;
            for item in batch.iter() {
                let key: &[u8] = (&item.0).into();
                let key = match std::str::from_utf8(&key[trim_prefix.len()..]) {
                    Ok(x) => x,
                    Err(_) => continue,
                };
                let value = match std::str::from_utf8(&item.1) {
                    Ok(x) => x,
                    Err(_) => continue,
                };
                if !callback(key, value) {
                    return Ok(());
                }
            }

            if batch.len() == batch_size as usize {
                current_prefix = join_slices(&[(&batch.last().unwrap().0).into(), &[0u8]]);
            } else {
                return Ok(());
            }
        }
    }

    pub async fn log_put(&self, topic: &str, time: SystemTime, text: &str) -> GenericResult<()> {
        let key = join_slices(&[
            PREFIX_LOG_V1,
            topic.as_bytes(),
            b"\x00",
            make_time_str(time).as_bytes(),
        ]);
        self.raw
            .put(key, text)
            .await
            .map_err(|e| GenericError::Other(format!("log_put: {:?}", e)))
    }

    pub async fn log_delete_range(
        &self,
        topic: &str,
        range: std::ops::Range<SystemTime>,
    ) -> GenericResult<()> {
        let start_prefix = join_slices(&[
            PREFIX_LOG_V1,
            topic.as_bytes(),
            b"\x00",
            make_time_str(range.start).as_bytes(),
        ]);
        let end_prefix = join_slices(&[
            PREFIX_LOG_V1,
            topic.as_bytes(),
            b"\x00",
            make_time_str(range.end).as_bytes(),
        ]);
        self.raw
            .delete_range(start_prefix..end_prefix)
            .await
            .map_err(|e| GenericError::Other(format!("log_delete_range: {:?}", e)))
    }
}

fn make_time_str(time: SystemTime) -> String {
    let time = chrono::DateTime::<chrono::Utc>::from(time);
    format!("{}", time.format("%Y-%m-%dT%H:%M:%S%.6f"))
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

fn kvp_deref_key(p: &KvPair) -> &[u8] {
    (&p.0).into()
}

fn kvp_deref_value(p: &KvPair) -> &[u8] {
    &p.1
}

fn key_deref(k: &Key) -> &[u8] {
    k.into()
}

fn mk_unit(_: &Key) -> () {
    ()
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
