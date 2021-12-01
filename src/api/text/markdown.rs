use anyhow::Result;
use pulldown_cmark::{html, Options, Parser};
use schemars::JsonSchema;
use serde::Deserialize;
use v8;

use crate::api::util::{mk_v8_string, v8_deserialize};

#[derive(Deserialize, JsonSchema)]
pub struct TextMarkdownRenderOpts {
  #[serde(default)]
  enable_tables: bool,
  #[serde(default)]
  enable_footnotes: bool,
  #[serde(default)]
  enable_strikethrough: bool,
  #[serde(default)]
  enable_tasklists: bool,
  #[serde(default)]
  enable_smart_punctuation: bool,
  #[serde(default)]
  disable_sanitization: bool,
}

pub fn api_text_markdown_render(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let markdown_input = args.get(1).to_rust_string_lossy(scope);
  let jsopts: TextMarkdownRenderOpts = v8_deserialize(scope, args.get(2))?;
  let mut opts = Options::empty();
  if jsopts.enable_footnotes {
    opts.insert(Options::ENABLE_FOOTNOTES);
  }
  if jsopts.enable_tables {
    opts.insert(Options::ENABLE_TABLES);
  }
  if jsopts.enable_strikethrough {
    opts.insert(Options::ENABLE_STRIKETHROUGH);
  }
  if jsopts.enable_tasklists {
    opts.insert(Options::ENABLE_TASKLISTS);
  }
  if jsopts.enable_smart_punctuation {
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
  }
  let parser = Parser::new_ext(&markdown_input, opts);
  let mut html_output = String::new();
  html::push_html(&mut html_output, parser);

  if !jsopts.disable_sanitization {
    html_output = ammonia::clean(&html_output);
  }
  retval.set(mk_v8_string(scope, &html_output)?.into());
  Ok(())
}
