use std::convert::Infallible;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc, time::Instant};

use crate::generational_cache::GenerationalCache;
use crate::headers::{
  HDR_GLOBAL_PREFIX, HDR_REQ_CLIENT_CITY, HDR_REQ_CLIENT_COUNTRY, HDR_REQ_CLIENT_IP,
  HDR_REQ_CLIENT_SUBDIVISION_PREFIX, HDR_REQ_CLIENT_WPBL, HDR_REQ_METADATA, HDR_REQ_REQUEST_ID,
  HDR_RES_HANDLE_LATENCY, HDR_RES_REQUEST_ID, PROXY_HEADER_WHITELIST,
};
use crate::ipc::{BlueboatIpcReqV, BlueboatIpcRes};
use crate::lpch::{LogEntry, LowPriorityMsg};
use crate::pm::pm_handle;
use crate::util::KafkaProducerService;
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
use rdkafka::producer::FutureRecord;
use rusoto_core::Region;
use rusoto_s3::{GetObjectRequest, S3Client, S3};
use smr::config::{APP_INACTIVE_TIMEOUT_MS, SPRING_CLEANING_INTERVAL_MS};
use smr::ipc_channel::ipc::IpcSender;
use smr::{ipc_channel::ipc::IpcSharedMemory, scheduler::Scheduler};

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
  #[structopt(short, long)]
  listen: SocketAddr,

  #[structopt(long)]
  s3_bucket: String,

  #[structopt(long)]
  s3_region: String,

  #[structopt(long, default_value = "-")]
  s3_endpoint: String,

  #[structopt(long, default_value = "524288")]
  mem_high_watermark_kb: u64,

  #[structopt(long, default_value = "131072")]
  mem_critical_watermark_kb: u64,

  #[structopt(long, default_value = "-")]
  db: String,

  #[structopt(long, default_value = "-")]
  log_kafka: String,

  #[structopt(long, default_value = "-")]
  mmdb_city: String,

  #[structopt(long, default_value = "-")]
  wpbl_db: String,
}

struct CacheEntry {
  last_use: Mutex<Instant>,
  shm: Arc<IpcSharedMemory>,
}

struct LpContext {
  log_kafka: Option<KafkaProducerService>,
  log_permit: Arc<Semaphore>,
  bg_permit: Arc<Semaphore>,
}

type CacheType = GenerationalCache<Arc<PackageKey>, Arc<RwLock<Option<CacheEntry>>>>;
type MdCacheType = GenerationalCache<Arc<str>, Arc<Metadata>>;

static SCHEDULER: OnceCell<Arc<RwLock<Scheduler<PackageKey, BlueboatIpcReq>>>> =
  OnceCell::const_new();
static S3: OnceCell<(S3Client, String)> = OnceCell::const_new();
static CACHE: OnceCell<CacheType> = OnceCell::const_new();
static MD_CACHE: OnceCell<MdCacheType> = OnceCell::const_new();
static MEM_HIGH_WATERMARK_KB: OnceCell<u64> = OnceCell::const_new();
static MEM_CRITICAL_WATERMARK_KB: OnceCell<u64> = OnceCell::const_new();
static LP_TX: OnceCell<Mutex<IpcSender<LowPriorityMsg>>> = OnceCell::const_new();
static MMDB_CITY: OnceCell<Option<maxminddb::Reader<Mmap>>> = OnceCell::const_new();
static WPBL_DB: OnceCell<Option<WpblDb>> = OnceCell::const_new();

static LP_DISPATCH_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static LP_LOG_ISSUE_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static LP_BG_ISSUE_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);

const MIN_GAP_KB: u64 = 65536;
const WORKER_IDLE_TTL_SECS: u64 = 400;
const CODE_CACHE_SWEEP_INTERVAL_SECS: u64 = WORKER_IDLE_TTL_SECS + 10;

pub fn main() {
  pretty_env_logger::init_timed();
  tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .unwrap()
    .block_on(async_main())
}

pub fn global_scheduler() -> &'static Arc<RwLock<Scheduler<PackageKey, BlueboatIpcReq>>> {
  SCHEDULER.get().unwrap()
}

pub fn s3() -> &'static (S3Client, String) {
  S3.get().unwrap()
}

fn cache() -> &'static CacheType {
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

  fn ss_evict(&self) {
    if matches!(self, MemoryWatermark::Critical) {
      cache().emergency_evict();
    }
  }
}

async fn async_main() {
  let opt = Opt::from_args();

  let s3_client = S3Client::new(if opt.s3_endpoint != "-" {
    Region::Custom {
      name: opt.s3_region.clone(),
      endpoint: opt.s3_endpoint.clone(),
    }
  } else {
    Region::from_str(&opt.s3_region).unwrap()
  });

  S3.set((s3_client, opt.s3_bucket.clone()))
    .unwrap_or_else(|_| unreachable!());

  // Tweak parameters
  SPRING_CLEANING_INTERVAL_MS.store(300);
  APP_INACTIVE_TIMEOUT_MS.store(WORKER_IDLE_TTL_SECS * 1000);

  let scheduler = Scheduler::<PackageKey, _>::new(pm_handle());
  SCHEDULER.set(scheduler).unwrap_or_else(|_| unreachable!());

  CACHE
    .set(GenerationalCache::new("code"))
    .unwrap_or_else(|_| unreachable!());
  MD_CACHE
    .set(GenerationalCache::new("md"))
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
        log::info!("Opened MMDB (city) at {}.", opt.mmdb_city);
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
        log::info!("Opened Wikipedia blocklist DB at {}.", opt.wpbl_db);
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

  // Monitor memory pressure
  std::thread::spawn(|| {
    let mut last_wm: Option<MemoryWatermark> = None;
    loop {
      let (wm, avail_mem) = memory_watermark(last_wm);
      if let Some(last) = last_wm {
        if last != wm {
          log::info!(
            "Memory watermark changed from {:?} to {:?}. {} KB available.",
            last,
            wm,
            avail_mem
          );
        }
      } else {
        log::info!(
          "Memory watermark initialized to {:?}. {} KB available.",
          wm,
          avail_mem
        );
      }
      last_wm = Some(wm);
      wm.tune_smr_parameters();
      wm.ss_evict();
      std::thread::sleep(Duration::from_secs(1));
    }
  });

  tokio::spawn(sweep_md_cache());
  tokio::spawn(sweep_code_cache());
  tokio::spawn(async move {
    let mut sig = tokio::signal::unix::signal(SignalKind::user_defined1()).unwrap();
    log::info!("SIGUSR1 handler registered. Send SIGUSR1 to process {} and system status will be printed to stderr.", std::process::id());
    loop {
      sig.recv().await;
      print_status().await;
    }
  });

  let mut lp_ctx = LpContext {
    log_kafka: None,
    log_permit: Arc::new(Semaphore::new(5000)),
    bg_permit: Arc::new(Semaphore::new(500)),
  };

  // Write logs
  if opt.log_kafka != "-" {
    log::info!("Logging enabled. Logs will be written to the provided kafka cluster.");
    let producer = KafkaProducerService::open(&opt.log_kafka).unwrap();
    lp_ctx.log_kafka = Some(producer);
  }

  spawn_lp_handler(Arc::new(lp_ctx), lp_rx);

  let make_svc = make_service_fn(|_| async move { Ok::<_, hyper::Error>(service_fn(handle)) });

  Server::bind(&opt.listen).serve(make_svc).await.unwrap();
}

async fn handle(mut req: Request<Body>) -> Result<Response<Body>, Infallible> {
  if req.uri().path() == "/_blueboat/health" {
    return Ok(Response::new(Body::from("OK")));
  }

  let md_path = match req.headers().get(HDR_REQ_METADATA) {
    Some(x) => x.to_str().unwrap_or("").to_string(),
    None => {
      let mut res = Response::new(Body::empty());
      *res.status_mut() = StatusCode::BAD_REQUEST;
      return Ok(res);
    }
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
      log::error!(
        "early runtime error (app {}): {:?}",
        serde_json::to_string(&md_path).unwrap(),
        e
      );
      let mut res = Response::new(Body::empty());
      *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
      Ok(res)
    }
  }
}

async fn sweep_md_cache() -> ! {
  loop {
    tokio::time::sleep(Duration::from_secs(30)).await;
    md_cache()
      .sweep(|k, orig| async move {
        match load_md(&*k).await {
          Ok(x) => Some(x),
          Err(e) => {
            log::error!("sweep_md_cache: md {:?} load error: {:?}", k, e);
            Some(orig)
          }
        }
      })
      .await;
  }
}

async fn sweep_code_cache() -> ! {
  loop {
    tokio::time::sleep(Duration::from_secs(CODE_CACHE_SWEEP_INTERVAL_SECS)).await;
    cache()
      .sweep(|_, v| async move {
        // Code cache entries are immutable.
        Some(v)
      })
      .await;
  }
}

async fn load_md(path: &str) -> Result<Arc<Metadata>> {
  #[derive(Error, Debug)]
  #[error("metadata error")]
  struct MetadataError;

  let (s3c, bucket) = s3();
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
  Ok(Arc::new(serde_json::from_slice(&body)?))
}

async fn generic_invoke(
  req: BlueboatIpcReq,
  md_path: &str,
  version_assertion: Option<&str>,
) -> Result<BlueboatIpcRes> {
  #[derive(Error, Debug)]
  #[error("version changed")]
  struct VersionChanged;

  let md = md_cache().get(md_path);
  let md = if let Some(md) = md {
    md
  } else {
    let md = load_md(md_path).await?;
    md_cache().insert(Arc::from(md_path), md.clone());
    md
  };
  if let Some(version_assertion) = version_assertion {
    if md.version != version_assertion {
      return Err(VersionChanged.into());
    }
  }

  let pk = PackageKey {
    path: md_path.to_string(),
    version: md.version.clone(),
  };
  let package = load_package(&pk, &md).await?;
  let pk2 = pk.clone();
  let w = Scheduler::get_worker(global_scheduler(), &pk, move || BlueboatInitData {
    key: pk2.clone(),
    package: (*package).clone(),
    metadata: (*md).clone(),
    lp_tx: LP_TX.get().unwrap().lock().clone(),
  })
  .await?;

  let res = w.invoke(req).await?;

  Ok(res)
}

async fn raw_handle(mut req: Request<Body>, md_path: &str) -> Result<Response<Body>> {
  #[derive(Error, Debug)]
  #[error("metadata error")]
  struct MetadataError;

  let handle_start = Instant::now();
  let request_id = req
    .headers()
    .get(HDR_REQ_REQUEST_ID)
    .and_then(|x| x.to_str().ok())
    .map(|x| x.to_string())
    .unwrap_or_else(|| format!("u:{}", Uuid::new_v4().to_string()));
  req
    .headers_mut()
    .insert(HDR_REQ_REQUEST_ID, HeaderValue::from_str(&request_id)?);
  let request = BlueboatRequest::from_hyper(req).await?;
  let request = BlueboatIpcReq {
    v: BlueboatIpcReqV::Http(request),
    id: request_id.clone(),
  };
  let res = generic_invoke(request, md_path, None).await;
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

async fn load_package(pk: &PackageKey, md: &Metadata) -> Result<Arc<IpcSharedMemory>> {
  #[derive(Error, Debug)]
  #[error("package unavailable")]
  struct Unavailable;

  // Don't attempt to load the same resource from multiple tasks concurrently.
  let (entry, write_owner) = {
    let mut cache = cache().lock();
    if let Some(entry) = cache.get(pk) {
      (entry.clone(), None)
    } else {
      let m = Arc::new(RwLock::new(None));
      let w = m.clone().try_write_owned().unwrap();
      cache.insert(Arc::new(pk.clone()), m.clone());
      (m, Some(w))
    }
  };

  let shm = if let Some(mut write_owner) = write_owner {
    struct Guard<'a>(&'a PackageKey);
    impl<'a> Drop for Guard<'a> {
      fn drop(&mut self) {
        cache().remove(self.0);
      }
    }

    // If the future is cancelled or fetch failed, remove the entry from the cache.
    let g = Guard(&pk);

    let shm = fetch_package(md).await?;
    *write_owner = Some(CacheEntry {
      last_use: Mutex::new(Instant::now()),
      shm: shm.clone(),
    });
    std::mem::forget(g);
    shm
  } else {
    let entry = entry.read().await;
    let entry = entry.as_ref().ok_or_else(|| Unavailable)?;
    if let Some(mut g) = entry.last_use.try_lock() {
      *g = Instant::now();
    }
    entry.shm.clone()
  };

  Ok(shm)
}

async fn fetch_package(md: &Metadata) -> Result<Arc<IpcSharedMemory>> {
  #[derive(Error, Debug)]
  #[error("missing package for this metadata")]
  struct MissingPackage;

  let (s3c, bucket) = s3();
  let output = s3c
    .get_object(GetObjectRequest {
      bucket: bucket.clone(),
      key: md.package.clone(),
      ..Default::default()
    })
    .await?;
  let mut body: Vec<u8> = vec![];
  output
    .body
    .ok_or(MissingPackage)?
    .into_async_read()
    .read_to_end(&mut body)
    .await?;
  Ok(Arc::new(IpcSharedMemory::from_bytes(&body)))
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
  let (fail_counter, sem) = match msg {
    LowPriorityMsg::Log(_) => (&LP_LOG_ISSUE_FAIL_COUNT, &ctx.log_permit),
    LowPriorityMsg::Background(_) => (&LP_BG_ISSUE_FAIL_COUNT, &ctx.bg_permit),
  };
  match sem.clone().try_acquire_owned() {
    Ok(permit) => {
      let ctx = ctx.clone();
      tokio::spawn(async move {
        handle_lp(&ctx, msg).await;
        drop(permit);
      });
    }
    Err(_) => {
      fail_counter.fetch_add(1, Ordering::Relaxed);
    }
  }
}

async fn handle_lp(ctx: &Arc<LpContext>, msg: LowPriorityMsg) {
  match msg {
    LowPriorityMsg::Log(entry) => {
      if let Some(producer) = &ctx.log_kafka {
        if let Err(e) = write_log(producer, entry).await {
          log::debug!("write_log failed: {:?}", e);
        }
      }
    }
    LowPriorityMsg::Background(entry) => {
      let request_id = format!(
        "{}+bg-{}",
        entry.request_id.split("+").next().unwrap(),
        Uuid::new_v4().to_string()
      );
      let req = BlueboatIpcReq {
        v: BlueboatIpcReqV::Background(entry.wire_bytes),
        id: request_id,
      };
      let app = entry.app;
      let fut = async {
        match generic_invoke(req, &app.path, Some(app.version.as_str())).await {
          Ok(_) => {}
          Err(e) => {
            log::warn!("background invoke failed (app {}): {:?}", app, e);
          }
        }
      };
      tokio::select! {
        _ = fut => {}
        _ = tokio::time::sleep(Duration::from_secs(30)) => {
          log::warn!("background invoke timed out (app {})", app);
        }
      }
    }
  }
}

async fn write_log(producer: &KafkaProducerService, entry: LogEntry) -> Result<()> {
  let entry = serde_json::to_string(&entry)?;
  producer
    .producer
    .send(
      FutureRecord {
        topic: &producer.topic,
        partition: Some(producer.partition),
        payload: Some(entry.as_str()),
        key: None::<&[u8]>,
        timestamp: None,
        headers: None,
      },
      rdkafka::util::Timeout::Never,
    )
    .await
    .map_err(|(e, _)| e)?;
  Ok(())
}

async fn print_status() {
  log::info!("Requested to print system status.");
  let md_cache_stats = md_cache().stats();
  eprintln!("[md_cache]\n{}", md_cache_stats);
  let code_cache_stats = cache().stats();
  eprintln!("[code_cache]\n{}", code_cache_stats);
  eprintln!(
    "LP_DISPATCH_FAIL_COUNT: {}",
    LP_DISPATCH_FAIL_COUNT.load(Ordering::Relaxed)
  );
  eprintln!(
    "LP_BG_ISSUE_FAIL_COUNT: {}",
    LP_BG_ISSUE_FAIL_COUNT.load(Ordering::Relaxed)
  );
  eprintln!(
    "LP_LOG_ISSUE_FAIL_COUNT: {}",
    LP_LOG_ISSUE_FAIL_COUNT.load(Ordering::Relaxed)
  );
  log::info!("End of system status.");
}
