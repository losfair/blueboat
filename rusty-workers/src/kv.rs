use crate::types::*;
use std::collections::BTreeMap;

/// Will be used a lot so keep it short.
pub static PREFIX_WORKER_DATA_V1: &'static [u8] = b"V1\x00W\x00";

pub static PREFIX_APP_METADATA_V1: &'static [u8] = b"V1\x00APPMD\x00";

pub static PREFIX_ROUTE_MAPPING_V1: &'static [u8] = b"V1\x00ROUTEMAP\x00";

pub struct KvClient {
    raw: tikv_client::RawClient,
}

impl KvClient {
    pub async fn new<S: Into<String>>(pd_endpoints: Vec<S>) -> GenericResult<Self> {
        let raw = tikv_client::RawClient::new(pd_endpoints).await
            .map_err(|e| GenericError::Other(format!("tikv initialization failed: {:?}", e)))?;
        Ok(Self {
            raw,
        })
    }

    async fn delete_prefix(&self, prefix: &[u8]) -> GenericResult<()> {
        assert!(prefix.len() > 0, "delete_prefix: prefix must be non-empty");
        assert!(*prefix.last().unwrap() == 0, "delete_prefix: prefix must end with zero");
        let start_prefix = prefix.to_vec();
        let mut end_prefix = prefix.to_vec();
        *end_prefix.last_mut().unwrap() = 1;

        self.raw.delete_range(start_prefix..end_prefix).await
            .map_err(|e| GenericError::Other(format!("delete_prefix: {:?}", e)))
    }

    async fn scan_prefix(&self, prefix: &[u8], mut cb: impl FnMut(&[u8], &[u8]) -> bool) -> GenericResult<()> {
        assert!(prefix.len() > 0, "scan_prefix: prefix must be non-empty");
        assert!(*prefix.last().unwrap() == 0, "scan_prefix: prefix must end with zero");

        let batch_size: u32 = 20;

        let mut start_prefix = prefix.to_vec();
        let mut end_prefix = prefix.to_vec();
        *end_prefix.last_mut().unwrap() = 1;

        loop {
            let batch = self.raw.scan(start_prefix..end_prefix.clone(), batch_size).await
                .map_err(|e| GenericError::Other(format!("scan_prefix: {:?}", e)))?;
            for item in batch.iter() {
                let key: &[u8] = (&item.0).into();
                if key.starts_with(prefix) {
                    if !cb(&key[prefix.len()..], &item.1) {
                        return Ok(());
                    }
                } else {
                    error!("scan_prefix: invalid data from database");
                    return Ok(());
                }
            }
            if batch.len() == batch_size as usize {
                // The immediate next key
                start_prefix = batch.into_iter().rev().next().unwrap().0.into();
                start_prefix.extend_from_slice(&[0u8]);
            } else {
                return Ok(());
            }
        }
    }

    pub async fn worker_data_get(&self, namespace_id: &[u8; 16], key: &[u8]) -> GenericResult<Option<Vec<u8>>> {
        self.raw.get(make_worker_data_key(namespace_id, key)).await
            .map_err(|e| GenericError::Other(format!("worker_data_get: {:?}", e)))
    }

    pub async fn worker_data_put(&self, namespace_id: &[u8; 16], key: &[u8], value: Vec<u8>) -> GenericResult<()> {
        self.raw.put(make_worker_data_key(namespace_id, key), value).await
            .map_err(|e| GenericError::Other(format!("worker_data_put: {:?}", e)))
    }

    pub async fn worker_data_delete(&self, namespace_id: &[u8; 16], key: &[u8]) -> GenericResult<()> {
        self.raw.delete(make_worker_data_key(namespace_id, key)).await
            .map_err(|e| GenericError::Other(format!("worker_data_delete: {:?}", e)))
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
                from_utf8(v).unwrap_or("")
            )
        }).await?;
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
                result.insert(String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned());
            }
            true
        }).await?;
        Ok(result)
    }

    pub async fn route_mapping_lookup(&self, domain: &str, path: &str) -> GenericResult<Option<String>> {
        let matches = self.route_mapping_list_for_domain(domain, |x| path.as_bytes().starts_with(x)).await?;

        // Most specific match
        Ok(matches.into_iter().rev().next().map(|x| x.1))
    }

    pub async fn route_mapping_insert(&self, domain: &str, path: &str, appid: String) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_ROUTE_MAPPING_V1, domain.as_bytes(), b"\x00", path.as_bytes()]);
        self.raw.put(key, Vec::from(appid)).await
            .map_err(|e| GenericError::Other(format!("route_mapping_insert: {:?}", e)))
    }

    pub async fn route_mapping_delete(&self, domain: &str, path: &str) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_ROUTE_MAPPING_V1, domain.as_bytes(), b"\x00", path.as_bytes()]);
        self.raw.delete(key).await
            .map_err(|e| GenericError::Other(format!("route_mapping_delete: {:?}", e)))
    }

    pub async fn app_metadata_for_each(
        &self,
        mut callback: impl FnMut(&str, &[u8]) -> bool,
    ) -> GenericResult<()> {
        self.scan_prefix(PREFIX_APP_METADATA_V1, |k, v| {
            let appid = std::str::from_utf8(k).unwrap_or("");
            callback(appid, v)
        }).await?;
        Ok(())
    }

    pub async fn app_metadata_get(&self, appid: &str) -> GenericResult<Option<Vec<u8>>> {
        let key = join_slices(&[PREFIX_APP_METADATA_V1, appid.as_bytes()]);
        self.raw.get(key).await
            .map_err(|e| GenericError::Other(format!("app_metadata_get: {:?}", e)))
    }

    pub async fn app_metadata_put(&self, appid: &str, value: Vec<u8>) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_METADATA_V1, appid.as_bytes()]);
        self.raw.put(key, value).await
            .map_err(|e| GenericError::Other(format!("app_metadata_put: {:?}", e)))
    }

    pub async fn app_metadata_delete(&self, appid: &str) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_METADATA_V1, appid.as_bytes()]);
        self.raw.delete(key).await
            .map_err(|e| GenericError::Other(format!("app_metadata_delete: {:?}", e)))
    }
}

fn make_worker_data_key(namespace_id: &[u8; 16], key: &[u8]) -> Vec<u8> {
    join_slices(&[PREFIX_WORKER_DATA_V1, namespace_id, key])
}

fn join_slices(slices: &[&[u8]]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(slices.iter().map(|x| x.len()).sum());
    for s in slices {
        buf.extend_from_slice(s);
    }
    buf
}
