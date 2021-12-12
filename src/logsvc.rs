use std::{
  collections::BTreeMap,
  sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
  },
  time::SystemTime,
};

use anyhow::Result;
use lazy_static::lazy_static;
use rdkafka::{
  producer::{FutureProducer, FutureRecord, Producer},
  ClientConfig,
};
use regex::Regex;
use serde::Serialize;
use tracing::field::Field;
use tracing_subscriber::Layer;

use crate::lpch::AppLogEntry;

lazy_static! {
  static ref KAFKA_CONFIG_MATCHER: Regex = Regex::new("^([a-zA-Z0-9._-]+):([0-9]+)@(.+)$").unwrap();
}

static NEXT_TID: AtomicU64 = AtomicU64::new(1);

thread_local! {
  static UNIQUE_TID: u64 = NEXT_TID.fetch_add(1, Ordering::Relaxed);
}

fn unique_tid() -> String {
  format!("{}", UNIQUE_TID.with(|tid| *tid))
}

#[derive(Clone)]
pub struct LogService {
  inner: Arc<LogServiceInner>,
}

#[derive(Serialize)]
struct Payload<'a> {
  host: &'a str,
  pid: u32,
  tid: String,
  ts: f64,
  name: &'a str,
  target: &'a str,
  level: &'a str,
  module_path: &'a str,
  file: &'a str,
  line: u32,
  fields: &'a BTreeMap<&'a str, serde_json::Value>,
  span_id: String,
}

struct LogServiceInner {
  producer: FutureProducer,
  topic: String,
  partition: i32,
  hostname: String,
  pid: u32,
}

impl LogService {
  pub fn open(s: &str) -> Result<Self> {
    let caps = match KAFKA_CONFIG_MATCHER.captures(s) {
      Some(x) => x,
      None => anyhow::bail!("invalid kafka config string"),
    };

    let topic = &caps[1];
    let partition: i32 = caps[2].parse()?;
    let servers = &caps[3];

    let mut client_config = ClientConfig::new();
    client_config.set("bootstrap.servers", servers);
    let hostname = hostname::get()
      .ok()
      .and_then(|x| x.into_string().ok())
      .unwrap_or_else(|| format!("unknown-host-{}", uuid::Uuid::new_v4()));
    let pid = std::process::id();

    let producer = client_config.create()?;
    Ok(Self {
      inner: Arc::new(LogServiceInner {
        producer,
        topic: topic.to_string(),
        partition,
        hostname,
        pid,
      }),
    })
  }

  pub fn flush_before_exit(&self) {
    self.inner.producer.flush(rdkafka::util::Timeout::Never);
  }

  pub fn write(&self, entry: &str) {
    if let Err((e, _)) = self.inner.producer.send_result(FutureRecord {
      topic: &self.inner.topic,
      partition: Some(self.inner.partition),
      payload: Some(entry),
      key: None::<&[u8]>,
      timestamp: None,
      headers: None,
    }) {
      let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis();
      eprintln!("[{}] failed to send log entry: {} {}", now, e, entry);
    }
  }

  pub fn write_applog(&self, entry: AppLogEntry) {
    let entry = match serde_json::to_string(&entry) {
      Ok(x) => x,
      Err(e) => {
        eprintln!("[applog] failed to serialize log entry: {}", e);
        return;
      }
    };
    self.write(&entry);
  }

  fn emit_syslog(&self, payload: &Payload) {
    match serde_json::to_string(payload) {
      Ok(x) => self.write(&x),
      Err(e) => eprintln!("[syslog] failed to serialize log entry: {}", e),
    }
  }
}

impl<S> Layer<S> for LogService
where
  S: tracing::Subscriber,
{
  fn on_new_span(
    &self,
    attrs: &tracing::span::Attributes<'_>,
    id: &tracing::span::Id,
    _ctx: tracing_subscriber::layer::Context<'_, S>,
  ) {
    let mut visitor = JsonVisitor {
      values: BTreeMap::new(),
    };
    attrs.record(&mut visitor);
    let md = attrs.metadata();
    let name = format!("new span: {}", md.name());

    let payload = Payload {
      host: &self.inner.hostname,
      pid: self.inner.pid,
      tid: unique_tid(),
      ts: now_ts_secs_f64(),
      name: &name,
      target: md.target(),
      level: md.level().as_str(),
      module_path: md.module_path().unwrap_or(""),
      file: md.file().unwrap_or(""),
      line: md.line().unwrap_or(0),
      fields: &visitor.values,
      span_id: format!("{}", id.into_u64()),
    };
    self.emit_syslog(&payload);
  }

  fn on_enter(&self, id: &tracing::span::Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
    let payload = Payload {
      host: &self.inner.hostname,
      pid: self.inner.pid,
      tid: unique_tid(),
      ts: now_ts_secs_f64(),
      name: "enter span",
      target: "",
      level: "",
      module_path: "",
      file: "",
      line: 0,
      fields: &BTreeMap::new(),
      span_id: format!("{}", id.into_u64()),
    };
    self.emit_syslog(&payload);
  }

  fn on_exit(&self, id: &tracing::span::Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
    let payload = Payload {
      host: &self.inner.hostname,
      pid: self.inner.pid,
      tid: unique_tid(),
      ts: now_ts_secs_f64(),
      name: "exit span",
      target: "",
      level: "",
      module_path: "",
      file: "",
      line: 0,
      fields: &BTreeMap::new(),
      span_id: format!("{}", id.into_u64()),
    };
    self.emit_syslog(&payload);
  }

  fn on_close(&self, id: tracing::span::Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
    let payload = Payload {
      host: &self.inner.hostname,
      pid: self.inner.pid,
      tid: unique_tid(),
      ts: now_ts_secs_f64(),
      name: "close span",
      target: "",
      level: "",
      module_path: "",
      file: "",
      line: 0,
      fields: &BTreeMap::new(),
      span_id: format!("{}", id.into_u64()),
    };
    self.emit_syslog(&payload);
  }

  fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
    let mut visitor = JsonVisitor {
      values: BTreeMap::new(),
    };
    event.record(&mut visitor);
    let md = event.metadata();
    let payload = Payload {
      host: &self.inner.hostname,
      pid: self.inner.pid,
      tid: unique_tid(),
      ts: now_ts_secs_f64(),
      name: md.name(),
      target: md.target(),
      level: md.level().as_str(),
      module_path: md.module_path().unwrap_or(""),
      file: md.file().unwrap_or(""),
      line: md.line().unwrap_or(0),
      fields: &visitor.values,
      span_id: "".into(),
    };
    self.emit_syslog(&payload);
  }
}

fn now_ts_secs_f64() -> f64 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_secs_f64()
}

// https://docs.rs/tracing-subscriber/latest/src/tracing_subscriber/fmt/format/json.rs.html
pub struct JsonVisitor<'a> {
  values: BTreeMap<&'a str, serde_json::Value>,
}

impl<'a> tracing::field::Visit for JsonVisitor<'a> {
  /// Visit a double precision floating point value.
  fn record_f64(&mut self, field: &Field, value: f64) {
    self
      .values
      .insert(field.name(), serde_json::Value::from(value));
  }

  /// Visit a signed 64-bit integer value.
  fn record_i64(&mut self, field: &Field, value: i64) {
    self
      .values
      .insert(field.name(), serde_json::Value::from(value));
  }

  /// Visit an unsigned 64-bit integer value.
  fn record_u64(&mut self, field: &Field, value: u64) {
    self
      .values
      .insert(field.name(), serde_json::Value::from(value));
  }

  /// Visit a boolean value.
  fn record_bool(&mut self, field: &Field, value: bool) {
    self
      .values
      .insert(field.name(), serde_json::Value::from(value));
  }

  /// Visit a string value.
  fn record_str(&mut self, field: &Field, value: &str) {
    self
      .values
      .insert(field.name(), serde_json::Value::from(value));
  }

  fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
    match field.name() {
      // Skip fields that are actually log metadata that have already been handled
      name if name.starts_with("log.") => (),
      name if name.starts_with("r#") => {
        self
          .values
          .insert(&name[2..], serde_json::Value::from(format!("{:?}", value)));
      }
      name => {
        self
          .values
          .insert(name, serde_json::Value::from(format!("{:?}", value)));
      }
    };
  }
}
