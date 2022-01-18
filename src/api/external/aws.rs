use std::{
  collections::{BTreeMap, HashMap},
  convert::TryFrom,
  str::FromStr,
  time::Duration,
};

use rusoto_core::{credential::AwsCredentials, region::ParseRegionError, ByteStream, Region};

use anyhow::Result;
use rusoto_signature::SignedRequest;
use v8;

use crate::{
  api::util::{mk_v8_string, v8_deserialize, v8_serialize},
  v8util::LocalValueExt,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct JsAwsCredentials {
  key: String,
  secret: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsSignaturePayload<'a> {
  method: String,
  service: String,
  region: AwsRegion,
  path: String,
  headers: HashMap<String, Vec<String>>,

  /// Only needed for presigned URL
  #[serde(default)]
  expires_in_millis: u64,

  #[serde(default)]
  presigned_url: bool,

  body: Option<serde_v8::Value<'a>>,
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
  for (k, arr) in &payload.headers {
    for v in arr {
      req.add_header(k, v);
    }
  }

  if payload.presigned_url {
    let url = req.generate_presigned_url(
      &creds,
      &Duration::from_millis(payload.expires_in_millis),
      false,
    );
    retval.set(mk_v8_string(scope, &url)?.into());
  } else {
    if let Some(body) = &payload.body {
      let body = unsafe { body.v8_value.read_bytes_assume_noalias(scope)? };
      req.set_payload(Some(body.to_vec()));
    } else {
      // Get rid of Content-Length header and payload signing
      let fake_stream = ByteStream::new(futures::stream::empty());
      req.set_payload_stream(fake_stream);
    }

    req.sign(&creds);

    // https://docs.rs/rusoto_signature/0.47.0/src/rusoto_signature/signature.rs.html#514
    let mut final_url = format!(
      "{}://{}{}",
      req.scheme(),
      req.hostname(),
      req.canonical_path()
    );
    if !req.canonical_query_string().is_empty() {
      final_url = final_url + &format!("?{}", req.canonical_query_string());
    }
    let headers = req
      .headers
      .into_iter()
      .map(|(k, v)| {
        (
          k,
          v.into_iter()
            .map(|x| {
              String::from_utf8(x)
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
            })
            .collect::<Vec<_>>(),
        )
      })
      .collect::<BTreeMap<_, _>>();
    let js_obj = v8_serialize(
      scope,
      &serde_json::json!({
        "headers": headers,
        "url": final_url,
      }),
    )?;
    retval.set(js_obj);
  }
  Ok(())
}
