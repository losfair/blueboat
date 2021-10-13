use std::convert::{Infallible, TryFrom};

use anyhow::Result;
use bytes::Bytes;
use futures::stream::once;
use multer::Multipart;
use rusty_v8 as v8;
use serde::Serialize;

use crate::{
  api::util::{v8_deref_typed_array_assuming_noalias, v8_serialize},
  v8util::create_uint8array_from_bytes,
};

#[derive(Serialize)]
pub struct CodecMultipartData<'s> {
  name: Option<String>,
  file_name: Option<String>,
  content_type: Option<String>,
  body: serde_v8::Value<'s>,
}

pub fn api_codec_multipart_decode(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let mut res: Vec<CodecMultipartData> = vec![];
  let data = v8::Local::<v8::TypedArray>::try_from(args.get(1))?;
  let data = Bytes::copy_from_slice(&unsafe { v8_deref_typed_array_assuming_noalias(scope, data) });
  let stream = once(async move { Result::<Bytes, Infallible>::Ok(data) });
  let boundary = args.get(2).to_rust_string_lossy(scope);

  let mut multipart = Multipart::new(stream, boundary);
  futures::executor::block_on(async {
    while let Ok(Some(field)) = multipart.next_field().await {
      let name = field.name().map(|x| x.to_string());
      let file_name = field.file_name().map(|x| x.to_string());
      let content_type = field.content_type().map(|x| x.essence_str().to_string());
      let body = field.bytes().await;
      let body = body.as_ref().map(|x| &x[..]).unwrap_or(&[]);
      let body = create_uint8array_from_bytes(scope, body);
      let data = CodecMultipartData {
        name,
        file_name,
        content_type,
        body: v8::Local::<v8::Value>::from(body).into(),
      };
      res.push(data);
    }
  });
  let res = v8_serialize(scope, &res)?;
  retval.set(res);
  Ok(())
}
