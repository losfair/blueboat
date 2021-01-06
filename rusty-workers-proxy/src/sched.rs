use crate::config::*;
use rusty_workers::types::*;
use rusty_workers::rpc::RuntimeServiceClient;
use anyhow::Result;
use std::collections::{BTreeSet, BTreeMap};
use std::collections::VecDeque;
use std::time::{Instant, Duration};
use thiserror::Error;
use rusty_workers::tarpc;
use tokio::sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock};
use std::net::SocketAddr;
use std::sync::Arc;
use rand::Rng;
use futures::StreamExt;
use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicU16, Ordering};

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
    worker_config: WorkerConfiguration,
    clients: AsyncRwLock<BTreeMap<SocketAddr, RtState>>,
    apps: AsyncRwLock<BTreeMap<AppId, AppState>>,
    route_mappings: AsyncRwLock<BTreeMap<String, BTreeMap<String, AppId>>>, // domain -> (prefix -> appid)
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

    /// Code.
    script: String,

    /// Instances that are ready to run this app.
    ready_instances: AsyncMutex<VecDeque<ReadyInstance>>,
}

/// State of an instance ready for an app.
#[derive(Clone)]
struct ReadyInstance {
    /// Address of the runtime.
    addr: SocketAddr,

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
    /// A instance is no longer usable when `current_time - last_active > config.instance_expiration_time_ms`.
    fn is_usable(&self, config: &Config) -> bool {
        let current = Instant::now();
        if current.duration_since(self.last_active) > Duration::from_millis(config.instance_expiration_time_ms) {
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
    async fn pool_instance(&self, inst: ReadyInstance) {
        self.ready_instances.lock().await.push_back(inst);
    }

    async fn get_instance(&self, config: &Config, clients: &AsyncRwLock<BTreeMap<SocketAddr, RtState>>) -> Result<ReadyInstance> {
        let mut ready_instances = self.ready_instances.lock().await;
        while let Some(mut instance) = ready_instances.pop_front() {
            // TODO: Maintain load data for each client and select based on load.
            if instance.is_usable(config) {
                instance.update_last_active();
                return Ok(instance);
            }
        }
        drop(ready_instances);

        let clients = clients.read().await;

        // No cached instance now. Create one.
        let (&addr, mut rt) = clients.iter()
            .min_by_key(|x| x.1.load.load(Ordering::Relaxed))
            .ok_or(SchedError::NoAvailableInstance)?;

        info!("spawning new worker for app {} on runtime {:?} with load {}", self.id.0, addr, rt.load.load(Ordering::Relaxed));

        let mut client = rt.client.clone();
        let handle = client.spawn_worker(
            tarpc::context::current(),
            self.id.0.clone(),
            self.config.clone(),
            self.script.clone()
        ).await??;
        Ok(ReadyInstance {
            addr,
            last_active: Instant::now(),
            handle,
            client,
        })
    }
}

impl Scheduler {
    pub fn new(worker_config: WorkerConfiguration) -> Self {
        Self {
            config: ArcSwap::new(Arc::new(Config::default())),
            worker_config,
            clients: AsyncRwLock::new(BTreeMap::new()),
            apps: AsyncRwLock::new(BTreeMap::new()),
            route_mappings: AsyncRwLock::new(BTreeMap::new()),
        }
    }

    pub async fn handle_request(&self, mut req: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>> {
        let route_mappings = self.route_mappings.read().await;

        // Rewrite host to remove port.
        let host = req.headers().get("host").and_then(|x| x.to_str().ok()).unwrap_or("").split(":").nth(0).unwrap().to_string();
        debug!("host: {}", host);
        req.headers_mut().insert("host", hyper::header::HeaderValue::from_bytes(host.as_bytes())?);

        let uri = req.uri().clone();
        let submappings = route_mappings.get(&host).ok_or(SchedError::NoRouteMapping)?;

        // Match in reverse order.
        let mut appid = None;
        for (k, v) in submappings.iter().rev() {
            if uri.path().starts_with(k) {
                appid = Some(v.clone());
                break;
            }
        }
        drop(route_mappings);

        let appid = appid.ok_or(SchedError::NoRouteMapping)?;

        let method = req.method().as_str().to_string();
        let mut headers = BTreeMap::new();
        let url = format!("https://{}{}", host.split(":").nth(0).unwrap(), uri); // TODO: detect https
        let mut full_body = vec![];

        for (k, v) in req.headers() {
            headers.entry(k.as_str().to_string()).or_insert(vec![]).push(v.to_str()?.to_string());
        }

        let mut body_error: Result<()> = Ok(());
        let config = self.config.load();
        req.into_body().for_each(|bytes| {
            match bytes {
                Ok(x) => {
                    if full_body.len() + x.len() > config.max_request_body_size_bytes as usize {
                        body_error = Err(SchedError::RequestBodyTooLarge.into());
                    }
                    full_body.extend_from_slice(&x);
                }
                Err(e) => {
                    body_error = Err(e.into());
                }
            };
            futures::future::ready(())
        }).await;

        body_error?;

        let target_req = RequestObject {
            headers,
            method,
            url,
            body: if full_body.len() == 0 { None } else { Some(HttpBody::Binary(full_body)) },
        };

        let apps = self.apps.read().await;
        let mut app = apps.get(&appid).ok_or(SchedError::NoRouteMapping)?;

        let config = self.config.load();

        // Backend retries.
        for _ in 0..3usize {
            let mut instance = app.get_instance(&config, &self.clients).await?;
            info!("routing request {}{} to app {}, instance {:?}", host, uri, appid.0, instance.addr);
    
            let mut fetch_context = tarpc::context::current();
            fetch_context.deadline = std::time::SystemTime::now() + Duration::from_millis(config.request_timeout_ms);

            let fetch_res = instance.client.fetch(fetch_context, instance.handle.clone(), target_req.clone())
                .await?; // Don't retry in case of network errors.
            let fetch_res = match fetch_res {
                Ok(x) => x,
                Err(e) => {
                    debug!("backend returns error: {:?}", e);

                    // Don't pool it back.
                    // Runtime would give us a 500 instead of an error when it is recoverable.
                    match e {
                        GenericError::NoSuchWorker => {
                            // Backend terminated our worker.
                            // Re-select another instance.
                            continue;
                        }
                        _ => {
                            // Don't attempt to recover otherwise.
                            break;
                        }
                    }
                }
            };

            // Pool it back.
            app.pool_instance(instance).await;

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

    pub async fn check_config_update(&self, url: &str, runtime_cluster_append: &BTreeSet<SocketAddr>) -> Result<()> {
        let res = reqwest::get(url)
            .await?;
        if !res.status().is_success() {
            return Err(ConfigurationError::FetchConfig.into());
        }
        let body = res.text().await?;
        let mut config: Config = toml::from_str(&body)?;
        for addr in runtime_cluster_append.iter() {
            config.runtime_cluster.insert(*addr);
        }
        if config != **self.config.load() {
            self.config.store(Arc::new(config));
            self.populate_config().await;
            info!("configuration updated");
        }
        Ok(())
    }

    pub async fn query_runtime_loads(&self) {
        let clients = self.clients.read().await;
        for (addr, rt) in clients.iter() {
            if let Ok(Ok(load)) = rt.client.clone().load(tarpc::context::current()).await {
                let load_float = (load as f64) / (u16::MAX as f64);
                info!("updating load for backend {:?}: {}", addr, load_float);
                rt.load.store(load, Ordering::Relaxed);
            }
        }
    }

    async fn populate_config(&self) {
        let config = self.config.load();

        // TODO: Finer-grained locking
        let mut clients = self.clients.write().await;

        // Add new clients.
        for service_addr in config.runtime_cluster.iter() {
            if !clients.contains_key(service_addr) {
                match RuntimeServiceClient::connect(*service_addr).await {
                    Ok(client) => {
                        clients.insert(*service_addr, RtState {
                            client,
                            load: Arc::new(AtomicU16::new(0)),
                        });
                    },
                    Err(e) => {
                        error!("populate_config: cannot connect to runtime service {:?}: {:?}", service_addr, e);
                    }
                }
            }
        }

        // Drop removed clients.
        let mut clients_to_remove = vec![];
        for (k, _) in clients.iter() {
            if !config.runtime_cluster.contains(k) {
                clients_to_remove.push(*k);
            }
        }
        for k in clients_to_remove {
            clients.remove(&k);
        }

        drop(clients);

        // Update app list.
        let apps = self.apps.read().await;

        let mut new_apps_config: BTreeMap<AppId, &AppConfig> = config.apps.iter().map(|x| (x.id.clone(), x)).collect();

        // Figure out newly added apps
        let mut unseen_appids: BTreeSet<AppId> = new_apps_config.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in apps.iter() {
            unseen_appids.remove(k);
        }

        // Release lock.
        drop(apps);

        // Build new apps.
        let mut unseen_apps: Vec<(AppId, AppState)> = vec![];

        // unseen_appids is a subset of keys(new_apps_config) so we can unwrap here
        // Concurrently fetch scripts
        let app_scripts: Vec<_> = unseen_appids.iter().map(|id| {
            new_apps_config.get(&id).unwrap().script.clone()
        }).map(|script_url| async move {
            info!("fetching script for app {}", script_url);
            // TODO: limit body size
            let res = reqwest::get(&script_url)
                .await?;
            if !res.status().is_success() {
                Ok::<_, reqwest::Error>(None)
            } else {
                let body = res.text().await?;
                Ok::<_, reqwest::Error>(Some(body))
            }
        }).collect();
        let app_scripts = futures::future::join_all(app_scripts).await;

        for (id, fetch_result) in unseen_appids.into_iter().zip(app_scripts.into_iter()) {
            info!("loading app {}", id.0);
            let app_config = new_apps_config.get(&id).unwrap(); 
            let script = match fetch_result {
                Ok(Some(x)) => x,
                Ok(None) => {
                    info!("fetch failed: app {} ({})", id.0, app_config.script);
                    continue;
                }
                Err(e) => {
                    info!("fetch failed: app {} ({}): {:?}", id.0, app_config.script, e);
                    continue;
                }
            };

            let state = AppState {
                id: id.clone(),
                config: self.worker_config.clone(),
                script,
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

        // Rebuild routing table.
        let mut routing_table: BTreeMap<String, BTreeMap<String, AppId>> = BTreeMap::new();
        for (id, &app_config) in new_apps_config.iter() {
            for route in &app_config.routes {
                info!("inserting route: {:?}", route);
                routing_table.entry(route.domain.clone()).or_insert(BTreeMap::new())
                    .insert(route.path_prefix.clone(), id.clone());
            }
        }

        *self.route_mappings.write().await = routing_table;
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
        let mut res = hyper::Response::new(hyper::Body::from(status.canonical_reason().unwrap_or("unknown error")));
        *res.status_mut() = status;
        res
    }
}