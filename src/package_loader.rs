use std::time::Duration;

use crate::{
  metadata::Metadata,
  package::PackageKey,
  server::{cache, s3},
};
use anyhow::Result;
use rusoto_s3::{GetObjectRequest, S3};
use tokio::io::AsyncReadExt;

const PACKAGE_TTL: Duration = Duration::from_secs(86400 * 30);

pub async fn load_package(pk: &PackageKey, md: &Metadata) -> Result<Vec<u8>> {
  let cache = cache();
  let value = cache.get(&pk.version)?;
  if let Some(x) = value {
    return Ok(x.data);
  }

  let (upd, value) = cache.get_for_update(&pk.version).await?;
  if let Some(x) = value {
    return Ok(x.data);
  }

  let new_data = fetch_package(md)
    .await
    .map_err(|e| e.context("failed to fetch package"))?;
  upd.write(&new_data, PACKAGE_TTL)?;
  Ok(new_data)
}

async fn fetch_package(md: &Metadata) -> Result<Vec<u8>> {
  let (s3c, bucket) = s3();
  let output = s3c
    .get_object(GetObjectRequest {
      bucket: bucket.clone(),
      key: md.package.clone(),
      ..Default::default()
    })
    .await?;
  let mut body: Vec<u8> = vec![];
  output
    .body
    .ok_or_else(|| anyhow::anyhow!("missing package for this metadata"))?
    .into_async_read()
    .read_to_end(&mut body)
    .await?;
  Ok(body)
}
