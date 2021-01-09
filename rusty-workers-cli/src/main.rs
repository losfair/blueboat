#[macro_use]
extern crate log;

use anyhow::Result;
use rusty_workers::tarpc;
use rusty_workers::types::*;
use std::net::SocketAddr;
use structopt::StructOpt;
use tokio::io::AsyncReadExt;
use rusty_workers::kv::KvClient;
use rusty_workers::app::AppConfig;
use sha2::Digest;
use thiserror::Error;

#[derive(Debug, Error)]
enum CliError {
    #[error("bad bundle hash")]
    BadBundleHash,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "rusty-workers-cli", about = "Rusty Workers (cli)")]
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
    },

    /// App management.
    App {
        /// TiKV PD address.
        #[structopt(long, env = "TIKV_PD")]
        tikv_pd: String,

        #[structopt(subcommand)]
        op: AppCmd,
    }
}

#[derive(Debug, StructOpt)]
enum AppCmd {
    #[structopt(name = "all-routes")]
    AllRoutes,
    #[structopt(name = "list-routes")]
    ListRoutes {
        domain: String,
    },
    #[structopt(name = "add-route")]
    AddRoute {
        domain: String,

        #[structopt(long)]
        path: String,

        #[structopt(long)]
        appid: String,
    },
    #[structopt(name = "delete-domain")]
    DeleteDomain {
        domain: String,
    },
    #[structopt(name = "delete-route")]
    DeleteRoute {
        domain: String,

        #[structopt(long)]
        path: String,
    },
    #[structopt(name = "lookup-route")]
    LookupRoute {
        domain: String,

        #[structopt(long)]
        path: String,
    },
    #[structopt(name = "all-apps")]
    AllApps,
    #[structopt(name = "add-app")]
    AddApp {
        config: String,
    },
    #[structopt(name = "delete-app")]
    DeleteApp {
        appid: String,
    },
    #[structopt(name = "get-app")]
    GetApp {
        appid: String,
    },
    #[structopt(name = "add-bundle")]
    AddBundle {
        path: String,
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

        #[structopt(long)]
        fetch_service: SocketAddr,

        script: String,
    },

    #[structopt(name = "terminate")]
    Terminate { handle: String },

    #[structopt(name = "list")]
    List,

    #[structopt(name = "fetch")]
    Fetch { handle: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    rusty_workers::init();
    let opt = Opt::from_args();
    match opt.cmd {
        Cmd::Runtime { remote, op } => {
            let mut client = rusty_workers::rpc::RuntimeServiceClient::connect(remote).await?;
            match op {
                RuntimeCmd::Spawn {
                    appid,
                    config,
                    script,
                    fetch_service,
                } => {
                    let config = if let Some(config) = config {
                        let text = read_file(&config).await?;
                        serde_json::from_str(&text)?
                    } else {
                        WorkerConfiguration {
                            executor: ExecutorConfiguration {
                                max_memory_mb: 32,
                                max_time_ms: 50,
                                max_io_concurrency: 10,
                                max_io_per_request: 50,
                            },
                            fetch_service,
                            env: Default::default(),
                            kv_namespaces: Default::default(),
                        }
                    };
                    let script = read_file_raw(&script).await?;
                    let result = client
                        .spawn_worker(make_context(), appid, config, script)
                        .await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
                RuntimeCmd::Terminate { handle } => {
                    let worker_handle = WorkerHandle { id: handle };
                    let result = client
                        .terminate_worker(make_context(), worker_handle)
                        .await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
                RuntimeCmd::List => {
                    let result = client.list_workers(make_context()).await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
                RuntimeCmd::Fetch { handle } => {
                    let worker_handle = WorkerHandle { id: handle };
                    let req = RequestObject::default();
                    let result = client.fetch(make_context(), worker_handle, req).await?;
                    println!("{}", serde_json::to_string(&result).unwrap());
                }
            }
        }
        Cmd::App { tikv_pd, op } => {
            let client = KvClient::new(vec![tikv_pd]).await?;
            match op {
                AppCmd::AllRoutes => {
                    print!("[");
                    let mut first = true;
                    client.route_mapping_for_each(|domain, path, appid| {
                        if first {
                            first = false;
                        } else {
                            print!(",");
                        }
                        print!("{}", serde_json::to_string(&serde_json::json!({
                            "domain": domain,
                            "path": path,
                            "appid": appid,
                        })).unwrap());
                        true
                    }).await?;
                    println!("]");
                }
                AppCmd::ListRoutes { domain } => {
                    let routes = client.route_mapping_list_for_domain(&domain, |_| true).await?;
                    println!("{}", serde_json::to_string(&routes)?);
                }
                AppCmd::AddRoute { domain, path, appid } => {
                    client.route_mapping_insert(&domain, &path, appid).await?;
                    println!("OK");
                }
                AppCmd::DeleteDomain { domain } => {
                    client.route_mapping_delete_domain(&domain).await?;
                    println!("OK");
                }
                AppCmd::DeleteRoute { domain, path } => {
                    client.route_mapping_delete(&domain, &path).await?;
                    println!("OK");
                }
                AppCmd::LookupRoute { domain, path } => {
                    let result = client.route_mapping_lookup(&domain, &path).await?;
                    println!("{}", serde_json::to_string(&result)?);
                }
                AppCmd::AllApps => {
                    print!("[");
                    let mut first = true;
                    client.app_metadata_for_each(|k| {
                        if first {
                            first = false;
                        } else {
                            print!(",");
                        }
                        print!("{}", serde_json::to_string(k).unwrap());
                        true
                    }).await?;
                    println!("]");
                }
                AppCmd::AddApp { config } => {
                    let config = read_file(&config).await?;
                    let config: AppConfig = toml::from_str(&config)?;

                    let bundle_hash = rusty_workers::app::decode_bundle_hash(&config.bundle_hash)
                        .ok_or_else(|| CliError::BadBundleHash)?;
                    if client.app_bundle_get(&bundle_hash).await?.is_none() {
                        println!("bundle does not exist");
                    }

                    client.app_metadata_put(&config.id.0, serde_json::to_vec(&config)?).await?;
                    println!("OK");
                }
                AppCmd::DeleteApp { appid } => {
                    // Don't delete bundle as this is shared
                    client.app_metadata_delete(&appid).await?;
                    println!("OK");
                }
                AppCmd::GetApp { appid } => {
                    let result: Option<AppConfig> = client.app_metadata_get(&appid).await?
                        .map(|x| serde_json::from_slice(&x))
                        .transpose()?;
                    println!("{}", serde_json::to_string(&result)?);
                }
                AppCmd::AddBundle { path } => {
                    let bundle = read_file_raw(&path).await?;

                    let mut hasher = sha2::Sha256::new();
                    hasher.update(&bundle);
                    let hash: [u8; 32] = *hasher.finalize().as_ref();

                    client.app_bundle_put(&hash, bundle).await?;
                    println!("{}", rusty_workers::app::encode_bundle_hash(&hash));
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

async fn read_file_raw(path: &str) -> Result<Vec<u8>> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).await?;
    Ok(buf)
}

fn make_context() -> tarpc::context::Context {
    let mut current = tarpc::context::current();
    current.deadline = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    current
}
