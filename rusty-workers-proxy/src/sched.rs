use crate::config::*;
use anyhow::Result;
use futures::StreamExt;
use lru_time_cache::LruCache;
use rand::distributions::{Distribution, Open01, WeightedIndex};
use rand::Rng;
use rusty_workers::app::*;
use rusty_workers::kv::KvClient;
use rusty_workers::rpc::RuntimeServiceClient;
use rusty_workers::tarpc;
use rusty_workers::types::*;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock};

#[derive(Debug, Error)]
pub enum SchedError {
    #[error("no available instance")]
    NoAvailableInstance,

    #[error("no route mapping found")]
    NoRouteMapping,

    #[error("request body too large")]
    RequestBodyTooLarge,

    #[error("request failed after retries")]
    RequestFailedAfterRetries,
}

pub struct Scheduler {
    local_config: LocalConfig,
    worker_config: WorkerConfiguration,
    clients: AsyncRwLock<BTreeMap<RuntimeId, RtState>>,
    apps: AsyncMutex<LruCache<AppId, Arc<AppState>>>,
    route_cache: AsyncMutex<LruCache<String, BTreeMap<String, AppId>>>, // domain -> (prefix -> appid)
    terminate_queue: tokio::sync::mpsc::Sender<ReadyInstance>,
    kv_client: KvClient,
    lookup_route_tx: Sender<(String, String)>,
    lookup_app_tx: Sender<AppId>,
}

/// State of a backing runtime.
#[derive(Clone)]
struct RtState {
    /// The client.
    client: RuntimeServiceClient,

    /// Load.
    load: Arc<AtomicU16>,
}

/// Scheduling state of an app.
struct AppState {
    /// Identifier of this app.
    id: AppId,

    /// App configuration.
    config: WorkerConfiguration,

    /// Hash of the bundle.
    bundle_id: [u8; 16],

    /// File bundle.
    bundle: Vec<u8>,

    /// Instances that are ready to run this app.
    ready_instances: AsyncMutex<VecDeque<ReadyInstance>>,
}

/// State of an instance ready for an app.
#[derive(Clone)]
struct ReadyInstance {
    /// Identifier of this runtime.
    rtid: RuntimeId,

    /// Last active time.
    last_active: Instant,

    // Worker handle.
    handle: WorkerHandle,

    /// The tarpc client.
    client: RuntimeServiceClient,
}

impl ReadyInstance {
    /// Returns whether the instance is usable.
    ///
    /// A instance is no longer usable when `current_time - last_active > config.ready_instance_expiration_ms`.
    fn is_usable(&self, scheduler: &Scheduler) -> bool {
        let current = Instant::now();
        if current.duration_since(self.last_active)
            > Duration::from_millis(scheduler.local_config.ready_instance_expiration_ms)
        {
            false
        } else {
            true
        }
    }

    /// Updates last_active time.
    fn update_last_active(&mut self) {
        self.last_active = Instant::now();
    }
}

impl AppState {
    async fn gc_ready_instances(&self, scheduler: &Scheduler) {
        let mut ready = self.ready_instances.lock().await;
        while ready.len() > scheduler.local_config.max_ready_instances_per_app {
            drop(
                scheduler
                    .terminate_queue
                    .try_send(ready.pop_front().unwrap()),
            );
        }

        while let Some(x) = ready.front() {
            if !x.is_usable(scheduler) {
                drop(
                    scheduler
                        .terminate_queue
                        .try_send(ready.pop_front().unwrap()),
                );
            } else {
                break;
            }
        }
    }

    async fn pool_instance(&self, scheduler: &Scheduler, inst: ReadyInstance) {
        if rand::thread_rng().sample::<f32, _>(Open01) > scheduler.local_config.dropout_rate {
            self.ready_instances.lock().await.push_back(inst);
        } else {
            // Dropped out. Let's terminate it.
            drop(scheduler.terminate_queue.try_send(inst));
        }

        self.gc_ready_instances(scheduler).await;
    }

    async fn get_instance(&self, scheduler: &Scheduler) -> Result<ReadyInstance> {
        self.gc_ready_instances(scheduler).await;
        if let Some(mut inst) = self.ready_instances.lock().await.pop_front() {
            inst.update_last_active();
            return Ok(inst);
        }

        let clients = scheduler.clients.read().await;

        // No cached instance now. Create one.
        if clients.len() == 0 {
            return Err(SchedError::NoAvailableInstance.into());
        }
        let all_clients: Vec<(&RuntimeId, &RtState)> = clients.iter().collect();

        // TODO: Overflow?
        let distribution = WeightedIndex::new(
            all_clients
                .iter()
                .map(|x| (u16::MAX - x.1.load.load(Ordering::Relaxed)) as u32 + 10000),
        )
        .expect("WeightedIndex::new failed");
        let index = distribution.sample(&mut rand::thread_rng());
        let (rtid, rt) = all_clients[index];

        info!(
            "spawning new worker for app {} on runtime {} with load {}",
            self.id.0,
            rtid.0,
            rt.load.load(Ordering::Relaxed) as f64 / std::u16::MAX as f64
        );

        let rtid = rtid.clone();
        let mut client = rt.client.clone();
        let handle = client
            .spawn_worker(
                tarpc::context::current(),
                self.id.0.clone(),
                self.config.clone(),
                self.bundle.clone(),
            )
            .await??;
        Ok(ReadyInstance {
            rtid,
            last_active: Instant::now(),
            handle,
            client,
        })
    }
}

impl Scheduler {
    pub fn new(
        worker_config: WorkerConfiguration,
        local_config: LocalConfig,
        kv_client: KvClient,
    ) -> Arc<Self> {
        let (terminate_queue_tx, mut terminate_queue_rx): (
            tokio::sync::mpsc::Sender<ReadyInstance>,
            _,
        ) = tokio::sync::mpsc::channel(1000);

        tokio::spawn(async move {
            loop {
                if let Some(mut inst) = terminate_queue_rx.recv().await {
                    let mut ctx = tarpc::context::current();

                    // This isn't critical so don't wait too long
                    ctx.deadline = std::time::SystemTime::now() + Duration::from_secs(1);

                    let res = inst.client.terminate_worker(ctx, inst.handle.clone()).await;
                    info!(
                        "terminate_worker {}, instance {}, result = {:?}",
                        inst.handle.id, inst.rtid.0, res
                    );
                } else {
                    break;
                }
            }
        });
        let route_cache_size = local_config.route_cache_size;
        let app_cache_size = local_config.app_cache_size;

        let (lookup_route_tx, lookup_route_rx) = tokio::sync::mpsc::channel(100);
        let (lookup_app_tx, lookup_app_rx) = tokio::sync::mpsc::channel(100);
        let me = Arc::new(Self {
            local_config,
            worker_config,
            clients: AsyncRwLock::new(BTreeMap::new()),
            apps: AsyncMutex::new(LruCache::with_capacity(app_cache_size)),
            route_cache: AsyncMutex::new(LruCache::with_capacity(route_cache_size)),
            terminate_queue: terminate_queue_tx,
            kv_client,
            lookup_route_tx,
            lookup_app_tx,
        });
        let me2 = me.clone();
        let me3 = me.clone();
        let me4 = me.clone();
        let me5 = me.clone();
        tokio::spawn(async move {
            me2.lookup_route_background(lookup_route_rx).await;
        });
        tokio::spawn(async move {
            me3.lookup_app_background(lookup_app_rx).await;
        });
        tokio::spawn(async move {
            me4.apps_gc_task().await;
        });
        tokio::spawn(async move {
            me5.route_cache_gc_task().await;
        });
        me
    }

    pub async fn handle_request(
        &self,
        mut req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>> {
        // Rewrite host to remove port.
        let host = req
            .headers()
            .get("host")
            .and_then(|x| x.to_str().ok())
            .unwrap_or("")
            .split(":")
            .nth(0)
            .unwrap()
            .to_string();
        trace!("host: {}", host);
        req.headers_mut().insert(
            "host",
            hyper::header::HeaderValue::from_bytes(host.as_bytes())?,
        );

        let uri = req.uri().clone();

        let mut appid = None;

        for _ in 0..3 {
            let mut route_cache = self.route_cache.lock().await;
            appid = route_cache
                .get(&host)
                .and_then(|submappings| lookup_submappings(uri.path(), submappings))
                .cloned();
            drop(route_cache);

            if appid.is_some() {
                break;
            }

            // Notify the worker thread
            drop(
                self.lookup_route_tx
                    .try_send((host.clone(), uri.path().to_string())),
            );

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let appid = appid.ok_or(SchedError::NoRouteMapping)?;

        let method = req.method().as_str().to_string();
        let mut headers = BTreeMap::new();
        let url = format!("https://{}{}", host.split(":").nth(0).unwrap(), uri); // TODO: detect https
        let mut full_body = vec![];

        for (k, v) in req.headers() {
            headers
                .entry(k.as_str().to_string())
                .or_insert(vec![])
                .push(v.to_str()?.to_string());
        }

        let mut body_error: Result<()> = Ok(());
        req.into_body()
            .for_each(|bytes| {
                match bytes {
                    Ok(x) => {
                        if full_body.len() + x.len()
                            > self.local_config.max_request_body_size_bytes as usize
                        {
                            body_error = Err(SchedError::RequestBodyTooLarge.into());
                        }
                        full_body.extend_from_slice(&x);
                    }
                    Err(e) => {
                        body_error = Err(e.into());
                    }
                };
                futures::future::ready(())
            })
            .await;

        body_error?;

        let target_req = RequestObject {
            headers,
            method,
            url,
            body: HttpBody::Binary(full_body),
        };

        let mut app = None;

        for _ in 0..3 {
            app = self.apps.lock().await.get(&appid).cloned();
            if app.is_some() {
                break;
            }

            // Notify the worker thread
            drop(self.lookup_app_tx.try_send(appid.clone()));

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let app = app.ok_or(SchedError::NoRouteMapping)?;

        // Backend retries.
        for _ in 0..3usize {
            let mut instance = app.get_instance(self).await?;
            debug!(
                "routing request {}{} to app {}, instance {}",
                host, uri, appid.0, instance.rtid.0
            );

            let mut fetch_context = tarpc::context::current();
            fetch_context.deadline = std::time::SystemTime::now()
                + Duration::from_millis(self.local_config.request_timeout_ms);

            let fetch_res = instance
                .client
                .fetch(fetch_context, instance.handle.clone(), target_req.clone())
                .await;
            let fetch_res = match fetch_res {
                Ok(x) => x,
                Err(e) => {
                    // Network error. Drop this and select another instance.
                    self.clients.write().await.remove(&instance.rtid);
                    info!("network error for instance {}: {:?}", instance.rtid.0, e);
                    continue;
                }
            };
            let fetch_res = match fetch_res {
                Ok(x) => x,
                Err(e) => {
                    debug!("backend returns error: {:?}", e);

                    // Don't pool it back.
                    // Runtime would give us a 500 instead of an error when it is recoverable.
                    match e {
                        ExecutionError::NoSuchWorker => {
                            // Backend terminated our worker "unexpectedly".
                            // Re-select another instance.
                            continue;
                        }
                        _ => {
                            info!("execution error: {:?}", e);
                            // Don't attempt to recover otherwise.
                            // Pool it back if possible.
                            if !e.terminates_worker() {
                                app.pool_instance(self, instance).await;
                            }
                            break;
                        }
                    }
                }
            };

            // Pool it back.
            app.pool_instance(self, instance).await;

            // Build response.
            let mut res = hyper::Response::new(match fetch_res.body {
                HttpBody::Binary(bytes) => hyper::Body::from(bytes),
            });

            *res.status_mut() = hyper::StatusCode::from_u16(fetch_res.status)?;
            for (k, values) in fetch_res.headers {
                for v in values {
                    res.headers_mut().append(
                        hyper::header::HeaderName::from_bytes(k.as_bytes())?,
                        hyper::header::HeaderValue::from_bytes(v.as_bytes())?,
                    );
                }
            }

            return Ok(res);
        }

        Err(SchedError::RequestFailedAfterRetries.into())
    }

    /// Query each runtime for its health/load status, etc.
    pub async fn query_runtimes(&self) {
        let mut to_drop = vec![];
        let clients = self.clients.read().await;
        for (rtid, rt) in clients.iter() {
            if let Ok(Ok(load)) = rt.client.clone().load(tarpc::context::current()).await {
                let load_float = (load as f64) / (u16::MAX as f64);
                debug!("updating load for backend {}: {}", rtid.0, load_float);
                rt.load.store(load, Ordering::Relaxed);
            } else {
                // Something is wrong. Drop it.
                to_drop.push(rtid.clone());
            }
        }
        drop(clients);

        // Remove all clients that don't respond to our load query.
        if to_drop.len() > 0 {
            let mut clients = self.clients.write().await;
            for rtid in to_drop {
                info!("dropping backend {}", rtid.0);
                clients.remove(&rtid);
            }
        }
    }

    /// Discover new runtimes behind each specified address. (with load balancing)
    pub async fn discover_runtimes(&self) {
        let new_clients = self
            .local_config
            .runtime_cluster
            .iter()
            .map(|addr| async move {
                match RuntimeServiceClient::connect_noretry(addr).await {
                    Ok(mut client) => match client.id(tarpc::context::current()).await {
                        Ok(id) => Some((id, client)),
                        Err(e) => {
                            info!("cannot fetch id from backend {:?}: {:?}", addr, e);
                            None
                        }
                    },
                    Err(e) => {
                        info!("cannot connect to backend {:?}: {:?}", addr, e);
                        None
                    }
                }
            });
        let new_clients: Vec<Option<(RuntimeId, RuntimeServiceClient)>> =
            futures::future::join_all(new_clients).await;

        let mut clients = self.clients.write().await;
        for item in new_clients {
            if let Some((id, client)) = item {
                if !clients.contains_key(&id) {
                    info!("discovered new backend: {}", id.0);
                    clients.insert(
                        id,
                        RtState {
                            client,
                            load: Arc::new(AtomicU16::new(0)),
                        },
                    );
                }
            }
        }
        drop(clients);
    }

    async fn lookup_route_background(&self, mut rx: Receiver<(String, String)>) {
        loop {
            let (domain, path) = match rx.recv().await {
                Some(x) => x,
                None => return,
            };
            self.do_lookup_route_background(domain, path).await;

            // Don't stress kv too much
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn do_lookup_route_background(&self, domain: String, _path: String) {
        // Double check
        let mut route_cache = self.route_cache.lock().await;
        if let Some(_) = route_cache.get(&domain) {
            return;
        }
        drop(route_cache);

        match self
            .kv_client
            .route_mapping_list_for_domain(&domain, |_| true)
            .await
        {
            Ok(map) => {
                let map: BTreeMap<_, _> = map.into_iter().map(|(k, v)| (k, AppId(v))).collect();
                info!(
                    "do_lookup_route_background: updating routing for domain {}: {:?}",
                    domain, map
                );
                self.route_cache.lock().await.insert(domain, map);
            }
            Err(e) => {
                warn!(
                    "do_lookup_route_background: error looking up route for domain {}: {:?}",
                    domain, e
                );
            }
        }
    }

    async fn route_cache_gc_task(&self) {
        loop {
            let domains: Vec<String> = self
                .route_cache
                .lock()
                .await
                .peek_iter()
                .map(|x| x.0.clone())
                .collect();
            for domain in domains {
                match self
                    .kv_client
                    .route_mapping_list_for_domain(&domain, |_| true)
                    .await
                {
                    Ok(map) => {
                        let map: BTreeMap<_, _> =
                            map.into_iter().map(|(k, v)| (k, AppId(v))).collect();
                        let mut route_cache = self.route_cache.lock().await;
                        if let Some(entry) = route_cache.peek(&domain) {
                            if entry != &map {
                                info!("route for domain {} changed. flushing cache", domain);
                                // Remove instead of update to prevent updating LRU timestamp.
                                // TODO: patch LruCache.
                                route_cache.remove(&domain);
                            }
                        }
                    }
                    Err(_) => {}
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    async fn apps_gc_task(&self) {
        loop {
            let apps: Vec<(AppId, ([u8; 16], WorkerConfiguration))> = self
                .apps
                .lock()
                .await
                .iter()
                .map(|(k, v)| (k.clone(), (v.bundle_id, v.config.clone())))
                .collect();

            for (id, (bundle_id, worker_config)) in apps {
                match self.kv_client.app_metadata_get(&id.0).await {
                    Ok(Some(config)) => {
                        if let Ok(config) = serde_json::from_slice::<AppConfig>(&config) {
                            if let Ok(expected_bundle_id) = base64::decode(&config.bundle_id) {
                                if expected_bundle_id != bundle_id
                                    || config.env != worker_config.env
                                    || decode_kv_namespaces(&config.kv_namespaces)
                                        != worker_config.kv_namespaces
                                {
                                    info!("app changed. removing app {} from cache", id.0);
                                    self.apps.lock().await.remove(&id);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        info!("app deleted. removing app {} from cache", id.0);
                        self.apps.lock().await.remove(&id);
                    }
                    _ => {}
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    async fn lookup_app_background(&self, mut rx: Receiver<AppId>) {
        loop {
            let appid = match rx.recv().await {
                Some(x) => x,
                None => return,
            };
            self.do_lookup_app_background(appid).await;

            // Don't stress kv too much
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn do_lookup_app_background(&self, id: AppId) {
        // TODO: Periodically query the database for no-longer-valid apps.

        // Double check
        if self.apps.lock().await.get(&id).is_some() {
            return;
        }

        let config: AppConfig = match self.kv_client.app_metadata_get(&id.0).await {
            Ok(Some(metadata)) => match serde_json::from_slice(&metadata) {
                Ok(x) => x,
                Err(e) => {
                    error!("cannot decode app metadata for app {}: {:?}", id.0, e);
                    return;
                }
            },
            Ok(None) => {
                warn!("do_lookup_app_background: app {} not found", id.0);
                return;
            }
            Err(e) => {
                warn!(
                    "do_lookup_app_background: error fetching app metadata for {}: {:?}",
                    id.0, e
                );
                return;
            }
        };

        let bundle_id = match decode_id128(&config.bundle_id) {
            Some(x) => x,
            None => {
                warn!("do_lookup_app_background: bad bundle hash (app {})", id.0);
                return;
            }
        };

        info!("fetching bundle {}", base64::encode(&bundle_id));
        let bundle = match self.kv_client.app_bundle_get(&bundle_id).await {
            Ok(Some(x)) => x,
            Ok(None) => {
                warn!("do_lookup_app_background: app bundle not found");
                return;
            }
            Err(e) => {
                warn!("do_lookup_app_background: db error: {:?}", e);
                return;
            }
        };

        let mut target_config = self.worker_config.clone();
        target_config.env = config.env.clone();
        target_config.kv_namespaces = decode_kv_namespaces(&config.kv_namespaces);

        let state = AppState {
            id: id.clone(),
            config: target_config,
            bundle_id,
            bundle,
            ready_instances: AsyncMutex::new(VecDeque::new()),
        };

        info!("inserting app: {}", id.0);
        self.apps.lock().await.insert(id, Arc::new(state));
    }
}

impl SchedError {
    pub fn build_response(&self) -> hyper::Response<hyper::Body> {
        let status = match self {
            SchedError::NoAvailableInstance => hyper::StatusCode::SERVICE_UNAVAILABLE,
            SchedError::NoRouteMapping => hyper::StatusCode::BAD_GATEWAY,
            SchedError::RequestBodyTooLarge => hyper::StatusCode::PAYLOAD_TOO_LARGE,
            SchedError::RequestFailedAfterRetries => hyper::StatusCode::SERVICE_UNAVAILABLE,
        };
        let mut res = hyper::Response::new(hyper::Body::from(
            status.canonical_reason().unwrap_or("unknown error"),
        ));
        *res.status_mut() = status;
        res
    }
}

fn decode_kv_namespaces(namespaces: &Vec<KvNamespaceConfig>) -> BTreeMap<String, [u8; 16]> {
    namespaces
        .iter()
        .filter_map(|ns| {
            base64::decode(&ns.id)
                .ok()
                .filter(|x| x.len() == 16)
                .map(|x| {
                    let mut slice = [0u8; 16];
                    slice.copy_from_slice(&x);
                    (ns.name.clone(), slice)
                })
                .or_else(|| {
                    warn!("decode_kv_namespaces: bad value for namespace: {}", ns.name);
                    None
                })
        })
        .collect()
}

fn lookup_submappings<'a>(
    path: &str,
    submappings: &'a BTreeMap<String, AppId>,
) -> Option<&'a AppId> {
    // Match in reverse order.
    for (k, v) in submappings.iter().rev() {
        if path.starts_with(k) {
            return Some(v);
        }
    }
    None
}
