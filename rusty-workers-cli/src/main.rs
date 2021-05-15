#[allow(unused_imports)]
#[macro_use]
extern crate log;

use anyhow::Result;
use rand::Rng;
use rusty_workers::app::AppConfig;
use rusty_workers::db::DataClient;
use rusty_workers::tarpc;
use rusty_workers::types::*;
use std::net::SocketAddr;
use structopt::StructOpt;
use thiserror::Error;
use tokio::io::AsyncReadExt;

#[derive(Debug, Error)]
enum CliError {
    #[error("bad id128")]
    BadId128,
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
        /// MySQL-compatible database URL.
        #[structopt(long, env = "DB_URL")]
        db_url: String,

        #[structopt(subcommand)]
        op: AppCmd,
    },
}

#[derive(Debug, StructOpt)]
enum AppCmd {
    #[structopt(name = "list-routes")]
    ListRoutes { domain: String },
    #[structopt(name = "add-route")]
    AddRoute {
        domain: String,

        #[structopt(long)]
        path: String,

        #[structopt(long)]
        appid: String,
    },
    #[structopt(name = "delete-domain")]
    DeleteDomain { domain: String },
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
    #[structopt(name = "add-app")]
    AddApp {
        config: String,

        #[structopt(long)]
        bundle: String,
    },
    #[structopt(name = "add-single-file-app")]
    AddSingleFileApp {
        config: String,

        #[structopt(long)]
        js: String,
    },
    #[structopt(name = "delete-app")]
    DeleteApp { appid: String },
    #[structopt(name = "get-app")]
    GetApp { appid: String },
    #[structopt(name = "get-bundle")]
    GetBundle { bundle: String },
    #[structopt(name = "list-worker-data")]
    ListWorkerData {
        namespace: String,
        #[structopt(long)]
        from: String,
        #[structopt(long)]
        limit: u32,
        #[structopt(long)]
        base64_key: bool,
    },
    #[structopt(name = "get-worker-data")]
    GetWorkerData {
        namespace: String,
        #[structopt(long)]
        key: String,
        #[structopt(long)]
        base64_key: bool,
        #[structopt(long)]
        base64_value: bool,
    },
    #[structopt(name = "put-worker-data")]
    PutWorkerData {
        namespace: String,
        #[structopt(long)]
        key: String,
        #[structopt(long)]
        value: String,
        #[structopt(long)]
        base64_key: bool,
        #[structopt(long)]
        base64_value: bool,
    },
    #[structopt(name = "delete-worker-data")]
    DeleteWorkerData {
        namespace: String,
        #[structopt(long)]
        key: String,
        #[structopt(long)]
        base64_key: bool,
    },
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
                                max_ab_memory_mb: 32,
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
        Cmd::App { db_url, op } => {
            let client = DataClient::new(&db_url).await?;
            match op {
                AppCmd::ListRoutes { domain } => {
                    let routes = client.route_mapping_list_for_domain(&domain).await?;
                    println!("{}", serde_json::to_string(&routes)?);
                }
                AppCmd::AddRoute {
                    domain,
                    path,
                    appid,
                } => {
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
                AppCmd::AddApp { config, bundle } => {
                    let config = read_file(&config).await?;
                    let mut config: AppConfig = toml::from_str(&config)?;
                    let bundle = read_file_raw(&bundle).await?;

                    do_add_app(&client, &mut config, &bundle).await?;
                    println!("OK");
                }
                AppCmd::AddSingleFileApp { config, js } => {
                    let config = read_file(&config).await?;
                    let mut config: AppConfig = toml::from_str(&config)?;

                    let mut js = std::fs::File::open(&js)?;
                    let mut archive: Vec<u8> = Vec::new();
                    {
                        let mut builder = tar::Builder::new(&mut archive);
                        builder.append_file("./index.js", &mut js)?;
                        builder.finish()?;
                    }

                    do_add_app(&client, &mut config, &archive).await?;
                    println!("OK");
                }
                AppCmd::DeleteApp { appid } => {
                    let appid = rusty_workers::app::AppId(appid);
                    client.app_metadata_delete(&appid.0).await?;

                    // TODO: Delete logs?

                    println!("OK");
                }
                AppCmd::GetApp { appid } => {
                    let result: Option<AppConfig> = client.app_metadata_get(&appid).await?;
                    println!("{}", serde_json::to_string(&result)?);
                }
                AppCmd::GetBundle { bundle } => {
                    let bundle = client.app_bundle_get(&bundle).await?;
                    println!(
                        "{}",
                        serde_json::to_string(&bundle.map(|x| base64::encode(&x)))?
                    );
                }
                AppCmd::ListWorkerData {
                    namespace,
                    from,
                    limit,
                    base64_key,
                } => {
                    let from = if !base64_key {
                        Vec::from(from)
                    } else {
                        base64::decode(&from)?
                    };

                    let keys = client
                        .worker_data_scan_keys(&namespace, &from, None, limit)
                        .await?;
                    let keys: Vec<Option<String>> = keys
                        .into_iter()
                        .map(|k| {
                            if !base64_key {
                                String::from_utf8(k).ok()
                            } else {
                                Some(base64::encode(&k))
                            }
                        })
                        .collect();
                    let serialized = serde_json::to_string(&keys)?;
                    println!("{}", serialized);
                }
                AppCmd::GetWorkerData {
                    namespace,
                    key,
                    base64_key,
                    base64_value,
                } => {
                    let key = if !base64_key {
                        key.as_bytes().to_vec()
                    } else {
                        base64::decode(&key)?
                    };
                    let value = client.worker_data_get(&namespace, &key).await?;
                    if let Some(v) = value {
                        if !base64_value {
                            println!(
                                "{}",
                                serde_json::to_string(std::str::from_utf8(&v)?).unwrap()
                            );
                        } else {
                            println!("{}", serde_json::to_string(&base64::encode(v)).unwrap());
                        }
                    } else {
                        println!("null");
                    }
                }
                AppCmd::PutWorkerData {
                    namespace,
                    key,
                    value,
                    base64_key,
                    base64_value,
                } => {
                    let key = if !base64_key {
                        key.as_bytes().to_vec()
                    } else {
                        base64::decode(&key)?
                    };
                    let value = if !base64_value {
                        value.as_bytes().to_vec()
                    } else {
                        base64::decode(&value)?
                    };
                    client
                        .worker_data_put(&namespace, &key, &value, false, 0)
                        .await?;
                    println!("OK");
                }
                AppCmd::DeleteWorkerData {
                    namespace,
                    key,
                    base64_key,
                } => {
                    let key = if !base64_key {
                        key.as_bytes().to_vec()
                    } else {
                        base64::decode(&key)?
                    };
                    client.worker_data_delete(&namespace, &key).await?;
                    println!("OK");
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

async fn do_add_app(client: &DataClient, config: &mut AppConfig, bundle: &[u8]) -> Result<()> {
    let mut bundle_id = [0u8; 16];
    rand::thread_rng().fill(&mut bundle_id);
    let bundle_id = base64::encode(&bundle_id);
    client.app_bundle_put(&bundle_id, &bundle).await?;
    config.bundle_id = bundle_id;

    client.app_metadata_put(config).await?;
    Ok(())
}

fn make_context() -> tarpc::context::Context {
    let mut current = tarpc::context::current();
    current.deadline = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    current
}
