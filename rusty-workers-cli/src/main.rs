#[macro_use]
extern crate log;

use std::net::SocketAddr;
use structopt::StructOpt;
use anyhow::Result;
use rusty_workers::tarpc;
use rusty_workers::types::*;
use tokio::io::AsyncReadExt;

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
    #[structopt(name = "spawn")]
    Spawn {
        #[structopt(long)]
        account: String,

        #[structopt(long)]
        config: String,

        #[structopt(long)]
        script: String,
    },

    #[structopt(name = "terminate")]
    Terminate {
        #[structopt(long)]
        handle: String,
    },

    #[structopt(name = "list")]
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    match opt.cmd {
        Cmd::Runtime { remote, op } => {
            let mut client = rusty_workers::rpc::RuntimeServiceClient::connect(remote).await?;
            match op {
                RuntimeCmd::Spawn { account, config, script } => {
                    let config = read_file(&config).await?;
                    let script = read_file(&script).await?;
                    let result = client.spawn_worker(tarpc::context::current(), account, serde_json::from_str(&config)?, script).await?;
                    println!("{:?}", result);
                }
                RuntimeCmd::Terminate { handle } => {
                    let worker_handle = WorkerHandle { id: handle };
                    let result = client.terminate_worker(tarpc::context::current(), worker_handle).await?;
                    println!("{:?}", result);
                }
                RuntimeCmd::List => {
                    let result = client.list_workers(tarpc::context::current()).await?;
                    println!("{:?}", result);
                }
            }
        }
    }
    Ok(())
}

async fn read_file(path: &str) -> Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).await?;
    Ok(buf)
}