use anyhow::Result;
use bytes::Bytes;
use rusty_v8 as v8;
use std::convert::TryFrom;

use crate::{
  api::util::{v8_error, v8_serialize},
  exec::Executor,
  ipc::{BlueboatRequest, BlueboatResponse},
  v8util::{create_arraybuffer_from_bytes, FunctionCallbackArgumentsExt},
};

use super::util::v8_deserialize;

pub fn api_fetch(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let mut req: BlueboatRequest = v8_deserialize(scope, args.get(1))?;

  let body = args.get(2);
  if !body.is_undefined() {
    if let Ok(body) = v8::Local::<v8::Uint8Array>::try_from(body) {
      let mut buf = vec![0u8; body.byte_length()];
      body.copy_contents(&mut buf);
      req.body = buf;
    }
  }

  let req = req.into_reqwest()?;

  let callback = v8::Global::new(scope, args.load_function_at(3)?);
  let exec = Executor::try_current_result()?;
  let ctx = exec.upgrade().unwrap().ctx;
  Executor::spawn(&exec.clone(), async move {
    let res = ctx.http_client.execute(req).await;
    let (res, body) = match res {
      Ok(x) => {
        let x = BlueboatResponse::from_reqwest(x).await;
        match x {
          Ok((x, body)) => (Ok(x), body),
          Err(e) => (Err(e), Bytes::new()),
        }
      }
      Err(e) => (Err(anyhow::Error::from(e)), Bytes::new()),
    };

    Executor::enter(&exec, |scope| {
      let body = create_arraybuffer_from_bytes(scope, &body);
      let callback = v8::Local::new(scope, &callback);
      let res = res.and_then(|x| v8_serialize(scope, &x));
      let undef = v8::undefined(scope);
      match res {
        Ok(x) => {
          callback.call(scope, undef.into(), &[undef.into(), x, body.into()]);
        }
        Err(e) => {
          let e = v8_error("fetch", scope, &e);
          callback.call(scope, undef.into(), &[e, undef.into(), undef.into()]);
        }
      }
    });
  });

  Ok(())
}
