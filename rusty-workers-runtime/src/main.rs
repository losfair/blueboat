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

    /// Address of fetch service.
    #[structopt(long, env = "RW_FETCH_SERVICE_ADDR")]
    fetch_service: Option<SocketAddr>,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    info!("rusty-workers-runtime starting");

    runtime::init();

    let rt = runtime::Runtime::new();

    if let Some(addr) = opt.fetch_service {
        let mut client = rusty_workers::rpc::FetchServiceClient::connect(addr).await?;
        rt.set_fetch_client(client).await;
        info!("connected to fetch service");
    }

    server::RuntimeServer::listen(&opt.rpc_listen, move || server::RuntimeServer {
        runtime: rt.clone(),
    }).await?;

    Ok(())
}
