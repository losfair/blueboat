#![feature(trait_alias)]

#[macro_use]
extern crate log;

mod config;
mod engine;
mod error;
mod executor;
mod interface;
mod io;
mod runtime;
mod server;

use anyhow::Result;
use rusty_workers::rpc::RuntimeService;
use rusty_workers::tarpc;
use std::net::SocketAddr;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "rusty-workers-runtime", about = "Rusty Workers (runtime)")]
struct Opt {
    /// RPC listen address.
    #[structopt(short = "l", long)]
    rpc_listen: SocketAddr,

    #[structopt(flatten)]
    config: config::Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    rusty_workers::init();

    let opt = Opt::from_args();

    runtime::init();

    let max_concurrency = opt.config.max_concurrent_requests;
    let rt = runtime::Runtime::new(opt.config);
    info!("id: {}", rt.id().0);

    let rt2 = rt.clone();
    server::RuntimeServer::listen(&opt.rpc_listen, max_concurrency, move || {
        server::RuntimeServer {
            runtime: rt2.clone(),
        }
    })
    .await?;

    // GC thread
    tokio::spawn(async move {
        loop {
            rt.lru_gc().await;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    Ok(())
}
