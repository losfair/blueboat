use crate::{
    app::{AppConfig, AppId},
    types::*,
    util::current_millis,
};
use mysql_async::{params, prelude::Queryable, IsolationLevel, Pool, TxOpts};
use rand::Rng;
use std::time::SystemTime;
use std::{
    collections::BTreeMap,
    time::{Duration, UNIX_EPOCH},
};

pub struct DataClient {
    db: Pool,
}

impl DataClient {
    pub async fn new(db_url: &str) -> GenericResult<Self> {
        let db = Pool::from_url(db_url)
            .map_err(|e| GenericError::Other(format!("db connection failed: {:?}", e,)))?;
        Ok(Self { db })
    }

    pub async fn worker_data_get(
        &self,
        namespace_id: &str,
        key: &[u8],
    ) -> GenericResult<Option<Vec<u8>>> {
        let mut conn = self.db.get_conn().await?;
        let value: Option<Vec<u8>> = conn
            .exec_first(
                "select appvalue from appkv where nsid = ? and appkey = ? and (appexpiration = 0 or appexpiration > ?)",
                (namespace_id, key, current_millis()),
            )
            .await?;
        Ok(value)
    }

    pub async fn worker_data_put(
        &self,
        namespace_id: &str,
        key: &[u8],
        value: &[u8],
        if_not_exists: bool,
        ttl_ms: u64,
    ) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        let empty_md: &[u8] = &[];

        let expiration = if ttl_ms != 0 {
            current_millis()
                .checked_add(ttl_ms)
                .ok_or_else(|| GenericError::Other("expiration time overflow".into()))?
        } else {
            0u64
        };

        let prms = params! {
            "nsid" => namespace_id,
            "appkey" => key,
            "appvalue" => value,
            "appmetadata" => empty_md,
            "appexpiration" => expiration,
            "currenttime" => current_millis(),
        };

        if if_not_exists {
            conn.exec_drop(
                format!(
                    "{} where not exists ({})",
                    "replace into appkv (nsid, appkey, appvalue, appmetadata, appexpiration) select :nsid, :appkey, :appvalue, :appmetadata, :appexpiration",
                    "select 1 from appkv where nsid = :nsid and appkey = :appkey and (appexpiration = 0 or appexpiration > :currenttime)"
                ),
                prms,
            ).await?;
        } else {
            conn.exec_drop(
                format!(
                    "{} on duplicate key {}",
                    "insert into appkv (nsid, appkey, appvalue, appmetadata, appexpiration) values(:nsid, :appkey, :appvalue, :appmetadata, :appexpiration)",
                    "update appvalue = :appvalue, appexpiration = :appexpiration",
                ),
                prms,
            ).await?;
        }
        Ok(())
    }

    pub async fn worker_data_scan_keys(
        &self,
        namespace_id: &str,
        start: &[u8],
        end: Option<&[u8]>,
        limit: u32,
    ) -> GenericResult<Vec<Vec<u8>>> {
        let mut conn = self.db.get_conn().await?;
        let result: Vec<Vec<u8>> = if let Some(end) = end {
            conn.exec(
                "select appkey from appkv where nsid = ? and appkey between ? and ? and (appexpiration = 0 or appexpiration > ?) limit ?",
                (namespace_id, start, end, current_millis(), limit),
            )
            .await?
        } else {
            conn.exec(
                "select appkey from appkv where nsid = ? and appkey >= ? and (appexpiration = 0 or appexpiration > ?) limit ?",
                (namespace_id, start, current_millis(), limit),
            )
            .await?
        };
        Ok(result)
    }

    pub async fn worker_data_delete(&self, namespace_id: &str, key: &[u8]) -> GenericResult<()> {
        let mut conn = self.db.get_conn().await?;
        conn.exec_drop(
            "delete from appkv where nsid = ? and appkey = ?",
            (namespace_id, key),
        )
        .await?;
        Ok(())
    }

    pub async fn worker_data_cmpupdate(
        &self,
        namespace_id: &str,
        assertions: &[(Vec<u8>, Vec<u8>)],
        writes: &[(Vec<u8>, Vec<u8>)],
        ttl_ms: u64,
    ) -> GenericResult<bool> {
        let current_time = current_millis();
        let expiration = if ttl_ms != 0 {
            current_time
                .checked_add(ttl_ms)
                .ok_or_else(|| GenericError::Other("expiration time overflow".into()))?
        } else {
            0u64
        };

        let mut opts = TxOpts::new();
        opts.with_isolation_level(IsolationLevel::RepeatableRead);
        let mut txn = self.db.start_transaction(opts).await?;
        let stmt = txn
            .prep("select 1 from appkv where nsid = ? and appkey = ? and appvalue = ? and (appexpiration = 0 or appexpiration > ?) for update")
            .await?;
        for (k, v) in assertions {
            let existence: Option<u32> = txn
                .exec_first(&stmt, (namespace_id, k, v, current_time))
                .await?;
            if existence.is_none() {
                return Ok(false);
            }
        }
        let empty_md: &[u8] = &[];
        txn.exec_batch(
            format!(
                "{} on duplicate key {}",
                "insert into appkv (nsid, appkey, appvalue, appmetadata, appexpiration) values(:nsid, :appkey, :appvalue, :appmetadata, :appexpiration)",
                "update appvalue = :appvalue, appexpiration = :appexpiration",
            ),
            writes.iter().map(|(k, v)| params! {
                "nsid" => namespace_id,
                "appkey" => k,
                "appvalue" => v,
                "appmetadata" => empty_md,
                "appexpiration" => expiration,
            }).collect::<Vec<_>>(),
        ).await?;
        txn.commit().await?;
        Ok(true)
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
            "replace into routes (domain, path, appid, createtime) values(?, ?, ?, ?)",
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
