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

#[derive(Debug, Error)]
pub enum SchedError {
    #[error("no available instance")]
    NoAvailableInstance,

    #[error("initialization timoeut")]
    InitializationTimeout,

    #[error("no route mapping found")]
    NoRouteMapping,

    #[error("request body too large")]
    RequestBodyTooLarge,

    #[error("request failed after retries")]
    RequestFailedAfterRetries,
}

pub struct Scheduler {
    config: Arc<Config>,
    clients: AsyncRwLock<BTreeMap<SocketAddr, RtState>>,
    apps: AsyncRwLock<BTreeMap<AppId, AsyncMutex<AppState>>>,
    route_mappings: AsyncRwLock<BTreeMap<String, BTreeMap<String, AppId>>>, // domain -> (prefix -> appid)
}

/// State of a backing runtime.
#[derive(Clone)]
struct RtState {
    /// The client.
    client: RuntimeServiceClient,
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
    ready_instances: VecDeque<ReadyInstance>,
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
    fn pool_instance(&mut self, inst: ReadyInstance) {
        self.ready_instances.push_back(inst);
    }

    async fn get_instance(&mut self, config: &Config, clients: &AsyncRwLock<BTreeMap<SocketAddr, RtState>>) -> Result<ReadyInstance> {
        while let Some(mut instance) = self.ready_instances.pop_front() {
            // TODO: Maintain load data for each client and select based on load.
            if instance.is_usable(config) {
                instance.update_last_active();
                return Ok(instance);
            }
        }

        let clients = clients.read().await;

        // No available instance now. Create one.
        if clients.len() == 0 {
            return Err(SchedError::NoAvailableInstance.into());
        }
        let index = rand::thread_rng().gen_range(0..clients.len());
        let (addr, mut rt) = clients.iter().nth(index).map(|(k, v)| (*k, v.clone())).unwrap();

        info!("spawning new worker for app {}", self.id.0);

        // TODO: Re-select and retry on failure
        let handle = rt.client.spawn_worker(
            tarpc::context::current(),
            self.id.0.clone(),
            self.config.clone(),
            self.script.clone()
        ).await??;
        Ok(ReadyInstance {
            addr,
            last_active: Instant::now(),
            handle,
            client: rt.client,
        })
    }
}

impl Scheduler {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        let mut scheduler = Self {
            config,
            clients: AsyncRwLock::new(BTreeMap::new()),
            apps: AsyncRwLock::new(BTreeMap::new()),
            route_mappings: AsyncRwLock::new(BTreeMap::new()),
        };
        scheduler.populate_config().await;
        Ok(scheduler)
    }

    pub async fn handle_request(&self, req: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>> {
        let route_mappings = self.route_mappings.read().await;

        let uri = req.uri();
        let host = req.headers().get("host").and_then(|x| x.to_str().ok()).unwrap_or("");
        debug!("host: {}", host);
        let submappings = route_mappings.get(host).ok_or(SchedError::NoRouteMapping)?;

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
        debug!("routing request to app {}", appid.0);

        let method = req.method().as_str().to_string();
        let mut headers = BTreeMap::new();
        let url = format!("{}", uri);
        let mut full_body = vec![];

        for (k, v) in req.headers() {
            headers.entry(k.as_str().to_string()).or_insert(vec![]).push(v.to_str()?.to_string());
        }

        let mut body_error: Result<()> = Ok(());
        req.into_body().for_each(|bytes| {
            match bytes {
                Ok(x) => {
                    if full_body.len() + x.len() > self.config.max_request_body_size_bytes as usize {
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
        let mut app = apps.get(&appid).ok_or(SchedError::NoRouteMapping)?.lock().await;

        // Backend retries.
        for _ in 0..3usize {
            let mut instance = app.get_instance(&self.config, &self.clients).await?;
    
            let mut fetch_context = tarpc::context::current();
            fetch_context.deadline = std::time::SystemTime::now() + Duration::from_millis(self.config.request_timeout_ms);

            let fetch_res = instance.client.fetch(fetch_context, instance.handle.clone(), target_req.clone())
                .await?; // Don't retry in case of network errors.
            let fetch_res = match fetch_res {
                Ok(x) => x,
                Err(e) => {
                    // Don't pool it back.
                    // Runtime would give us a 500 instead of an error when it is recoverable.
                    debug!("backend returns error: {:?}", e);
                    continue;
                }
            };

            // Pool it back.
            app.pool_instance(instance);

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

    async fn flush_route_mappings(&self) {
    }

    async fn populate_config(&self) {
        // TODO: Finer-grained locking
        let mut clients = self.clients.write().await;

        // Add new clients.
        for service_addr in self.config.runtime_cluster.iter() {
            if !clients.contains_key(service_addr) {
                match RuntimeServiceClient::connect(*service_addr).await {
                    Ok(client) => {
                        clients.insert(*service_addr, RtState {
                            client,
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
            if !self.config.runtime_cluster.contains(k) {
                clients_to_remove.push(*k);
            }
        }
        for k in clients_to_remove {
            clients.remove(&k);
        }

        drop(clients);

        // Update app list.
        let apps = self.apps.read().await;

        let mut new_apps_config: BTreeMap<AppId, &AppConfig> = self.config.apps.iter().map(|x| (x.id.clone(), x)).collect();

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
            debug!("fetching script for app {}", script_url);
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
            debug!("loading app {}", id.0);
            let app_config = new_apps_config.get(&id).unwrap(); 
            let script = match fetch_result {
                Ok(Some(x)) => x,
                Ok(None) => {
                    debug!("fetch failed: app {} ({})", id.0, app_config.script);
                    continue;
                }
                Err(e) => {
                    debug!("fetch failed: app {} ({}): {:?}", id.0, app_config.script, e);
                    continue;
                }
            };

            let state = AppState {
                id: id.clone(),
                config: app_config.worker.clone(),
                script,
                ready_instances: VecDeque::new(),
            };
            unseen_apps.push((id, state));
        }

        // Take a write lock.
        let mut apps = self.apps.write().await;

        // Add new apps.
        for (id, state) in unseen_apps {
            apps.insert(id, AsyncMutex::new(state));
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
                debug!("inserting route: {:?}", route);
                routing_table.entry(route.domain.clone()).or_insert(BTreeMap::new())
                    .insert(route.path_prefix.clone(), id.clone());
            }
        }

        *self.route_mappings.write().await = routing_table;
    }
}
