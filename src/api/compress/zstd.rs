use anyhow::Result;
use std::convert::TryFrom;
use v8;
use zstd::block::{compress_to_buffer, decompress_to_buffer};

use crate::api::util::{
  ensure_typed_arrays_have_distinct_backing_stores, truncate_typed_array_to_uint8array,
  v8_deref_typed_array_assuming_noalias, ArrayBufferBuilder,
};

pub fn api_compress_zstd_block_compress(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let src_data = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let dst_data = args.get(2);
  let dst_data = if dst_data.is_null_or_undefined() {
    None
  } else {
    Some(v8::Local::<v8::TypedArray>::try_from(dst_data)?)
  };

  let level = args.get(3);
  let level = if level.is_null_or_undefined() {
    0
  } else {
    level
      .int32_value(scope)
      .ok_or_else(|| anyhow::anyhow!("invalid level"))?
  };

  if let Some(dst_data) = dst_data {
    ensure_typed_arrays_have_distinct_backing_stores(scope, &[src_data, dst_data])?;
    let src_data = unsafe { v8_deref_typed_array_assuming_noalias(scope, src_data) };
    let mut dst_data_view = unsafe { v8_deref_typed_array_assuming_noalias(scope, dst_data) };
    let len = compress_to_buffer(&src_data[..], &mut dst_data_view[..], level)?;
    let out = truncate_typed_array_to_uint8array(scope, dst_data, len)?;
    retval.set(out.into());
  } else {
    let src_data = unsafe { v8_deref_typed_array_assuming_noalias(scope, src_data) };
    let mut builder =
      ArrayBufferBuilder::new(scope, zstd::zstd_safe::compress_bound(src_data.len()));
    let len = compress_to_buffer(&src_data[..], &mut builder[..], level)?;
    retval.set(builder.build_uint8array(scope, Some(len)).into());
  }
  Ok(())
}

pub fn api_compress_zstd_block_decompress(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let src_data = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let dst_data = v8::Local::<v8::TypedArray>::try_from(args.get(2))?;
  ensure_typed_arrays_have_distinct_backing_stores(scope, &[src_data, dst_data])?;
  let src_data = unsafe { v8_deref_typed_array_assuming_noalias(scope, src_data) };
  let mut dst_data_view = unsafe { v8_deref_typed_array_assuming_noalias(scope, dst_data) };
  let len = decompress_to_buffer(&src_data[..], &mut dst_data_view[..])?;
  let out = truncate_typed_array_to_uint8array(scope, dst_data, len)?;
  retval.set(out.into());
  Ok(())
}
