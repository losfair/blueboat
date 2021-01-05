#[macro_use]
extern crate log;

mod config;
mod sched;

use structopt::StructOpt;
use anyhow::Result;
use std::net::SocketAddr;
use rusty_workers::tarpc;
use config::*;
use rusty_workers::types::*;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use once_cell::sync::OnceCell;

use hyper::{Body, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use sched::SchedError;

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
    #[structopt(short = "c", long, env = "RW_CONFIG_URL")]
    config: String,

    #[structopt(long, env = "RW_FETCH_SERVICE")]
    fetch_service: SocketAddr,

    /// Max memory per worker, in MB
    #[structopt(long, env = "RW_MAX_MEMORY_MB", default_value = "16")]
    max_memory_mb: u32,

    /// Max CPU time, in milliseconds
    #[structopt(long, env = "RW_MAX_TIME_MS", default_value = "100")]
    max_time_ms: u32,

    /// Max number of concurrent I/O operations
    #[structopt(long, env = "RW_MAX_IO_CONCURRENCY", default_value = "10")]
    max_io_concurrency: u32,

    /// Max number of I/O operations per request
    #[structopt(long, env = "RW_MAX_IO_PER_REQUEST", default_value = "50")]
    max_io_per_request: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    info!("rusty-workers-proxy starting");

    SCHEDULER.set(sched::Scheduler::new(WorkerConfiguration {
        executor: ExecutorConfiguration {
            max_memory_mb: opt.max_memory_mb,
            max_time_ms: opt.max_time_ms,
            max_io_concurrency: opt.max_io_concurrency,
            max_io_per_request: opt.max_io_per_request,
        },
        fetch_service: opt.fetch_service,
    })).unwrap_or_else(|_| panic!("cannot set scheduler"));

    let config_url = opt.config;
    tokio::spawn(async move {
        loop {
            if let Err(e) = SCHEDULER.get().unwrap().check_config_update(&config_url).await {
                warn!("check_config_update: {:?}", e);
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
    tokio::spawn(async move {
        loop {
            SCHEDULER.get().unwrap().query_runtime_loads().await;
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });

    let make_svc = make_service_fn(|_| async move {
        Ok::<_, hyper::Error>(service_fn(|req| async move {
            let scheduler = SCHEDULER.get().unwrap();
            match scheduler.handle_request(req).await {
                Ok(x) => Ok::<_, hyper::Error>(x),
                Err(e) => {
                    warn!("handle_request failed: {:?}", e);
                    let res = match e.downcast::<SchedError>() {
                        Ok(e) => e.build_response(),
                        Err(e) => {
                            let mut res = Response::new(Body::from("internal server error"));
                            *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                            res
                        }
                    };
                    Ok::<_, hyper::Error>(res)
                }
            }
        }))
    });

    Server::bind(&opt.http_listen).serve(make_svc).await?;
    Ok(())
}
