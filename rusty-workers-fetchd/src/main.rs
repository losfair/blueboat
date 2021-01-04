#[macro_use]
extern crate log;

mod server;

use structopt::StructOpt;
use anyhow::Result;
use std::net::SocketAddr;
use rusty_workers::tarpc;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "rusty-workers-fetchd",
    about = "Rusty Workers (fetchd)"
)]
struct Opt {
    /// RPC listen address.
    #[structopt(short = "l", long)]
    rpc_listen: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    info!("rusty-workers-fetchd starting");

    let state = server::FetchState::new()?;
    server::FetchServer::listen(&opt.rpc_listen, move || server::FetchServer::new(state.clone())).await?;

    Ok(())
}
