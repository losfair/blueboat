use crate::buffer::*;
use crate::error::*;
use crate::mm::*;
use rusty_v8 as v8;
use serde::{Deserialize, Serialize};
use std::cell::Cell;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CryptoCall {
    Digest(DigestAlgorithm),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DigestAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl CryptoCall {
    pub fn run<'s>(
        self,
        scope: &mut v8::HandleScope<'s>,
        buffers: Vec<JsArrayBufferViewRef>,
    ) -> JsResult<Option<v8::Local<'s, v8::Value>>> {
        let mut buffers = buffers.into_iter();

        match self {
            CryptoCall::Digest(alg) => {
                let buf = buffers
                    .next()
                    .ok_or_else(|| JsError::error("crypto: missing buffer"))?;
                let alg: &'static ring::digest::Algorithm = match alg {
                    DigestAlgorithm::Sha1 => &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
                    DigestAlgorithm::Sha256 => &ring::digest::SHA256,
                    DigestAlgorithm::Sha384 => &ring::digest::SHA384,
                    DigestAlgorithm::Sha512 => &ring::digest::SHA512,
                };
                let bytes: &[Cell<u8>] = &*buf;
                let mut ctx = ring::digest::Context::new(alg);
                for b in bytes {
                    ctx.update(&[b.get()]);
                }
                let output = ctx.finish();
                let output: &[u8] = output.as_ref();
                let output = slice_to_arraybuffer(scope, output)?;
                Ok(Some(output.into()))
            }
        }
    }
}
