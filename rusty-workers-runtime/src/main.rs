#![feature(trait_alias)]

#[macro_use]
extern crate log;

mod server;
mod runtime;
mod executor;
mod error;
mod engine;
mod interface;
mod io;
mod config;

use structopt::StructOpt;
use anyhow::Result;
use std::net::SocketAddr;
use rusty_workers::tarpc;
use rusty_workers::rpc::RuntimeService;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "rusty-workers-runtime",
    about = "Rusty Workers (runtime)"
)]
struct Opt {
    /// RPC listen address.
    #[structopt(short = "l", long)]
    rpc_listen: SocketAddr,

    #[structopt(flatten)]
    config: config::Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    info!("rusty-workers-runtime starting");

    runtime::init();

    let max_concurrency = opt.config.max_concurrent_requests;
    let rt = runtime::Runtime::new(opt.config);

    let rt2 = rt.clone();
    server::RuntimeServer::listen(&opt.rpc_listen, max_concurrency, move || server::RuntimeServer {
        runtime: rt2.clone(),
    }).await?;

    // GC thread
    tokio::spawn(async move {
        loop {
            rt.lru_gc().await;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    Ok(())
}
