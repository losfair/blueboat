use std::{collections::HashMap, convert::TryFrom, str::FromStr, time::Duration};

use rusoto_core::{credential::AwsCredentials, region::ParseRegionError, Region};

use anyhow::Result;
use rusoto_signature::SignedRequest;
use v8;

use crate::api::util::{mk_v8_string, v8_deserialize};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct JsAwsCredentials {
  key: String,
  secret: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsSignaturePayload {
  method: String,
  service: String,
  region: AwsRegion,
  path: String,
  headers: HashMap<String, String>,
  expires_in_millis: u64,
}

#[derive(Deserialize)]
pub struct AwsRegion {
  name: String,
  endpoint: Option<String>,
}

impl TryFrom<AwsRegion> for Region {
  type Error = ParseRegionError;
  fn try_from(that: AwsRegion) -> Result<Self, Self::Error> {
    if let Some(endpoint) = that.endpoint {
      Ok(Region::Custom {
        name: that.name,
        endpoint,
      })
    } else {
      Region::from_str(&that.name)
    }
  }
}

pub fn api_external_aws_sign(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let creds = args.get(1);
  let creds: Option<JsAwsCredentials> = if creds.is_null_or_undefined() {
    None
  } else {
    Some(v8_deserialize(scope, creds)?)
  };
  let creds = creds
    .map(|x| AwsCredentials::new(x.key, x.secret, None, None))
    .unwrap_or_default();
  let payload: AwsSignaturePayload = v8_deserialize(scope, args.get(2))?;
  let region = Region::try_from(payload.region)?;
  let mut req = SignedRequest::new(&payload.method, &payload.service, &region, &payload.path);
  for (k, v) in &payload.headers {
    req.add_header(k, v);
  }
  let url = req.generate_presigned_url(
    &creds,
    &Duration::from_millis(payload.expires_in_millis),
    false,
  );
  retval.set(mk_v8_string(scope, &url)?.into());
  Ok(())
}
