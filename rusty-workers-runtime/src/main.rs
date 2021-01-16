#![feature(trait_alias)]
#![feature(ffi_returns_twice)]

#[macro_use]
extern crate log;

mod buffer;
mod config;
mod crypto;
mod engine;
mod error;
mod executor;
mod interface;
mod io;
mod isolate;
mod mm;
mod remote_buffer;
mod runtime;
mod semaphore;
mod server;

use anyhow::Result;
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
    let rt = runtime::Runtime::new(opt.config).await?;
    info!("id: {}", rt.id().0);

    // GC thread
    let rt2 = rt.clone();
    tokio::spawn(async move {
        loop {
            rt2.lru_gc().await;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    server::RuntimeServer::listen(&opt.rpc_listen, max_concurrency, move || {
        server::RuntimeServer {
            runtime: rt.clone(),
        }
    })
    .await?;

    Ok(())
}
