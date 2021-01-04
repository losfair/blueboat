#[macro_use]
extern crate log;

mod config;
mod sched;

use structopt::StructOpt;
use anyhow::Result;
use std::net::SocketAddr;
use rusty_workers::tarpc;
use config::*;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use once_cell::sync::OnceCell;

use hyper::{Body, Response, Server};
use hyper::service::{make_service_fn, service_fn};

static SCHEDULER: OnceCell<sched::Scheduler> = OnceCell::new();

#[derive(Debug, StructOpt)]
#[structopt(
    name = "rusty-workers-proxy",
    about = "Rusty Workers (frontend proxy)"
)]
struct Opt {
    /// HTTP listen address.
    #[structopt(short = "l", long)]
    http_listen: SocketAddr,

    /// Path to configuration
    #[structopt(short = "c", long)]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    info!("rusty-workers-proxy starting");

    let config = read_file(&opt.config).await?;
    let config: Arc<Config> = Arc::new(toml::from_str(&config)?);

    let scheduler = sched::Scheduler::new(config).await?;
    SCHEDULER.set(scheduler).unwrap_or_else(|_| panic!("cannot set scheduler"));

    let make_svc = make_service_fn(|_| async move {
        Ok::<_, hyper::Error>(service_fn(|req| async move {
            let scheduler = SCHEDULER.get().unwrap();
            match scheduler.handle_request(req).await {
                Ok(x) => Ok::<_, hyper::Error>(x),
                Err(e) => {
                    debug!("handle_request failed: {:?}", e);
                    let mut res = Response::new(Body::from("internal server error"));
                    *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                    Ok::<_, hyper::Error>(res)
                }
            }
        }))
    });

    Server::bind(&opt.http_listen).serve(make_svc).await?;
    Ok(())
}

async fn read_file(path: &str) -> Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).await?;
    Ok(buf)
}