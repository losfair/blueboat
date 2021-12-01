use anyhow::Result;
use curve25519_dalek::{edwards::CompressedEdwardsY, montgomery::MontgomeryPoint};
use std::convert::TryFrom;
use thiserror::Error;
use v8;

use crate::{
  api::util::v8_deref_typed_array_assuming_noalias, v8util::create_uint8array_from_bytes,
};

#[derive(Error, Debug)]
#[error("invalid curve25519 parameters")]
struct InvalidParam;

pub fn api_crypto_x25519_diffie_hellman(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  use x25519_dalek::{PublicKey, StaticSecret};
  let our_secret = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let their_public = v8::Local::<v8::TypedArray>::try_from(args.get(2))?;

  let our_secret = StaticSecret::from(<[u8; 32]>::try_from(
    &unsafe { v8_deref_typed_array_assuming_noalias(scope, our_secret) }[..],
  )?);
  let their_public = PublicKey::from(<[u8; 32]>::try_from(
    &unsafe { v8_deref_typed_array_assuming_noalias(scope, their_public) }[..],
  )?);
  let shared_secret = our_secret.diffie_hellman(&their_public);
  let shared_secret = shared_secret.as_bytes();

  let out = create_uint8array_from_bytes(scope, shared_secret);
  retval.set(out.into());
  Ok(())
}

pub fn api_crypto_ed25519_derive_public(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  use ed25519_dalek::{PublicKey, SecretKey};

  let secret = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let secret =
    SecretKey::from_bytes(&unsafe { v8_deref_typed_array_assuming_noalias(scope, secret) }[..])?;
  let public = PublicKey::from(&secret);
  let res = create_uint8array_from_bytes(scope, public.as_bytes());
  retval.set(res.into());
  Ok(())
}

pub fn api_crypto_x25519_derive_public(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  use x25519_dalek::{PublicKey, StaticSecret};

  let secret = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let secret = StaticSecret::from(<[u8; 32]>::try_from(
    &unsafe { v8_deref_typed_array_assuming_noalias(scope, secret) }[..],
  )?);
  let public = PublicKey::from(&secret);
  let res = create_uint8array_from_bytes(scope, public.as_bytes());
  retval.set(res.into());
  Ok(())
}

pub fn api_crypto_ed25519_sign(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};

  let public = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let public =
    PublicKey::from_bytes(&unsafe { v8_deref_typed_array_assuming_noalias(scope, public) }[..])?;

  let secret = v8::Local::<v8::TypedArray>::try_from(args.get(2))?;
  let secret =
    SecretKey::from_bytes(&unsafe { v8_deref_typed_array_assuming_noalias(scope, secret) }[..])?;

  let message = v8::Local::<v8::TypedArray>::try_from(args.get(3))?;
  let keypair = Keypair { public, secret };
  let sig = keypair.sign(&unsafe { v8_deref_typed_array_assuming_noalias(scope, message) }[..]);
  let res = create_uint8array_from_bytes(scope, sig.as_ref());
  retval.set(res.into());
  Ok(())
}

pub fn api_crypto_ed25519_verify(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  use ed25519_dalek::{PublicKey, Signature, Verifier};

  let public = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let public = match PublicKey::from_bytes(
    &unsafe { v8_deref_typed_array_assuming_noalias(scope, public) }[..],
  ) {
    Ok(x) => x,
    Err(_) => {
      retval.set(v8::Boolean::new(scope, false).into());
      return Ok(());
    }
  };

  let sig = v8::Local::<v8::TypedArray>::try_from(args.get(2))?;
  let sig =
    match Signature::try_from(&unsafe { v8_deref_typed_array_assuming_noalias(scope, sig) }[..]) {
      Ok(x) => x,
      Err(_) => {
        retval.set(v8::Boolean::new(scope, false).into());
        return Ok(());
      }
    };

  let message = v8::Local::<v8::TypedArray>::try_from(args.get(3))?;
  let message = unsafe { v8_deref_typed_array_assuming_noalias(scope, message) };

  let strict = args.get(4).boolean_value(scope);

  let is_authentic = if strict {
    public.verify_strict(&message[..], &sig).is_ok()
  } else {
    public.verify(&message[..], &sig).is_ok()
  };
  retval.set(v8::Boolean::new(scope, is_authentic).into());

  Ok(())
}

pub fn api_crypto_ed25519_pubkey_to_x25519(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let input = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let input =
    <[u8; 32]>::try_from(&unsafe { v8_deref_typed_array_assuming_noalias(scope, input) }[..])?;
  let point = CompressedEdwardsY::from_slice(&input)
    .decompress()
    .ok_or(InvalidParam)?
    .to_montgomery();
  let out = create_uint8array_from_bytes(scope, point.as_bytes());
  retval.set(out.into());
  Ok(())
}

pub fn api_crypto_x25519_pubkey_to_ed25519(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let input = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let input =
    <[u8; 32]>::try_from(&unsafe { v8_deref_typed_array_assuming_noalias(scope, input) }[..])?;
  let point = MontgomeryPoint(input)
    .to_edwards(0)
    .ok_or(InvalidParam)?
    .compress();
  let out = create_uint8array_from_bytes(scope, point.as_bytes());
  retval.set(out.into());
  Ok(())
}
