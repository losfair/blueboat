use std::collections::HashSet;

use anyhow::Result;
use jsonwebtoken::{DecodingKey, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use v8;

use crate::{
  api::util::{mk_v8_string, v8_deserialize, v8_serialize},
  v8util::LocalValueExt,
};

#[derive(Deserialize)]
struct KeyInfo {
  #[serde(rename = "type")]
  _type: KeyType,
  data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum KeyType {
  Base64Secret,
  RsaPem,
  EcPem,
}

#[derive(Deserialize)]
struct Validation {
  pub leeway: u64,
  pub validate_exp: Option<bool>,
  pub validate_nbf: Option<bool>,
  pub aud: Option<HashSet<String>>,
  pub iss: Option<String>,
  pub sub: Option<String>,
  pub algorithms: Vec<jsonwebtoken::Algorithm>,
}

#[derive(Serialize)]
struct TokenData {
  header: Header,
  claims: serde_json::Value,
}

impl From<jsonwebtoken::TokenData<serde_json::Value>> for TokenData {
  fn from(that: jsonwebtoken::TokenData<serde_json::Value>) -> Self {
    Self {
      header: that.header,
      claims: that.claims,
    }
  }
}

impl From<Validation> for jsonwebtoken::Validation {
  fn from(that: Validation) -> Self {
    Self {
      leeway: that.leeway,
      validate_exp: that.validate_exp.unwrap_or(true),
      validate_nbf: that.validate_nbf.unwrap_or(false),
      aud: that.aud,
      iss: that.iss,
      sub: that.sub,
      algorithms: that.algorithms,
    }
  }
}

pub fn api_crypto_jwt_encode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let header: Header = v8_deserialize(scope, args.get(1))?;
  let claims: serde_json::Value = v8_deserialize(scope, args.get(2))?;
  let key: KeyInfo = v8_deserialize(scope, args.get(3))?;
  let key = match key._type {
    KeyType::Base64Secret => EncodingKey::from_base64_secret(&key.data)?,
    KeyType::RsaPem => EncodingKey::from_rsa_pem(key.data.as_bytes())?,
    KeyType::EcPem => EncodingKey::from_ec_pem(key.data.as_bytes())?,
  };
  let token = jsonwebtoken::encode(&header, &claims, &key)?;
  retval.set(mk_v8_string(scope, &token)?.into());
  Ok(())
}

pub fn api_crypto_jwt_decode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let token = unsafe { args.get(1).read_string_assume_noalias(scope)? };
  let key: KeyInfo = v8_deserialize(scope, args.get(2))?;
  let validation: Validation = v8_deserialize(scope, args.get(3))?;
  let key = match key._type {
    KeyType::Base64Secret => DecodingKey::from_base64_secret(&key.data)?,
    KeyType::RsaPem => DecodingKey::from_rsa_pem(key.data.as_bytes())?,
    KeyType::EcPem => DecodingKey::from_ec_pem(key.data.as_bytes())?,
  };
  let validation = jsonwebtoken::Validation::from(validation);
  let token_info = jsonwebtoken::decode::<serde_json::Value>(&token, &key, &validation)?;
  retval.set(v8_serialize(scope, &TokenData::from(token_info))?);
  Ok(())
}
