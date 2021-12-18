use std::mem::ManuallyDrop;

use schemars::{schema_for, JsonSchema};
use structopt::StructOpt;
use v8;

use crate::{
  api::{
    apns::{ApnsRequest, ApnsResponse},
    codec::CodecBase64Mode,
    external::s3::{
      S3Credentials, S3DeleteObjectRequest, S3GetObjectRequest, S3ListObjectsV2Output,
      S3ListObjectsV2Request, S3PresignInfo, S3PresignOptions, S3PutObjectRequest, S3Region,
      S3UploadPartRequest,
    },
    graphics::{
      codec::CanvasEncodeConfig,
      draw::CanvasDrawConfig,
      svg::CanvasRenderSvgConfig,
      text::{GraphicsTextMeasureOutput, GraphicsTextMeasureSettings},
      CanvasConfig, CanvasOp,
    },
    text::markdown::TextMarkdownRenderOpts,
  },
  bootstrap::BlueboatBootstrapData,
  ctx::{native_invoke_entry, NI_ENTRY_KEY},
  ipc::{BlueboatRequest, BlueboatResponse},
  v8util::ObjectExt,
};

#[derive(Debug, StructOpt)]
#[structopt(name = "mkimage", about = "Snapshot builder for blueboat jsland.")]
struct Opt {
  #[structopt(short, long)]
  input: String,

  #[structopt(short, long)]
  output: String,
}

pub fn main() {
  if std::env::var("MKIMAGE_PRINT_SCHEMA").is_ok() {
    print_schema();
    return;
  }
  let dry = std::env::var("MKIMAGE_DRY_RUN").is_ok();
  tracing_subscriber::fmt().init();
  let opt = Opt::from_args();

  let platform = v8::new_default_platform(0, false).make_shared();
  v8::V8::initialize_platform(platform);
  v8::V8::initialize();

  let (mut sc, mut isolate) = if !dry {
    let mut sc = ManuallyDrop::new(v8::SnapshotCreator::new(None));

    // Not actually owned!
    // https://github.com/denoland/deno/blob/f9d29115a0164a861c99b36a0919324920225e42/core/runtime.rs#L162
    let isolate = ManuallyDrop::new(unsafe { sc.get_owned_isolate() });
    (Some(sc), isolate)
  } else {
    let isolate = ManuallyDrop::new(v8::Isolate::new(v8::CreateParams::default()));
    (None, isolate)
  };

  {
    let mut isolate_scope = v8::HandleScope::new(&mut *isolate);
    let context = v8::Context::new(&mut isolate_scope);

    if let Some(sc) = &mut sc {
      sc.set_default_context(context);
    }

    let context_scope = &mut v8::ContextScope::new(&mut isolate_scope, context);
    let global = context.global(context_scope);

    let generic_ni_entry = v8::Function::new(context_scope, native_invoke_entry).unwrap();
    global.set_ext(context_scope, NI_ENTRY_KEY, generic_ni_entry.into());

    let mut catch = v8::TryCatch::new(context_scope);
    let mut build_ok = false;
    {
      let catch = &mut catch;
      let undef = v8::undefined(catch);
      let jsland = std::fs::read_to_string(&opt.input).unwrap();
      let jsland = v8::String::new(catch, &jsland).unwrap();
      let jsland_name = v8::String::new(catch, "jsland.js").unwrap();
      let jsland_origin = v8::ScriptOrigin::new(
        catch,
        jsland_name.into(),
        0,
        0,
        false,
        1,
        undef.into(),
        false,
        false,
        false,
      );
      let jsland = v8::Script::compile(catch, jsland, Some(&jsland_origin))
        .expect("jsland compilation failed");
      if jsland.run(catch).is_some() {
        build_ok = true;
      }
    }
    if let Some(exc) = catch.exception() {
      let stack = catch
        .stack_trace()
        .map(|x| x.to_rust_string_lossy(&mut catch))
        .unwrap_or_default();
      let info = exc.to_rust_string_lossy(&mut catch);
      panic!("exception: {}\nstack: {}", info, stack);
    } else if !build_ok {
      panic!("unknown exception");
    }

    global.delete_ext(&mut catch, NI_ENTRY_KEY);
  }

  if let Some(sc) = &mut sc {
    let blob = sc
      .create_blob(v8::FunctionCodeHandling::Keep)
      .expect("snapshot creation failed")
      .to_vec();

    const N: usize = 5;

    for _ in 0..N {
      let mut isolate = v8::Isolate::new(v8::CreateParams::default().snapshot_blob(blob.clone()));
      let mut isolate_scope = v8::HandleScope::new(&mut *isolate);
      v8::Context::new(&mut isolate_scope);
    }
    log::info!(
      "Context deserialization test completed. All {} attempts succeeded.",
      N
    );

    std::fs::write(&opt.output, &blob[..]).unwrap();
    log::info!("Written snapshot.");
  }

  log::info!("Build completed.");
}

fn print_schema() {
  #[derive(JsonSchema)]
  #[allow(dead_code)]
  struct Root {
    blueboat_request: BlueboatRequest,
    blueboat_response: BlueboatResponse,
    apns_request: ApnsRequest,
    apns_response: ApnsResponse,
    blueboat_bootstrap_data: BlueboatBootstrapData,
    canvas_config: CanvasConfig,
    canvas_encode_config: CanvasEncodeConfig,
    canvas_draw_config: CanvasDrawConfig,
    canvas_render_svg_config: CanvasRenderSvgConfig,
    codec_base64_mode: CodecBase64Mode,
    canvas_op: CanvasOp,
    text_markdown_render_opts: TextMarkdownRenderOpts,
    s3_put_object_request: S3PutObjectRequest,
    s3_get_object_request: S3GetObjectRequest,
    s3_delete_object_request: S3DeleteObjectRequest,
    s3_upload_part_request: S3UploadPartRequest,
    s3_region: S3Region,
    s3_credentials: S3Credentials,
    s3_presign_info: S3PresignInfo,
    s3_list_objects_v2_request: S3ListObjectsV2Request,
    s3_list_objects_v2_output: S3ListObjectsV2Output,
    s3_presign_options: S3PresignOptions,
    graphics_text_measure_settings: GraphicsTextMeasureSettings,
    graphics_text_measure_output: GraphicsTextMeasureOutput,
  }

  let schema = schema_for!(Root);
  println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
