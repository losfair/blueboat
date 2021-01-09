use crate::config::*;
use rusty_workers::app::*;
use anyhow::Result;
use arc_swap::ArcSwap;
use futures::StreamExt;
use rand::distributions::{Distribution, Open01, WeightedIndex};
use rand::Rng;
use rusty_workers::rpc::{RuntimeServiceClient, FetchServiceClient};
use rusty_workers::tarpc;
use rusty_workers::types::*;
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock};
use lru_time_cache::LruCache;
use rusty_workers::kv::KvClient;
use tokio::sync::mpsc::{Sender, Receiver};

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

#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error("cannot fetch config")]
    FetchConfig,
}

pub struct Scheduler {
    config: ArcSwap<Config>,
    local_config: LocalConfig,
    worker_config: WorkerConfiguration,
    clients: AsyncRwLock<BTreeMap<RuntimeId, RtState>>,
    apps: AsyncRwLock<BTreeMap<AppId, AppState>>,
    route_cache: AsyncMutex<LruCache<String, BTreeMap<String, RouteCacheEntry>>>, // domain -> (prefix -> appid)
    terminate_queue: tokio::sync::mpsc::Sender<ReadyInstance>,
    fetch_client: FetchServiceClient,
    kv_client: KvClient,
    lookup_route_tx: Sender<(String, String)>,
}

#[derive(Clone)]
struct RouteCacheEntry {
    appid: AppId,
    expire_after: Instant,
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

impl RouteCacheEntry {
    fn is_expired(&self) -> bool {
        Instant::now() > self.expire_after
    }
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
        fetch_client: FetchServiceClient,
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

        let route_cache_lru_ttl_ms = local_config.route_cache_lru_ttl_ms;
        let route_cache_size = local_config.route_cache_size;

        let (lookup_route_tx, lookup_route_rx) = tokio::sync::mpsc::channel(100);
        let me = Arc::new(Self {
            config: ArcSwap::new(Arc::new(Config::default())),
            local_config,
            worker_config,
            clients: AsyncRwLock::new(BTreeMap::new()),
            apps: AsyncRwLock::new(BTreeMap::new()),
            route_cache: AsyncMutex::new(LruCache::with_expiry_duration_and_capacity(
                Duration::from_millis(route_cache_lru_ttl_ms),
                route_cache_size,
            )),
            terminate_queue: terminate_queue_tx,
            fetch_client,
            kv_client,
            lookup_route_tx,
        });
        let me2 = me.clone();
        tokio::spawn(async move { me2.lookup_route_background(lookup_route_rx).await; });
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
                .filter(|x| !x.is_expired())
                .cloned();
            drop(route_cache);

            if appid.is_some() {
                break;
            }

            // Notify the worker thread
            drop(self.lookup_route_tx.try_send((host.clone(), uri.path().to_string())));

            tokio::time::sleep(Duration::from_millis(500)).await;
        };

        let appid = appid.ok_or(SchedError::NoRouteMapping)?.appid;

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
            body: if full_body.len() == 0 {
                None
            } else {
                Some(HttpBody::Binary(full_body))
            },
        };

        let apps = self.apps.read().await;
        let app = apps.get(&appid).ok_or(SchedError::NoRouteMapping)?;

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
                HttpBody::Text(s) => hyper::Body::from(s),
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

    pub async fn check_config_update(&self, url: &str) -> Result<()> {
        let res = self.fetch_client.clone().fetch(tarpc::context::current(), RequestObject {
            url: url.to_string(),
            method: "GET".into(),
            ..Default::default()
        }).await??.map_err(|_| ConfigurationError::FetchConfig)?;
        if res.status != 200 {
            return Err(ConfigurationError::FetchConfig.into());
        }
        let body = match res.body {
            HttpBody::Binary(x) => String::from_utf8(x)?,
            HttpBody::Text(t) => t,
        };
        let config: Config = toml::from_str(&body)?;
        if config != **self.config.load() {
            self.config.store(Arc::new(config));
            self.populate_config().await;
            info!("configuration updated");
        }
        Ok(())
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
        let config = self.config.load();
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
        drop(config);

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

    async fn do_lookup_route_background(&self, domain: String, path: String) {
        // Double check
        let mut route_cache = self.route_cache.lock().await;
        if let Some(x) = route_cache.get(&domain) {
            if let Some(x) = lookup_submappings(&path, x) {
                if !x.is_expired() {
                    return;
                }
            }
        }
        drop(route_cache);

        match self.kv_client.route_mapping_lookup(&domain, &path).await {
            Ok(Some(appid)) => {
                info!("inserting route: {}{} -> {}", domain, path, appid);
                let mut route_cache = self.route_cache.lock().await;
                route_cache.entry(domain).or_insert(BTreeMap::new())
                    .insert(path.to_string(), RouteCacheEntry {
                        appid: AppId(appid),
                        expire_after: Instant::now() + Duration::from_millis(self.local_config.route_cache_ttl_ms),
                    });
            }
            Ok(None) => {
                warn!("lookup_route_background: mapping not found for {}{}", domain, path);
            }
            Err(e) => {
                warn!("error looking up route: {:?}", e);
            }
        }
    }

    async fn populate_config(&self) {
        let config = self.config.load();

        // Update app list.
        let apps = self.apps.read().await;

        let new_apps_config: BTreeMap<AppId, &AppConfig> =
            config.apps.iter().map(|x| (x.id.clone(), x)).collect();

        // Figure out newly added apps
        let mut unseen_appids: BTreeSet<AppId> =
            new_apps_config.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in apps.iter() {
            unseen_appids.remove(k);
        }

        // Release lock.
        drop(apps);

        // Build new apps.
        let mut unseen_apps: Vec<(AppId, AppState)> = vec![];

        // unseen_appids is a subset of keys(new_apps_config) so we can unwrap here
        // Concurrently fetch bundles
        let app_bundles: Vec<_> = unseen_appids
            .iter()
            .map(|id| new_apps_config.get(&id).unwrap().bundle.clone())
            .map(|bundle_url| async move {
                info!("fetching bundle for app {}", bundle_url);
                // TODO: limit body size
                self.fetch_client.clone().fetch(tarpc::context::current(), RequestObject {
                    url: bundle_url,
                    method: "GET".into(),
                    ..Default::default()
                }).await.ok().and_then(|x| x.ok()).and_then(|x| x.ok())
                    .filter(|x| x.status == 200)
                    .map(|x| match x.body {
                        HttpBody::Binary(x) => x,
                        HttpBody::Text(x) => Vec::from(x),
                    })
            })
            .collect();
        let app_bundles = futures::future::join_all(app_bundles).await;

        for (id, fetch_result) in unseen_appids.into_iter().zip(app_bundles.into_iter()) {
            info!("loading app {}", id.0);
            let app_config = new_apps_config.get(&id).unwrap();
            let bundle = match fetch_result {
                Some(x) => x,
                None => {
                    info!("fetch failed: app {} ({})", id.0, app_config.bundle);
                    continue;
                }
            };

            let mut config = self.worker_config.clone();
            config.env = app_config.env.clone();
            config.kv_namespaces = decode_kv_namespaces(&app_config.kv_namespaces);

            let state = AppState {
                id: id.clone(),
                config,
                bundle,
                ready_instances: AsyncMutex::new(VecDeque::new()),
            };
            unseen_apps.push((id, state));
        }

        // Take a write lock.
        let mut apps = self.apps.write().await;

        // Add new apps.
        for (id, state) in unseen_apps {
            apps.insert(id, state);
        }

        // Drop removed apps.
        let mut apps_to_remove = vec![];
        for (k, _) in apps.iter() {
            if !new_apps_config.contains_key(k) {
                apps_to_remove.push(k.clone());
            }
        }
        for k in apps_to_remove {
            apps.remove(&k);
        }

        drop(apps);

        drop(config);

        // Trigger a runtime discovery with new configuration
        self.discover_runtimes().await;

        // ... and query their status.
        self.query_runtimes().await;
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
    namespaces.iter().filter_map(|ns| {
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
    }).collect()
}

fn lookup_submappings<'a>(path: &str, submappings: &'a BTreeMap<String, RouteCacheEntry>) -> Option<&'a RouteCacheEntry> {
    // Match in reverse order.
    for (k, v) in submappings.iter().rev() {
        if path.starts_with(k) {
            return Some(v);
        }
    }
    None
}