#[macro_use]
extern crate log;

use std::net::SocketAddr;
use structopt::StructOpt;
use anyhow::Result;
use rusty_workers::tarpc;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "rusty-workers-cli",
    about = "Rusty Workers (cli)"
)]
struct Opt {
    #[structopt(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, StructOpt)]
enum Cmd {
    /// Connect to runtime.
    Runtime {
        /// Remote address.
        #[structopt(short = "r", long)]
        remote: SocketAddr,

        #[structopt(subcommand)]
        op: RuntimeCmd,
    }
}

#[derive(Debug, StructOpt)]
enum RuntimeCmd {
    #[structopt(name = "list-workers")]
    ListWorkers,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    match opt.cmd {
        Cmd::Runtime { remote, op } => {
            let mut client = rusty_workers::rpc::RuntimeServiceClient::connect(remote).await?;
            let result = client.list_workers(tarpc::context::current()).await?;
            println!("result: {:?}", result);
        }
    }
    Ok(())
}