use crate::types::*;
use std::collections::BTreeMap;
use tikv_client::{KvPair, Key};

macro_rules! impl_scan_prefix {
    ($name:ident, $cb_value_ty:ty, $scan_func:ident, $deref_key:ident, $deref_value:ident) => {
        async fn $name(&self, prefix: &[u8], mut cb: impl FnMut(&[u8], $cb_value_ty) -> bool) -> GenericResult<()> {
            assert!(prefix.len() > 0, "scan_prefix: prefix must be non-empty");
            assert!(*prefix.last().unwrap() == 0, "scan_prefix: prefix must end with zero");
    
            let batch_size: u32 = 20;
    
            let mut start_prefix = prefix.to_vec();
            let mut end_prefix = prefix.to_vec();
            *end_prefix.last_mut().unwrap() = 1;
    
            loop {
                let batch = self.raw.$scan_func(start_prefix..end_prefix.clone(), batch_size).await
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

/// Will be used a lot so keep it short.
pub static PREFIX_WORKER_DATA_V2: &'static [u8] = b"V2\x00W\x00";

pub static PREFIX_APP_METADATA_V1: &'static [u8] = b"V1\x00APPMD\x00";

pub static PREFIX_APP_BUNDLE_V1: &'static [u8] = b"V1\x00APPBUNDLE\x00";

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

    impl_scan_prefix!(scan_prefix, &[u8], scan, kvp_deref_key, kvp_deref_value);
    impl_scan_prefix!(scan_prefix_keys, (), scan_keys, key_deref, mk_unit);

    pub async fn worker_data_for_each_key_in_namespace(
        &self,
        namespace_id: &[u8; 16],
        mut callback: impl FnMut(&[u8]) -> bool,
    ) -> GenericResult<()> {
        let prefix = join_slices(&[PREFIX_WORKER_DATA_V2, namespace_id, b"\x00"]);
        self.scan_prefix_keys(&prefix, |k, ()| callback(k)).await?;
        Ok(())
    }

    pub async fn worker_data_delete_namespace(
        &self,
        namespace_id: &[u8; 16],
    ) -> GenericResult<()> {
        let prefix = join_slices(&[PREFIX_WORKER_DATA_V2, namespace_id, b"\x00"]);
        self.delete_prefix(&prefix).await
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
        mut callback: impl FnMut(&str) -> bool,
    ) -> GenericResult<()> {
        self.scan_prefix_keys(PREFIX_APP_METADATA_V1, |k, ()| {
            let appid = std::str::from_utf8(k).unwrap_or("");
            callback(appid)
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

    pub async fn app_bundle_for_each(
        &self, 
        mut callback: impl FnMut(&[u8]) -> bool,
    ) -> GenericResult<()> {
        self.scan_prefix_keys(PREFIX_APP_BUNDLE_V1, |k, ()| {
            callback(k)
        }).await?;
        Ok(())
    }

    pub async fn app_bundle_get(&self, id: &[u8; 16]) -> GenericResult<Option<Vec<u8>>> {
        let key = join_slices(&[PREFIX_APP_BUNDLE_V1, id]);
        self.raw.get(key).await
            .map_err(|e| GenericError::Other(format!("app_bundle_get: {:?}", e)))
    }

    pub async fn app_bundle_put(&self, id: &[u8; 16], value: Vec<u8>) -> GenericResult<()> {
        let key = join_slices(&[PREFIX_APP_BUNDLE_V1, id]);
        self.raw.put(key, value).await
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
        self.raw.delete(key).await
            .map_err(|e| GenericError::Other(format!("app_bundle_delete: {:?}", e)))
    }
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