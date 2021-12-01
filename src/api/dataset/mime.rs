use anyhow::Result;
use mime_guess::MimeGuess;
use v8;

use crate::api::util::mk_v8_string;

pub fn api_dataset_mime_guess_by_ext(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let ext = args.get(1).to_rust_string_lossy(scope);
  let guess = MimeGuess::from_ext(&ext)
    .first()
    .map(|x| mk_v8_string(scope, x.essence_str()))
    .transpose()?;
  if let Some(guess) = guess {
    retval.set(guess.into());
  }
  Ok(())
}
