#[macro_use]
extern crate log;

mod config;
mod sched;

use anyhow::Result;
use once_cell::sync::OnceCell;
use rusty_workers::types::*;
use std::net::SocketAddr;
use structopt::StructOpt;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Response, Server};
use sched::SchedError;
use crate::config::*;

static SCHEDULER: OnceCell<sched::Scheduler> = OnceCell::new();

#[derive(Debug, StructOpt)]
#[structopt(name = "rusty-workers-proxy", about = "Rusty Workers (frontend proxy)")]
struct Opt {
    /// HTTP listen address.
    #[structopt(short = "l", long)]
    http_listen: SocketAddr,

    /// Path to configuration
    #[structopt(short = "c", long, env = "RW_CONFIG_URL")]
    config: String,

    #[structopt(long, env = "RW_FETCH_SERVICE")]
    fetch_service: SocketAddr,

    /// Runtime service backends, comma-separated.
    #[structopt(long, env = "RUNTIMES")]
    runtimes: String,

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

    /// Max ready instances per app
    #[structopt(long, env = "RW_MAX_READY_INSTANCES_PER_APP", default_value = "50")]
    max_ready_instances_per_app: usize,

    /// Expiration time for ready instances
    #[structopt(long, env = "RW_READY_INSTANCE_EXPIRATION_MS", default_value = "120000")]
    ready_instance_expiration_ms: u64,

    /// Request timeout in milliseconds.
    #[structopt(long, env = "RW_REQUEST_TIMEOUT_MS", default_value = "30000")]
    pub request_timeout_ms: u64,

    /// Max request body size in bytes.
    #[structopt(long, env = "RW_MAX_REQUEST_BODY_SIZE_BYTES", default_value = "2097152")]
    pub max_request_body_size_bytes: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    rusty_workers::init();

    let opt = Opt::from_args();

    let mut runtime_cluster: Vec<SocketAddr> = Vec::new();
    for elem in opt.runtimes.split(",") {
        runtime_cluster.push(elem.parse()?);
    }

    SCHEDULER
        .set(sched::Scheduler::new(WorkerConfiguration {
            executor: ExecutorConfiguration {
                max_memory_mb: opt.max_memory_mb,
                max_time_ms: opt.max_time_ms,
                max_io_concurrency: opt.max_io_concurrency,
                max_io_per_request: opt.max_io_per_request,
            },
            fetch_service: opt.fetch_service,
            env: Default::default(),
        }, LocalConfig {
            max_ready_instances_per_app: opt.max_ready_instances_per_app,
            ready_instance_expiration_ms: opt.ready_instance_expiration_ms,
            request_timeout_ms: opt.request_timeout_ms,
            max_request_body_size_bytes: opt.max_request_body_size_bytes,
            runtime_cluster,
        }))
        .unwrap_or_else(|_| panic!("cannot set scheduler"));

    let config_url = opt.config;
    tokio::spawn(async move {
        loop {
            if let Err(e) = SCHEDULER
                .get()
                .unwrap()
                .check_config_update(&config_url)
                .await
            {
                warn!("check_config_update: {:?}", e);
            }
            tokio::time::sleep(std::time::Duration::from_secs(53)).await;
        }
    });
    tokio::spawn(async move {
        loop {
            let scheduler = SCHEDULER.get().unwrap();
            scheduler.discover_runtimes().await;
            scheduler.query_runtimes().await;
            tokio::time::sleep(std::time::Duration::from_secs(7)).await;
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
                        Err(_) => {
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
