use std::convert::Infallible;
use std::net::IpAddr;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc, time::Instant};

use crate::headers::{
  HDR_GLOBAL_PREFIX, HDR_REQ_CLIENT_CITY, HDR_REQ_CLIENT_COUNTRY, HDR_REQ_CLIENT_IP,
  HDR_REQ_CLIENT_SUBDIVISION_PREFIX, HDR_REQ_CLIENT_WPBL, HDR_REQ_METADATA, HDR_REQ_REQUEST_ID,
  HDR_RES_HANDLE_LATENCY, HDR_RES_REQUEST_ID, PROXY_HEADER_WHITELIST,
};
use crate::ipc::{BlueboatIpcReqV, BlueboatIpcRes};
use crate::logsvc::LogService;
use crate::lpch::{BackgroundEntry, LowPriorityMsg};
use crate::mds::config_v2::MdsConfig;
use crate::mds::{MdsService, MDS};
use crate::pm::pm_handle;
use crate::pubsub::mq::{MessageQueue, MessageQueueConfig};
use crate::pubsub::MQ;
use crate::reliable_channel::create_reliable_channel;
use crate::wpbl::WpblDb;
use crate::{
  ctx::BlueboatInitData,
  ipc::{BlueboatIpcReq, BlueboatRequest},
  metadata::Metadata,
  package::PackageKey,
};
use hyper::header::{HeaderName, HeaderValue};
use hyper::StatusCode;
use hyper::{
  service::{make_service_fn, service_fn},
  Body, Request, Response, Server,
};
use maxminddb::geoip2::City;
use memmap2::Mmap;
use parking_lot::Mutex;
use rusoto_core::Region;
use rusoto_s3::{GetObjectRequest, S3Client, S3};
use smr::config::{APP_INACTIVE_TIMEOUT_MS, SPRING_CLEANING_INTERVAL_MS};
use smr::ipc_channel::ipc::IpcSender;
use smr::scheduler::Scheduler;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

use anyhow::Result;
use structopt::StructOpt;
use sysinfo::{RefreshKind, System, SystemExt};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::signal::unix::SignalKind;
use tokio::sync::{OnceCell, RwLock, Semaphore};
use uuid::Uuid;

#[derive(Debug, StructOpt)]
#[structopt(
  name = "blueboat",
  about = "The monolithic runtime for modern web backends."
)]
struct Opt {
  /// Listen address.
  #[structopt(short, long)]
  listen: SocketAddr,

  /// S3 bucket for storing apps' code and metadata.
  #[structopt(long, default_value = "-")]
  s3_bucket: String,

  /// S3 region for storing apps' code and metadata.
  #[structopt(long, default_value = "-")]
  s3_region: String,

  /// S3-compatible endpoint for storing apps' code and metadata. Leave this default if using AWS S3.
  #[structopt(long, default_value = "-")]
  s3_endpoint: String,

  /// High watermark of available memory. Currently unused.
  #[structopt(long, default_value = "524288")]
  mem_high_watermark_kb: u64,

  /// Critical watermark of available memory. The scheduler will begin killing workers after this threshold is reached.
  #[structopt(long, default_value = "131072")]
  mem_critical_watermark_kb: u64,

  /// Kafka cluster(s) for writing apps' logs. Looks like "com.example.blueboat.applog:0@kafka.core.svc.cluster.local:9092"
  #[structopt(long, default_value = "-")]
  log_kafka: String,

  /// Kafka cluster(s) for writing Blueboat's own logs. Looks like "com.example.blueboat.syslog:0@kafka.core.svc.cluster.local:9092"
  #[structopt(long, default_value = "-")]
  syslog_kafka: String,

  /// Path to Maxmind GeoIP2 database.
  #[structopt(long, default_value = "-")]
  mmdb_city: String,

  /// Path to the Wikipedia Blocklist database.
  #[structopt(long, default_value = "-")]
  wpbl_db: String,

  /// Metadata service bootstrap URL. Usually looks like "wss://mds.example.com/bootstrap-shard".
  #[structopt(long, default_value = "-")]
  mds: String,

  /// Path to package cache file. Default is memory.
  #[structopt(long, default_value = "-")]
  package_cache: String,

  /// Max number of entries in metadata cache.
  #[structopt(long, default_value = "200")]
  md_cache_size: u64,

  /// Whether this instance should listen to and process background tasks.
  #[structopt(long)]
  accept_background_tasks: bool,

  /// Enable Tokio console for monitoring. Do not enable this in production as there seems to be memory leak.
  #[structopt(long)]
  enable_tokio_console: bool,

  /// (not yet implemented) Enable HTTP fastpath. Let Blueboat deal with domain-based routing, and eliminate the need for a reverse proxy.
  #[structopt(long)]
  enable_http_fastpath: bool,

  /// Drop the main process to UID 1 (workers are always dropped). Only enable this if Blueboat starts as root.
  #[structopt(long)]
  drop_privileges: bool,

  /// Pubsub FoundationDB cluster file path.
  #[structopt(long, default_value = "-")]
  pubsub_cluster: String,

  /// Pubsub FoundationDB key prefix.
  #[structopt(long, default_value = "-")]
  pubsub_prefix: String,

  /// Run in single-tenant mode with the provided `metadata.json`.
  #[structopt(long, default_value = "-")]
  single_tenant: String,
}

struct LpContext {
  log_kafka: Option<LogService>,
  bg_permit: Arc<Semaphore>,
}

type MdCacheType = moka::sync::Cache<String, Arc<Metadata>>;

static SCHEDULER: OnceCell<Arc<RwLock<Scheduler<PackageKey, BlueboatIpcReq>>>> =
  OnceCell::const_new();
static TENANCY: OnceCell<Tenancy> = OnceCell::const_new();
static CACHE: OnceCell<sqlite_cache::Topic> = OnceCell::const_new();
static MD_CACHE: OnceCell<MdCacheType> = OnceCell::const_new();
static MEM_HIGH_WATERMARK_KB: OnceCell<u64> = OnceCell::const_new();
static MEM_CRITICAL_WATERMARK_KB: OnceCell<u64> = OnceCell::const_new();
static LP_TX: OnceCell<Mutex<IpcSender<LowPriorityMsg>>> = OnceCell::const_new();
static MMDB_CITY: OnceCell<Option<maxminddb::Reader<Mmap>>> = OnceCell::const_new();
static WPBL_DB: OnceCell<Option<WpblDb>> = OnceCell::const_new();

static LP_DISPATCH_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static LP_BG_ISSUE_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static HTTP_FAST_PATH: AtomicBool = AtomicBool::new(false);

const MIN_GAP_KB: u64 = 65536;
const WORKER_IDLE_TTL_SECS: u64 = 400;

pub fn main() {
  let network = unsafe { foundationdb::boot() };
  tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .unwrap()
    .block_on(async_main());
  drop(network);
}

pub fn global_scheduler() -> &'static Arc<RwLock<Scheduler<PackageKey, BlueboatIpcReq>>> {
  SCHEDULER.get().unwrap()
}

pub enum Tenancy {
  MultiTenant {
    s3: (S3Client, String),
  },
  SingleTenant {
    metadata: Metadata,
    package: Vec<u8>,
  },
}

pub fn tenancy() -> &'static Tenancy {
  TENANCY.get().unwrap()
}

pub fn cache() -> &'static sqlite_cache::Topic {
  CACHE.get().unwrap()
}

fn md_cache() -> &'static MdCacheType {
  MD_CACHE.get().unwrap()
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Debug)]
enum MemoryWatermark {
  Normal,
  High,
  Critical,
}

fn memory_watermark(last: Option<MemoryWatermark>) -> (MemoryWatermark, u64) {
  let mut sys = System::new_with_specifics(RefreshKind::new().with_memory());
  sys.refresh_memory();

  let sys_avail_mem = sys.available_memory();

  let cg_avail_mem = if let Ok(cgroup) = std::fs::read_to_string("/proc/self/cgroup") {
    let cg_path: Option<&str> = cgroup
      .split("\n")
      .map(|x| x.splitn(3, ':').collect::<Vec<_>>())
      .filter(|x| x.get(1).copied() == Some("memory"))
      .filter_map(|x| x.get(2).copied())
      .next();
    if let Some(cg_path) = cg_path {
      let cg_limit = std::fs::read_to_string(&format!(
        "/sys/fs/cgroup/memory{}/memory.limit_in_bytes",
        cg_path
      ))
      .ok();
      let cg_usage = std::fs::read_to_string(&format!(
        "/sys/fs/cgroup/memory{}/memory.usage_in_bytes",
        cg_path
      ))
      .ok();
      log::trace!("cg path {}, mem {:?} {:?}", cg_path, cg_limit, cg_usage);
      let x: (Option<u64>, Option<u64>) = (
        cg_limit.and_then(|x| x.trim().parse().ok()),
        cg_usage.and_then(|x| x.trim().parse().ok()),
      );
      if let (Some(cg_limit), Some(cg_usage)) = x {
        cg_limit.saturating_sub(cg_usage) / 1024
      } else {
        sys_avail_mem
      }
    } else {
      sys_avail_mem
    }
  } else {
    sys_avail_mem
  };

  let avail_mem = sys_avail_mem.min(cg_avail_mem);
  let critical_wm = *MEM_CRITICAL_WATERMARK_KB.get().unwrap();
  let high_wm = *MEM_HIGH_WATERMARK_KB.get().unwrap();
  let (wm, gap) = if avail_mem < critical_wm {
    (MemoryWatermark::Critical, avail_mem)
  } else if avail_mem < high_wm {
    (MemoryWatermark::High, avail_mem - critical_wm)
  } else {
    (MemoryWatermark::Normal, avail_mem - high_wm)
  };

  if let Some(last) = last {
    if last > wm {
      // We are lowering the watermark - let's stablize it.
      if gap < MIN_GAP_KB {
        return (last, avail_mem);
      }
    }
  }
  (wm, avail_mem)
}

impl MemoryWatermark {
  fn tune_smr_parameters(&self) {
    match self {
      MemoryWatermark::Critical => {
        smr::config::MAX_WORKERS_PER_APP.store(1);
      }
      MemoryWatermark::High => {
        smr::config::MAX_WORKERS_PER_APP.store(4);
      }
      MemoryWatermark::Normal => {
        smr::config::MAX_WORKERS_PER_APP.store(4);
      }
    }
  }

  fn ss_evict(&self) {}
}

async fn async_main() {
  let opt = Opt::from_args();

  let mut syslog_service: Option<LogService> = None;

  if opt.syslog_kafka != "-" {
    let syslog = LogService::open(&opt.syslog_kafka)
      .map_err(|e| e.context("opening syslog-kafka"))
      .unwrap();
    syslog_service = Some(syslog.clone());
    if opt.enable_tokio_console {
      tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(syslog.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .init();
    } else {
      tracing_subscriber::registry()
        .with(syslog.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .init();
    }
    tracing::warn!("blueboat starting");
  } else {
    if opt.enable_tokio_console {
      tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(tracing_subscriber::fmt::layer())
        .init();
    } else {
      // So that `RUST_LOG` is not ignored.
      tracing_subscriber::fmt::init();
    }
    log::info!("Logging to stderr. Please use --syslog-kafka in production.");
  }

  if opt.drop_privileges {
    nix::unistd::setuid(nix::unistd::Uid::from_raw(1)).expect("failed to drop to uid 1");
    tracing::warn!("Dropped to uid 1.");
  }

  if opt.single_tenant == "-" {
    if opt.s3_region == "-" || opt.s3_bucket == "-" {
      panic!("--s3-region and --s3-bucket are required in multi-tenant mode");
    }
    let s3_client = S3Client::new(if opt.s3_endpoint != "-" {
      Region::Custom {
        name: opt.s3_region.clone(),
        endpoint: opt.s3_endpoint.clone(),
      }
    } else {
      Region::from_str(&opt.s3_region).unwrap()
    });

    TENANCY
      .set(Tenancy::MultiTenant {
        s3: (s3_client, opt.s3_bucket.clone()),
      })
      .unwrap_or_else(|_| unreachable!());
    tracing::info!("Running in multi-tenant mode.");
  } else {
    let metadata_json =
      std::fs::read_to_string(&opt.single_tenant).expect("failed to read single-tenant metadata");
    let metadata: Metadata =
      serde_json::from_str(&metadata_json).expect("failed to parse single-tenant metadata");
    let package_path = Path::new(&opt.single_tenant)
      .parent()
      .expect("failed to get metadata directory")
      .to_path_buf()
      .join(&metadata.package);
    let package = std::fs::read(&package_path).expect("failed to read package");
    TENANCY
      .set(Tenancy::SingleTenant { metadata, package })
      .unwrap_or_else(|_| unreachable!());
    tracing::info!("Running in single-tenant mode.");
  }

  if opt.pubsub_cluster != "-" && opt.pubsub_prefix != "-" {
    let mq = MessageQueue::open(MessageQueueConfig {
      fdb_cluster_file: opt.pubsub_cluster.clone(),
      prefix: opt.pubsub_prefix.clone(),
    })
    .expect("failed to open pubsub");
    MQ.set(mq).unwrap_or_else(|_| unreachable!());
    tracing::warn!(
      cluster = opt.pubsub_cluster,
      prefix = opt.pubsub_prefix,
      "pubsub opened"
    );
  }

  // Tweak parameters
  SPRING_CLEANING_INTERVAL_MS.store(300);
  APP_INACTIVE_TIMEOUT_MS.store(WORKER_IDLE_TTL_SECS * 1000);

  let scheduler = Scheduler::<PackageKey, _>::new(pm_handle());
  SCHEDULER.set(scheduler).unwrap_or_else(|_| unreachable!());

  let package_cache = if opt.package_cache == "-" {
    rusqlite::Connection::open_in_memory().unwrap()
  } else {
    rusqlite::Connection::open(&opt.package_cache).expect("failed to open package cache file")
  };
  let package_cache = sqlite_cache::Cache::new(Default::default(), package_cache)
    .expect("failed to initialize package cache")
    .topic("packages")
    .expect("failred to initialize package cache topic");

  CACHE.set(package_cache).unwrap_or_else(|_| unreachable!());
  MD_CACHE
    .set(moka::sync::Cache::new(opt.md_cache_size))
    .unwrap_or_else(|_| unreachable!());
  MEM_HIGH_WATERMARK_KB
    .set(opt.mem_high_watermark_kb)
    .unwrap_or_else(|_| unreachable!());
  MEM_CRITICAL_WATERMARK_KB
    .set(opt.mem_critical_watermark_kb)
    .unwrap_or_else(|_| unreachable!());

  let (lp_tx, lp_rx) = smr::ipc_channel::ipc::channel::<LowPriorityMsg>().unwrap();
  LP_TX
    .set(Mutex::new(lp_tx))
    .unwrap_or_else(|_| unreachable!());

  let mmdb_city = if opt.mmdb_city != "-" {
    match maxminddb::Reader::open_mmap(&opt.mmdb_city) {
      Ok(x) => {
        log::warn!("Opened MMDB (city) at {}.", opt.mmdb_city);
        Some(x)
      }
      Err(e) => {
        log::error!("mmdb open ({}) failed: {:?}", opt.mmdb_city, e);
        None
      }
    }
  } else {
    None
  };
  MMDB_CITY.set(mmdb_city).unwrap_or_else(|_| unreachable!());

  let wpbl_db = if opt.wpbl_db != "-" {
    match WpblDb::open(&opt.wpbl_db) {
      Ok(x) => {
        log::warn!("Opened Wikipedia blocklist DB at {}.", opt.wpbl_db);
        Some(x)
      }
      Err(e) => {
        log::error!("wpbl open ({}) failed: {:?}", opt.wpbl_db, e);
        None
      }
    }
  } else {
    None
  };
  WPBL_DB.set(wpbl_db).unwrap_or_else(|_| unreachable!());

  let mds = if opt.mds != "" && opt.mds != "-" {
    let config = MdsConfig::parse(&opt.mds)
      .map_err(|e| e.context("failed to parse mds config"))
      .unwrap();
    let mds = MdsService::open(&config)
      .map_err(|e| e.context("failed to open mds service"))
      .unwrap();
    Some(mds)
  } else {
    None
  };
  let has_mds = mds.is_some();
  MDS.set(mds).unwrap_or_else(|_| unreachable!());

  if opt.enable_http_fastpath {
    if !has_mds {
      log::error!("HTTP fastpath requires MDS");
      std::process::exit(1);
    }
    log::warn!("HTTP fastpath enabled. Routing will be managed by Blueboat.");
    HTTP_FAST_PATH.store(true, Ordering::Relaxed);

    panic!("http fastpath is not yet implemented");
  }

  // Monitor memory pressure
  std::thread::spawn(|| {
    let mut last_wm: Option<MemoryWatermark> = None;
    loop {
      let (wm, avail_mem) = memory_watermark(last_wm);
      if let Some(last) = last_wm {
        if last != wm {
          tracing::warn!(last_wm = ?last, new_wm = ?wm, avail_mem = %avail_mem, "memory watermark changed");
        }
      } else {
        tracing::warn!(wm = ?wm, avail_mem = %avail_mem, "memory watermark initialized");
      }
      last_wm = Some(wm);
      wm.tune_smr_parameters();
      wm.ss_evict();
      std::thread::sleep(Duration::from_secs(1));
    }
  });

  tokio::spawn(async move {
    let mut sig = tokio::signal::unix::signal(SignalKind::user_defined1()).unwrap();
    log::warn!("SIGUSR1 handler registered. Send SIGUSR1 to process {} and system status will be printed to stderr.", std::process::id());
    loop {
      sig.recv().await;
      print_status().await;
    }
  });

  let mut applog_service: Option<LogService> = None;

  // Write logs
  if opt.log_kafka != "-" {
    log::warn!("Logging enabled. Logs will be written to the provided kafka cluster.");
    let producer = LogService::open(&opt.log_kafka).unwrap();
    applog_service = Some(producer);
  }

  let lp_ctx = LpContext {
    log_kafka: applog_service.clone(),
    bg_permit: Arc::new(Semaphore::new(500)),
  };

  spawn_lp_handler(Arc::new(lp_ctx), lp_rx);

  if opt.accept_background_tasks {
    log::warn!("Background tasks not implemented.");
  }

  let make_svc = make_service_fn(|_| async move { Ok::<_, hyper::Error>(service_fn(handle)) });

  tracing::warn!(address = %opt.listen, "start listener");
  let server = Server::bind(&opt.listen).serve(make_svc);
  let graceful = server.with_graceful_shutdown(shutdown_signal());

  if let Err(e) = graceful.await {
    tracing::error!(error = %e, "server error");
  } else {
    tracing::warn!("server shutdown");
  }

  let bg_shutdown_start = Instant::now();
  std::mem::forget(BACKGROUND_TASK_LOCK.write().await);
  tracing::warn!(duration = ?bg_shutdown_start.elapsed(), "background tasks completed");

  tracing::warn!("system shutdown");

  if let Some(applog_service) = &applog_service {
    applog_service.flush_before_exit();
  }

  if let Some(syslog_service) = &syslog_service {
    syslog_service.flush_before_exit();
  }

  eprintln!("flushed logs");
}

async fn shutdown_signal() {
  let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate()).unwrap();
  let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt()).unwrap();
  loop {
    tokio::select! {
      _ = sigterm.recv() => {
        tracing::warn!("received SIGTERM");
        break;
      }
      _ = sigint.recv() => {
        tracing::warn!("received SIGINT");
        break;
      }
    }
  }
}

async fn handle(mut req: Request<Body>) -> Result<Response<Body>, Infallible> {
  if req.uri().path() == "/_blueboat/health" {
    return Ok(Response::new(Body::from("OK")));
  }

  let md_path = if matches!(tenancy(), Tenancy::MultiTenant { .. }) {
    match req.headers().get(HDR_REQ_METADATA) {
      Some(x) => x.to_str().unwrap_or("").to_string(),
      None => {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(res);
      }
    }
  } else {
    "app".to_string()
  };

  let client_ip = req
    .headers()
    .get(HDR_REQ_CLIENT_IP)
    .and_then(|x| x.to_str().ok())
    .map(|x| x.to_string());

  let headers = req.headers_mut();

  // Sanitize headers.
  {
    let mut remove_list = vec![];
    for name in headers.keys() {
      let name_s = name.as_str();
      if name_s.starts_with(HDR_GLOBAL_PREFIX) && !PROXY_HEADER_WHITELIST.contains(name_s) {
        remove_list.push(name.clone());
      }
    }
    for name in &remove_list {
      headers.remove(name);
    }
  }

  if let Some(client_ip) = &client_ip {
    if let Ok(x) = IpAddr::from_str(client_ip) {
      // Query MMDB for geoip information.
      if let Some(mmdb_city) = MMDB_CITY.get().unwrap() {
        let city: Option<City> = mmdb_city.lookup(x).ok();
        if let Some(city) = city {
          if let Some(country) = city.country.as_ref().and_then(|x| x.iso_code) {
            if let Ok(h) = HeaderValue::from_str(country) {
              headers.insert(HDR_REQ_CLIENT_COUNTRY, h);
            }
          }
          for (i, x) in city
            .subdivisions
            .as_ref()
            .map(|x| x.as_slice())
            .unwrap_or(&[])
            .iter()
            .enumerate()
          {
            let id = i + 1;
            let v = x
              .iso_code
              .and_then(|x| HeaderValue::from_str(x).ok())
              .unwrap_or(HeaderValue::from_static(""));
            headers.insert(
              HeaderName::from_str(&format!("{}{}", HDR_REQ_CLIENT_SUBDIVISION_PREFIX, id))
                .unwrap(),
              v,
            );
          }
          if let Some(city) = &city.city {
            if let Some(names) = &city.names {
              if let Some(name) = names.get("en") {
                if let Ok(h) = HeaderValue::from_str(*name) {
                  headers.insert(HDR_REQ_CLIENT_CITY, h);
                }
              }
            }
          }
        }
      }

      // Query WPBL.
      if let Some(wpbl) = WPBL_DB.get().unwrap() {
        match wpbl.in_blocklist(x).await {
          Ok(x) => {
            headers.insert(
              HDR_REQ_CLIENT_WPBL,
              HeaderValue::from_static(if x { "1" } else { "0" }),
            );
          }
          Err(e) => {
            log::error!("wpbl blocklist query failed (ip {}): {:?}", x, e);
          }
        }
      }
    }
  }

  match raw_handle(req, &md_path).await {
    Ok(x) => Ok(x),
    Err(e) => {
      tracing::error!(app = %md_path, error = %e, "early runtime error");
      let mut res = Response::new(Body::empty());
      *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
      Ok(res)
    }
  }
}

async fn load_md(path: &str) -> Result<Arc<Metadata>> {
  #[derive(Error, Debug)]
  #[error("metadata error")]
  struct MetadataError;

  let mut md = match tenancy() {
    Tenancy::MultiTenant { s3: (s3c, bucket) } => {
      let md = s3c
        .get_object(GetObjectRequest {
          bucket: bucket.clone(),
          key: path.to_string(),
          ..Default::default()
        })
        .await?;
      let mut body: Vec<u8> = vec![];
      md.body
        .ok_or(MetadataError)?
        .into_async_read()
        .read_to_end(&mut body)
        .await?;
      serde_json::from_slice(&body)?
    }
    Tenancy::SingleTenant { metadata, .. } => metadata.clone(),
  };

  md.path = path.to_string();
  for (k, x) in &mut md.pubsub {
    if hex::decode_to_slice(&x.namespace, &mut x.namespace_bytes).is_err() {
      match Uuid::parse_str(x.namespace.as_str()) {
        Ok(uuid) => {
          x.namespace_bytes.copy_from_slice(uuid.as_bytes());
        }
        Err(_) => {
          anyhow::bail!("invalid pubsub '{}' namespace id: {}", k, x.namespace);
        }
      }
    }
  }
  Ok(Arc::new(md))
}

async fn load_md_with_cache(md_path: &String) -> Result<Arc<Metadata>> {
  let md = md_cache().get(md_path);
  if let Some(md) = md {
    Ok(md)
  } else {
    let md = load_md(&md_path).await?;
    md_cache().insert(md_path.to_string(), md.clone());
    Ok(md)
  }
}

pub async fn generic_invoke(
  req: BlueboatIpcReq,
  md: Arc<Metadata>,
  version_assertion: Option<&str>,
) -> Result<BlueboatIpcRes> {
  #[derive(Error, Debug)]
  #[error("version changed")]
  struct VersionChanged;

  if let Some(version_assertion) = version_assertion {
    if md.version != version_assertion {
      return Err(VersionChanged.into());
    }
  }

  let pk = PackageKey {
    path: md.path.to_string(),
    version: md.version.clone(),
  };
  let pk2 = pk.clone();
  let w = Scheduler::get_worker(global_scheduler(), &pk, move || {
    let rch = create_reliable_channel(md.clone());
    BlueboatInitData {
      key: pk2.clone(),
      metadata: (*md).clone(),
      lp_tx: LP_TX.get().unwrap().lock().clone(),
      rch: Some(rch),
    }
  })
  .await?;

  let res = w.invoke(req).await?;

  Ok(res)
}

async fn raw_handle(mut req: Request<Body>, md_path: &String) -> Result<Response<Body>> {
  #[derive(Error, Debug)]
  #[error("metadata error")]
  struct MetadataError;

  let handle_start = Instant::now();
  let md = load_md_with_cache(md_path)
    .await
    .map_err(|e| e.context("failed to load metadata"))?;

  let request_id = req
    .headers()
    .get(HDR_REQ_REQUEST_ID)
    .and_then(|x| x.to_str().ok())
    .map(|x| x.to_string())
    .unwrap_or_else(|| format!("u:{}", Uuid::new_v4().to_string()));
  req
    .headers_mut()
    .insert(HDR_REQ_REQUEST_ID, HeaderValue::from_str(&request_id)?);

  if req.uri().path() == "/_blueboat/events" {
    return crate::pubsub::sse::handle_sse(&request_id, req, md)
      .await
      .map_err(|e| e.context("sse"));
  }
  let request = BlueboatRequest::from_hyper(req).await?;
  let request = BlueboatIpcReq {
    v: BlueboatIpcReqV::Http(request),
    id: request_id.clone(),
  };
  let res = generic_invoke(request, md, None).await;
  let mut res = match res {
    Ok(res) => res.response.into_hyper(res.body)?,
    Err(e) => {
      let mut res = hyper::Response::new(Body::from("invoke error".to_string()));
      *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
      log::error!("app {} request {:?}: {}", md_path, request_id, e);
      res
    }
  };
  let handle_dur = handle_start.elapsed();
  res.headers_mut().insert(
    HDR_RES_HANDLE_LATENCY,
    HeaderValue::from_str(&format!("{:.2}", handle_dur.as_secs_f64() * 1000.0)).unwrap(),
  );
  if let Ok(v) = HeaderValue::from_str(&request_id) {
    res.headers_mut().insert(HDR_RES_REQUEST_ID, v);
  }
  Ok(res)
}

fn spawn_lp_handler(ctx: Arc<LpContext>, rx: smr::ipc_channel::ipc::IpcReceiver<LowPriorityMsg>) {
  let handle = tokio::runtime::Handle::current();
  std::thread::spawn(move || loop {
    let msg = match rx.recv() {
      Ok(x) => x,
      Err(_) => break,
    };
    let _guard = handle.enter();
    issue_lp(&ctx, msg);
  });
}

fn issue_lp(ctx: &Arc<LpContext>, msg: LowPriorityMsg) {
  match msg {
    LowPriorityMsg::Log(msg) => {
      if let Some(producer) = &ctx.log_kafka {
        producer.write_applog(msg);
      }
    }
    LowPriorityMsg::Background(entry) => match ctx.bg_permit.clone().try_acquire_owned() {
      Ok(permit) => {
        tokio::spawn(async move {
          run_background_entry(entry).await;
          drop(permit);
        });
      }
      Err(_) => {
        LP_BG_ISSUE_FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
      }
    },
  }
}

static BACKGROUND_TASK_LOCK: RwLock<()> = RwLock::const_new(());

async fn run_background_entry(entry: BackgroundEntry) {
  let _entry_g = BACKGROUND_TASK_LOCK.read().await;
  let request_id = format!(
    "{}+bg-{}",
    entry.request_id.split("+").next().unwrap(),
    Uuid::new_v4().to_string()
  );
  let task_span = tracing::info_span!("background task", request_id = %request_id, package_path = %entry.app.path, package_version = %entry.app.version);
  do_run_background_entry(entry, request_id)
    .instrument(task_span)
    .await;
}

async fn do_run_background_entry(entry: BackgroundEntry, request_id: String) {
  tracing::info!("run background task");

  let req = BlueboatIpcReq {
    v: BlueboatIpcReqV::Background(entry.wire_bytes),
    id: request_id,
  };
  let app = entry.app;
  let md = match load_md_with_cache(&app.path).await {
    Ok(x) => x,
    Err(e) => {
      tracing::error!(reason = "load_md", error = %e, "background task failed");
      return;
    }
  };
  let fut = async {
    match generic_invoke(
      req,
      md,
      if entry.same_version {
        Some(app.version.as_str())
      } else {
        None
      },
    )
    .await
    {
      Ok(_) => {}
      Err(e) => {
        tracing::error!(reason = "invoke", error = %e, "background task failed");
      }
    }
  };
  tokio::select! {
    _ = fut => {},
    _ = tokio::time::sleep(Duration::from_secs(30)) => {
      tracing::error!(reason = "timeout", "background task failed")
    }
  }
}
async fn print_status() {
  log::warn!("Requested to print system status.");
  eprintln!(
    "LP_DISPATCH_FAIL_COUNT: {}",
    LP_DISPATCH_FAIL_COUNT.load(Ordering::Relaxed)
  );
  eprintln!(
    "LP_BG_ISSUE_FAIL_COUNT: {}",
    LP_BG_ISSUE_FAIL_COUNT.load(Ordering::Relaxed)
  );
  eprintln!("End of system status.");
}
