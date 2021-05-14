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
use rusty_workers::db::DataClient;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use structopt::StructOpt;
use thiserror::Error;
use types::*;

const MAX_REQUEST_BODY_SIZE: usize = 8 * 1024 * 1024;

#[derive(Error, Debug)]
enum CpError {
    #[error("request body too large")]
    RequestBodyTooLarge,
}

#[derive(Debug, StructOpt, Clone)]
#[structopt(name = "rusty-workers-cp", about = "Rusty Workers (control plane)")]
struct Opt {
    /// HTTP listen address.
    #[structopt(short = "l", long)]
    http_listen: SocketAddr,

    /// TiKV PD addresses.
    #[structopt(long, env = "TIKV_PD")]
    tikv_pd: String,

    /// MySQL-compatible database URL.
    #[structopt(long, env = "RW_DB_URL")]
    db_url: String,
}

struct Server {
    config: Opt,
    kv: DataClient,
}

impl Server {
    async fn handle(self: Arc<Self>, request: Request<Body>) -> Result<Response<Body>> {
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
        kv: DataClient::new(opt.tikv_pd.split(",").collect(), &opt.db_url).await?,
        config: opt.clone(),
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
