use std::{
  collections::HashMap,
  convert::{TryFrom, TryInto},
  str::FromStr,
  time::Duration,
};

use rusoto_core::{
  credential::{AwsCredentials, StaticProvider},
  region::ParseRegionError,
  HttpClient, Region,
};
use rusoto_s3::{
  util::PreSignedRequest, CommonPrefix, ListObjectsV2Request, Object, Owner, S3Client, S3,
};

use anyhow::Result;
use rusty_v8 as v8;

use crate::{
  api::util::{mk_v8_string, v8_deserialize, v8_invoke_callback, v8_serialize},
  exec::Executor,
  v8util::FunctionCallbackArgumentsExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[macro_export]
macro_rules! impl_idstruct_inversed {
  ($src: ty, $dst: ident, $(pub $field:ident : $field_ty:ty,)*) => {
    #[derive(Serialize, Deserialize, JsonSchema, Clone)]
    pub struct $dst {
      $(pub $field: $field_ty,)*
    }
    impl From<$src> for $dst {
      fn from(src: $src) -> Self {
        Self {
          $($field: src.$field.into(),)*
        }
      }
    }
  }
}

#[macro_export]
macro_rules! impl_idstruct_with_spread {
  ($src: ident, $dst: ty, $(pub $field:ident : $field_ty:ty,)*) => {
    #[derive(Serialize, Deserialize, JsonSchema, Clone)]
    pub struct $src {
      $(pub $field: $field_ty,)*
    }
    impl From<$src> for $dst {
      fn from(src: $src) -> Self {
        Self {
          $($field: src.$field.into(),)*
          ..Default::default()
        }
      }
    }
  }
}

impl_idstruct_with_spread!(
  S3DeleteObjectRequest,
  rusoto_s3::DeleteObjectRequest,
  pub bucket: String,
  pub bypass_governance_retention: Option<bool>,
  pub expected_bucket_owner: Option<String>,
  pub key: String,
  pub mfa: Option<String>,
  pub request_payer: Option<String>,
  pub version_id: Option<String>,
);

impl_idstruct_with_spread!(
  S3GetObjectRequest,
  rusoto_s3::GetObjectRequest,
  pub bucket: String,
  pub expected_bucket_owner: Option<String>,
  pub if_match: Option<String>,
  pub if_modified_since: Option<String>,
  pub if_none_match: Option<String>,
  pub if_unmodified_since: Option<String>,
  pub key: String,
  pub part_number: Option<i64>,
  pub range: Option<String>,
  pub request_payer: Option<String>,
  pub response_cache_control: Option<String>,
  pub response_content_disposition: Option<String>,
  pub response_content_encoding: Option<String>,
  pub response_content_language: Option<String>,
  pub response_content_type: Option<String>,
  pub response_expires: Option<String>,
  pub sse_customer_algorithm: Option<String>,
  pub sse_customer_key: Option<String>,
  pub sse_customer_key_md5: Option<String>,
  pub version_id: Option<String>,
);

impl_idstruct_with_spread!(
  S3PutObjectRequest,
  rusoto_s3::PutObjectRequest,
  pub acl: Option<String>,
  //pub body: Option<StreamingBody>,
  pub bucket: String,
  pub bucket_key_enabled: Option<bool>,
  pub cache_control: Option<String>,
  pub content_disposition: Option<String>,
  pub content_encoding: Option<String>,
  pub content_language: Option<String>,
  pub content_length: Option<i64>,
  pub content_md5: Option<String>,
  pub content_type: Option<String>,
  pub expected_bucket_owner: Option<String>,
  pub expires: Option<String>,
  pub grant_full_control: Option<String>,
  pub grant_read: Option<String>,
  pub grant_read_acp: Option<String>,
  pub grant_write_acp: Option<String>,
  pub key: String,
  pub metadata: Option<HashMap<String, String>>,
  pub object_lock_legal_hold_status: Option<String>,
  pub object_lock_mode: Option<String>,
  pub object_lock_retain_until_date: Option<String>,
  pub request_payer: Option<String>,
  pub sse_customer_algorithm: Option<String>,
  pub sse_customer_key: Option<String>,
  pub sse_customer_key_md5: Option<String>,
  pub ssekms_encryption_context: Option<String>,
  pub ssekms_key_id: Option<String>,
  pub server_side_encryption: Option<String>,
  pub storage_class: Option<String>,
  pub tagging: Option<String>,
  pub website_redirect_location: Option<String>,
);

impl_idstruct_with_spread!(
  S3UploadPartRequest,
  rusoto_s3::UploadPartRequest,
  //pub body: Option<StreamingBody>,
  pub bucket: String,
  pub content_length: Option<i64>,
  pub content_md5: Option<String>,
  pub expected_bucket_owner: Option<String>,
  pub key: String,
  pub part_number: i64,
  pub request_payer: Option<String>,
  pub sse_customer_algorithm: Option<String>,
  pub sse_customer_key: Option<String>,
  pub sse_customer_key_md5: Option<String>,
  pub upload_id: String,
);

impl_idstruct_with_spread!(
  S3ListObjectsV2Request,
  rusoto_s3::ListObjectsV2Request,
  pub bucket: String,
  pub continuation_token: Option<String>,
  pub delimiter: Option<String>,
  pub encoding_type: Option<String>,
  pub expected_bucket_owner: Option<String>,
  pub fetch_owner: Option<bool>,
  pub max_keys: Option<i64>,
  pub prefix: Option<String>,
  pub request_payer: Option<String>,
  pub start_after: Option<String>,
);

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(transparent)]
pub struct CommonPrefixList {
  list: Option<Vec<Option<String>>>,
}

impl From<Option<Vec<CommonPrefix>>> for CommonPrefixList {
  fn from(that: Option<Vec<CommonPrefix>>) -> Self {
    Self {
      list: that.map(|x| x.into_iter().map(|x| x.prefix).collect()),
    }
  }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(transparent)]
pub struct S3ContentList {
  list: Option<Vec<S3Object>>,
}

impl_idstruct_inversed!(
  rusoto_s3::Owner, S3Owner,
  pub display_name: Option<String>,
  pub id: Option<String>,
);

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(transparent)]
pub struct S3OwnerOptional {
  owner: Option<S3Owner>,
}

impl From<Option<Owner>> for S3OwnerOptional {
  fn from(that: Option<Owner>) -> Self {
    Self {
      owner: that.map(|x| x.into()),
    }
  }
}

impl_idstruct_inversed!(
  rusoto_s3::Object, S3Object,
  pub e_tag: Option<String>,
  pub key: Option<String>,
  pub last_modified: Option<String>,
  pub owner: S3OwnerOptional,
  pub size: Option<i64>,
  pub storage_class: Option<String>,
);

impl From<Option<Vec<Object>>> for S3ContentList {
  fn from(that: Option<Vec<Object>>) -> Self {
    Self {
      list: that.map(|x| x.into_iter().map(S3Object::from).collect()),
    }
  }
}

impl_idstruct_inversed!(
  rusoto_s3::ListObjectsV2Output,
  S3ListObjectsV2Output,
  pub common_prefixes: CommonPrefixList,
  pub contents: S3ContentList,
  pub continuation_token: Option<String>,
  pub delimiter: Option<String>,
  pub encoding_type: Option<String>,
  pub is_truncated: Option<bool>,
  pub key_count: Option<i64>,
  pub max_keys: Option<i64>,
  pub name: Option<String>,
  pub next_continuation_token: Option<String>,
  pub prefix: Option<String>,
  pub start_after: Option<String>,
);

#[derive(Deserialize, JsonSchema)]
pub struct S3Region {
  name: String,
  endpoint: Option<String>,
}

impl TryFrom<S3Region> for Region {
  type Error = ParseRegionError;
  fn try_from(that: S3Region) -> Result<Self, Self::Error> {
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

#[derive(Deserialize, JsonSchema)]
pub struct S3Credentials {
  key: String,
  secret: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum S3PresignInfo {
  DeleteObject { request: S3DeleteObjectRequest },
  GetObject { request: S3GetObjectRequest },
  PutObject { request: S3PutObjectRequest },
  UploadPart { request: S3UploadPartRequest },
}

#[derive(Deserialize, JsonSchema)]
pub struct S3PresignOptions {
  expires_in_secs: u64,
}

impl From<S3PresignOptions> for rusoto_s3::util::PreSignedRequestOption {
  fn from(that: S3PresignOptions) -> Self {
    Self {
      expires_in: Duration::from_secs(that.expires_in_secs),
    }
  }
}

impl From<S3PresignInfo> for Box<dyn PreSignedRequest> {
  fn from(that: S3PresignInfo) -> Self {
    match that {
      S3PresignInfo::DeleteObject { request } => {
        Box::new(rusoto_s3::DeleteObjectRequest::from(request))
      }
      S3PresignInfo::GetObject { request } => Box::new(rusoto_s3::GetObjectRequest::from(request)),
      S3PresignInfo::PutObject { request } => Box::new(rusoto_s3::PutObjectRequest::from(request)),
      S3PresignInfo::UploadPart { request } => {
        Box::new(rusoto_s3::UploadPartRequest::from(request))
      }
    }
  }
}

fn decode_s3_common_args<'s, 't, 'u>(
  scope: &mut v8::HandleScope<'s>,
  region: v8::Local<'t, v8::Value>,
  credentials: v8::Local<'u, v8::Value>,
) -> Result<(Region, AwsCredentials)> {
  let region: S3Region = v8_deserialize(scope, region)?;
  let credentials: Option<S3Credentials> = if credentials.is_null_or_undefined() {
    None
  } else {
    Some(v8_deserialize(scope, credentials)?)
  };
  let credentials = credentials
    .map(|x| AwsCredentials::new(x.key, x.secret, None, None))
    .unwrap_or_default();
  Ok((region.try_into()?, credentials))
}

pub fn api_external_s3_sign(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let (region, credentials) = decode_s3_common_args(scope, args.get(1), args.get(2))?;
  let presign_info: S3PresignInfo = v8_deserialize(scope, args.get(3))?;
  let options: S3PresignOptions = v8_deserialize(scope, args.get(4))?;
  let req = Box::<dyn PreSignedRequest>::from(presign_info);
  let url = req.get_presigned_url(&region.try_into()?, &credentials, &options.into());
  retval.set(mk_v8_string(scope, &url)?.into());
  Ok(())
}

pub fn api_external_s3_list_objects_v2(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let (region, credentials) = decode_s3_common_args(scope, args.get(1), args.get(2))?;
  let req: S3ListObjectsV2Request = v8_deserialize(scope, args.get(3))?;
  let callback = v8::Global::new(scope, args.load_function_at(4)?);
  let client = S3Client::new_with(
    HttpClient::new()?,
    StaticProvider::from(credentials),
    region,
  );
  let e = Executor::try_current_result()?;
  let e2 = e.clone();
  Executor::spawn(&e, async move {
    let res = client
      .list_objects_v2(ListObjectsV2Request::from(req))
      .await
      .map_err(anyhow::Error::from);
    Executor::enter(&e2, |scope| {
      let res = res.and_then(|x| v8_serialize(scope, &S3ListObjectsV2Output::from(x)));
      v8_invoke_callback("external_s3_list_objects_v2", scope, res, &callback);
    });
  });
  Ok(())
}
