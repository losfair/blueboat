#[macro_use]
extern crate log;

mod server;

use structopt::StructOpt;
use anyhow::Result;
use futures::future;
use futures::StreamExt;
use std::net::SocketAddr;
use rusty_workers::tarpc::{self, server::Channel};
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
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let opt = Opt::from_args();
    info!("rusty-workers-runtime starting");

    let mut listener = tarpc::serde_transport::tcp::listen(&opt.rpc_listen, rusty_workers::SerdeFormat::default).await?;
    listener.config_mut().max_frame_length(16 * 1048576);
    listener
        .filter_map(|r| future::ready(r.ok())) // Ignore accept errors.
        .map(tarpc::server::BaseChannel::with_defaults)
        .map(|channel| {
            let server = server::RuntimeServer;
            channel.respond_with(server.serve()).execute()
        })
        .buffer_unordered(100) // Max 100 channels.
        .for_each(|_| async {})
        .await;

    Ok(())
}
