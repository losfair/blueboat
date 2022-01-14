use aes_gcm_siv::aead::consts::{U12, U16};
use aes_gcm_siv::aead::generic_array::{ArrayLength, GenericArray};
use aes_gcm_siv::aead::{Aead, NewAead, Payload};
use aes_gcm_siv::{Aes128GcmSiv, Key, Nonce};
use anyhow::Result;
use v8;

use crate::v8util::{create_uint8array_from_bytes, GenericBytesView, LocalValueExt};

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

impl<'a> AesGcmSivParams<GenericArray<u8, U16>, GenericArray<u8, U12>, GenericBytesView> {
  fn from_args<'b>(
    scope: &mut v8::HandleScope<'a>,
    args: &v8::FunctionCallbackArguments<'b>,
  ) -> Result<Self> {
    let key = Key::try_from_slice(&unsafe { args.get(1).read_bytes_assume_noalias(scope)? }[..])?;
    let nonce =
      Nonce::try_from_slice(&unsafe { args.get(2).read_bytes_assume_noalias(scope)? }[..])?;

    // XXX: `data` and `associated_data` may be aliased - though we are using them immutably here, can something go wrong?
    let data = unsafe { args.get(3).read_bytes_assume_noalias(scope) }?;
    let associated_data = args.get(4);
    let associated_data = if associated_data.is_null_or_undefined() {
      None
    } else {
      Some(unsafe { associated_data.read_bytes_assume_noalias(scope) }?)
    };
    Ok(Self {
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
  let params = AesGcmSivParams::from_args(scope, &args)?;
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
  let params = AesGcmSivParams::from_args(scope, &args)?;
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
