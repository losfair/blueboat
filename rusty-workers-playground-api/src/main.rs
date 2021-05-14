#[macro_use]
extern crate log;

mod types;

use anyhow::Result;
use futures::StreamExt;
use hyper::{
    header::HeaderValue,
    service::{make_service_fn, service_fn},
    Body, Request, Response, StatusCode,
};
use rand::Rng;
use rusty_workers::app::AppConfig;
use rusty_workers::db::DataClient;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use structopt::StructOpt;
use thiserror::Error;
use types::*;

const MAX_REQUEST_BODY_SIZE: usize = 8 * 1024 * 1024;

#[derive(Error, Debug)]
enum CpError {
    #[error("request body too large")]
    RequestBodyTooLarge,

    #[error("bad 128-bit identifier")]
    BadId128,

    #[error("bad auth token")]
    BadAuthToken,
}

#[derive(Debug, StructOpt, Clone)]
#[structopt(
    name = "rusty-workers-playground-api",
    about = "Rusty Workers (playground api)"
)]
struct Opt {
    /// HTTP listen address.
    #[structopt(short = "l", long)]
    http_listen: SocketAddr,

    /// TiKV PD addresses.
    #[structopt(long, env = "TIKV_PD")]
    tikv_pd: String,

    /// Authentication token, 128-bit base64.
    #[structopt(long, env = "RW_AUTH_TOKEN")]
    auth_token: String,
}

struct Server {
    _config: Opt,
    auth_token: [u8; 16],
    kv: DataClient,
}

impl Server {
    async fn handle(self: Arc<Self>, request: Request<Body>) -> Result<Response<Body>> {
        let token = rusty_workers::app::decode_id128(
            &request
                .headers()
                .get("x-token")
                .ok_or_else(|| CpError::BadAuthToken)?
                .to_str()?,
        )
        .ok_or_else(|| CpError::BadId128)?;
        if ring::constant_time::verify_slices_are_equal(&token, &self.auth_token).is_err() {
            return Err(CpError::BadAuthToken.into());
        }

        let req_path = request.uri().path().to_string();
        let req_body = read_request_body(request).await?;
        match req_path.as_str() {
            "/v1/list_routes" => {
                let opt: ListRoutesOpt = serde_json::from_slice(&req_body)?;
                let routes = self.kv.route_mapping_list_for_domain(&opt.domain).await?;
                println!("{}", serde_json::to_string(&routes)?);
                Ok(mk_json_response(&routes)?)
            }
            "/v1/add_route" => {
                let opt: AddRouteOpt = serde_json::from_slice(&req_body)?;
                self.kv
                    .route_mapping_insert(&opt.domain, &opt.path, opt.appid)
                    .await?;
                Ok(mk_json_response(&())?)
            }
            "/v1/delete_domain" => {
                let opt: DeleteDomainOpt = serde_json::from_slice(&req_body)?;
                self.kv.route_mapping_delete_domain(&opt.domain).await?;
                Ok(mk_json_response(&())?)
            }
            "/v1/delete_route" => {
                let opt: DeleteRouteOpt = serde_json::from_slice(&req_body)?;
                self.kv.route_mapping_delete(&opt.domain, &opt.path).await?;
                Ok(mk_json_response(&())?)
            }
            "/v1/add_app" => {
                let opt: AddAppOpt = serde_json::from_slice(&req_body)?;
                let mut config: AppConfig = opt.config;
                let bundle = base64::decode(&opt.bundle_b64)?;

                cleanup_previous_app(&self.kv, &config.id).await?;

                let mut bundle_id = [0u8; 16];
                rand::thread_rng().fill(&mut bundle_id);
                self.kv.app_bundle_put(&bundle_id, bundle).await?;
                config.bundle_id = rusty_workers::app::encode_id128(&bundle_id);

                self.kv.app_metadata_put(&config).await?;
                Ok(mk_json_response(&())?)
            }
            "/v1/delete_app" => {
                let opt: DeleteAppOpt = serde_json::from_slice(&req_body)?;
                let appid = rusty_workers::app::AppId(opt.appid);
                cleanup_previous_app(&self.kv, &appid).await?;
                self.kv.app_metadata_delete(&appid.0).await?;

                // TODO: Delete logs?

                Ok(mk_json_response(&())?)
            }
            "/v1/delete_namespace" => {
                let opt: DeleteNamespaceOpt = serde_json::from_slice(&req_body)?;
                let namespace =
                    rusty_workers::app::decode_id128(&opt.nsid).ok_or_else(|| CpError::BadId128)?;
                let keys = self
                    .kv
                    .worker_data_scan_keys(&namespace, b"", None, opt.batch_size)
                    .await?;
                for k in keys.iter() {
                    self.kv.worker_data_delete(&namespace, k).await?;
                }
                Ok(mk_json_response(&keys.len())?)
            }
            "/v1/logs" => {
                let opt: LogsOpt = serde_json::from_slice(&req_body)?;
                let now = SystemTime::now();
                let since = now - Duration::from_secs(opt.since_secs);

                #[derive(serde::Serialize)]
                struct Item {
                    time: String,
                    text: String,
                }
                let mut items = vec![];

                self.kv
                    .log_range(&format!("app-{}", opt.appid), since..now, |time, text| {
                        items.push(Item {
                            time: time.to_string(),
                            text: text.to_string(),
                        });
                        if items.len() >= opt.limit as usize {
                            false
                        } else {
                            true
                        }
                    })
                    .await?;
                Ok(mk_json_response(&items)?)
            }
            _ => {
                let mut res = Response::new(Body::from("not found"));
                *res.status_mut() = StatusCode::NOT_FOUND;
                Ok(res)
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    rusty_workers::init();
    let opt = Opt::from_args();

    let server = Arc::new(Server {
        kv: DataClient::new(opt.tikv_pd.split(",").collect()).await?,
        _config: opt.clone(),
        auth_token: rusty_workers::app::decode_id128(&opt.auth_token)
            .ok_or_else(|| CpError::BadId128)?,
    });

    let make_svc = make_service_fn(move |_| {
        let server = server.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let server = server.clone();
                async move {
                    server.handle(req).await.or_else(|e| {
                        let mut res = Response::new(Body::from(format!("{:?}", e)));
                        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                        Ok::<_, hyper::Error>(res)
                    })
                }
            }))
        }
    });
    info!("starting http server");

    hyper::Server::bind(&opt.http_listen)
        .serve(make_svc)
        .await?;
    Ok(())
}

fn mk_json_response<T: serde::Serialize + ?Sized>(v: &T) -> Result<Response<Body>> {
    let body = serde_json::to_string(v)?;
    let mut res = Response::new(Body::from(body));
    res.headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    Ok(res)
}

async fn read_request_body(req: Request<Body>) -> Result<Vec<u8>> {
    let mut full_body = vec![];
    let mut body_error: Result<()> = Ok(());
    req.into_body()
        .for_each(|bytes| {
            match bytes {
                Ok(x) => {
                    if full_body.len() + x.len() > MAX_REQUEST_BODY_SIZE {
                        body_error = Err(CpError::RequestBodyTooLarge.into());
                    } else {
                        full_body.extend_from_slice(&x);
                    }
                }
                Err(e) => {
                    body_error = Err(e.into());
                }
            };
            futures::future::ready(())
        })
        .await;
    body_error?;
    Ok(full_body)
}

async fn cleanup_previous_app(
    client: &DataClient,
    appid: &rusty_workers::app::AppId,
) -> Result<()> {
    if let Some(prev_md) = client.app_metadata_get(&appid.0).await? {
        if let Ok(prev_config) = serde_json::from_slice::<AppConfig>(&prev_md) {
            client
                .app_bundle_delete(
                    &rusty_workers::app::decode_id128(&prev_config.bundle_id)
                        .ok_or_else(|| CpError::BadId128)?,
                )
                .await?;
            warn!("deleted previous bundle");
        } else {
            warn!("unable to decode previous metadata");
        }
    }
    Ok(())
}
