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
        #[structopt(short = "r", long, env = "RUNTIME_ADDR")]
        remote: SocketAddr,

        #[structopt(subcommand)]
        op: RuntimeCmd,
    }
}

#[derive(Debug, StructOpt)]
enum RuntimeCmd {
    #[structopt(name = "spawn")]
    Spawn {
        #[structopt(long, default_value = "")]
        appid: String,

        #[structopt(long)]
        config: Option<String>,

        script: String,
    },

    #[structopt(name = "terminate")]
    Terminate {
        handle: String,
    },

    #[structopt(name = "list")]
    List,

    #[structopt(name = "fetch")]
    Fetch {
        handle: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    match opt.cmd {
        Cmd::Runtime { remote, op } => {
            let mut client = rusty_workers::rpc::RuntimeServiceClient::connect(remote).await?;
            match op {
                RuntimeCmd::Spawn { appid, config, script } => {
                    let config = if let Some(config) = config {
                        let text = read_file(&config).await?;
                        serde_json::from_str(&text)?
                    } else {
                        WorkerConfiguration {
                            executor: ExecutorConfiguration {
                                max_memory_mb: 32,
                                max_time_ms: 100,
                                max_io_concurrency: 10,
                                max_io_per_request: 50,
                            }
                        }
                    };
                    let script = read_file(&script).await?;
                    let result = client.spawn_worker(tarpc::context::current(), appid, config, script).await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
                RuntimeCmd::Terminate { handle } => {
                    let worker_handle = WorkerHandle { id: handle };
                    let result = client.terminate_worker(tarpc::context::current(), worker_handle).await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
                RuntimeCmd::List => {
                    let result = client.list_workers(tarpc::context::current()).await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
                RuntimeCmd::Fetch { handle } => {
                    let worker_handle = WorkerHandle { id: handle };
                    let req = RequestObject::default();
                    let result = client.fetch(tarpc::context::current(), worker_handle, req).await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
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