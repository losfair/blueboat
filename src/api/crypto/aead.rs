use aes_gcm_siv::aead::consts::{U12, U16};
use aes_gcm_siv::aead::generic_array::typenum::Unsigned;
use aes_gcm_siv::aead::generic_array::{ArrayLength, GenericArray};
use aes_gcm_siv::aead::{AeadCore, AeadMutInPlace, Buffer, NewAead};
use aes_gcm_siv::{Aes128GcmSiv, Key, Nonce};
use anyhow::Result;
use v8;

use crate::api::util::ArrayBufferBuilder;
use crate::v8util::{GenericBytesView, LocalValueExt};

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

struct AeadBuffer<'a> {
  backing: ArrayBufferBuilder<'a>,
  len: usize,
}

impl<'a> AsRef<[u8]> for AeadBuffer<'a> {
  fn as_ref(&self) -> &[u8] {
    &self.backing[..self.len]
  }
}

impl<'a> AsMut<[u8]> for AeadBuffer<'a> {
  fn as_mut(&mut self) -> &mut [u8] {
    &mut self.backing[..self.len]
  }
}

impl<'a> Buffer for AeadBuffer<'a> {
  fn extend_from_slice(&mut self, other: &[u8]) -> Result<(), aes_gcm_siv::aead::Error> {
    if self
      .len
      .checked_add(other.len())
      .map(|new_len| new_len <= self.backing.len())
      .unwrap_or(false)
    {
      self.backing[self.len..self.len + other.len()].copy_from_slice(other);
      self.len += other.len();
      Ok(())
    } else {
      Err(aes_gcm_siv::aead::Error)
    }
  }
  fn truncate(&mut self, len: usize) {
    assert!(len <= self.len);
    self.len = len;
  }
}

pub fn api_crypto_aead_aes128_gcm_siv_encrypt(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let params = AesGcmSivParams::from_args(scope, &args)?;
  let mut cipher = Aes128GcmSiv::new(&&params.key);
  let plaintext = &params.data[..];
  let mut output_buffer = AeadBuffer {
    backing: ArrayBufferBuilder::new(
      scope,
      plaintext.len() + <Aes128GcmSiv as AeadCore>::TagSize::to_usize(),
    ),
    len: plaintext.len(),
  };
  output_buffer.backing[..plaintext.len()].copy_from_slice(plaintext);
  cipher
    .encrypt_in_place(
      &params.nonce,
      params
        .associated_data
        .as_ref()
        .map(|x| &x[..])
        .unwrap_or(&[]),
      &mut output_buffer,
    )
    .map_err(|_| anyhow::anyhow!("aead_aes128_gcm_siv_encrypt failed"))?;
  assert_eq!(output_buffer.len, output_buffer.backing.len());
  retval.set(
    output_buffer
      .backing
      .build_uint8array(scope, Some(output_buffer.len))
      .into(),
  );
  Ok(())
}

pub fn api_crypto_aead_aes128_gcm_siv_decrypt(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let params = AesGcmSivParams::from_args(scope, &args)?;
  let mut cipher = Aes128GcmSiv::new(&&params.key);
  let ciphertext = &params.data[..];
  let mut output_buffer = AeadBuffer {
    backing: ArrayBufferBuilder::new(scope, ciphertext.len()),
    len: ciphertext.len(),
  };
  output_buffer.backing.copy_from_slice(ciphertext);
  cipher
    .decrypt_in_place(
      &params.nonce,
      params
        .associated_data
        .as_ref()
        .map(|x| &x[..])
        .unwrap_or(&[]),
      &mut output_buffer,
    )
    .map_err(|_| anyhow::anyhow!("aead_aes128_gcm_siv_decrypt failed"))?;
  assert_eq!(
    output_buffer.len,
    output_buffer.backing.len() - <Aes128GcmSiv as AeadCore>::TagSize::to_usize()
  );
  retval.set(
    output_buffer
      .backing
      .build_uint8array(scope, Some(output_buffer.len))
      .into(),
  );
  Ok(())
}
