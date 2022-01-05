use aes_gcm_siv::aead::consts::{U12, U16};
use aes_gcm_siv::aead::generic_array::{ArrayLength, GenericArray};
use aes_gcm_siv::aead::{Aead, NewAead, Payload};
use aes_gcm_siv::{Aes128GcmSiv, Key, Nonce};
use anyhow::Result;
use serde::Deserialize;
use v8;

use crate::api::util::{v8_deref_typed_array_assuming_noalias, v8_deserialize, TypedArrayView};
use crate::v8util::create_uint8array_from_bytes;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AesGcmSivParams<K, N, Buf> {
  key: K,
  nonce: N,
  data: Buf,
  associated_data: Option<Buf>,
}

trait TryFromSlice: Sized {
  fn try_from_slice(slice: &[u8]) -> Result<Self>;
}

impl<N: ArrayLength<u8>> TryFromSlice for GenericArray<u8, N> {
  fn try_from_slice(slice: &[u8]) -> Result<Self> {
    if slice.len() != N::to_usize() {
      return Err(anyhow::anyhow!("invalid slice length"));
    }
    Ok(Self::clone_from_slice(slice))
  }
}

impl<'a, 'b, 'c> AesGcmSivParams<serde_v8::Value<'a>, serde_v8::Value<'b>, serde_v8::Value<'c>> {
  fn interpret<'t>(
    &self,
    scope: &mut v8::HandleScope<'t>,
  ) -> Result<AesGcmSivParams<GenericArray<u8, U16>, GenericArray<u8, U12>, TypedArrayView>> {
    let key: v8::Local<v8::TypedArray> = v8::Local::<v8::TypedArray>::try_from(self.key.v8_value)?;
    let key =
      Key::try_from_slice(&unsafe { v8_deref_typed_array_assuming_noalias(scope, key) }[..])?;
    let nonce: v8::Local<v8::TypedArray> =
      v8::Local::<v8::TypedArray>::try_from(self.nonce.v8_value)?;
    let nonce =
      Nonce::try_from_slice(&unsafe { v8_deref_typed_array_assuming_noalias(scope, nonce) }[..])?;

    // XXX: `data` and `associated_data` may be aliased - though we are using them immutably here, can something go wrong?
    let data: v8::Local<v8::TypedArray> =
      v8::Local::<v8::TypedArray>::try_from(self.data.v8_value)?;
    let data = unsafe { v8_deref_typed_array_assuming_noalias(scope, data) };
    let associated_data = self
      .associated_data
      .as_ref()
      .map(|v| {
        let associated_data: v8::Local<v8::TypedArray> =
          v8::Local::<v8::TypedArray>::try_from(v.v8_value).map_err(anyhow::Error::from)?;
        Ok::<_, anyhow::Error>(unsafe {
          v8_deref_typed_array_assuming_noalias(scope, associated_data)
        })
      })
      .transpose()?;
    Ok(AesGcmSivParams {
      key,
      nonce,
      data,
      associated_data,
    })
  }
}

pub fn api_crypto_aead_aes128_gcm_siv_encrypt(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let params: AesGcmSivParams<serde_v8::Value, serde_v8::Value, serde_v8::Value> =
    v8_deserialize(scope, args.get(1))?;
  let params = params.interpret(scope)?;
  let cipher = Aes128GcmSiv::new(&&params.key);
  let ciphertext = cipher
    .encrypt(
      &params.nonce,
      Payload {
        msg: &params.data[..],
        aad: params
          .associated_data
          .as_ref()
          .map(|x| &x[..])
          .unwrap_or(&[]),
      },
    )
    .map_err(|_| anyhow::anyhow!("aead_aes128_gcm_siv_encrypt failed"))?;
  retval.set(create_uint8array_from_bytes(scope, &ciphertext).into());
  Ok(())
}

pub fn api_crypto_aead_aes128_gcm_siv_decrypt(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let params: AesGcmSivParams<serde_v8::Value, serde_v8::Value, serde_v8::Value> =
    v8_deserialize(scope, args.get(1))?;
  let params = params.interpret(scope)?;
  let cipher = Aes128GcmSiv::new(&&params.key);
  let plaintext = cipher
    .decrypt(
      &params.nonce,
      Payload {
        msg: &params.data[..],
        aad: params
          .associated_data
          .as_ref()
          .map(|x| &x[..])
          .unwrap_or(&[]),
      },
    )
    .map_err(|_| anyhow::anyhow!("aead_aes128_gcm_siv_decrypt failed"))?;
  retval.set(create_uint8array_from_bytes(scope, &plaintext).into());
  Ok(())
}
