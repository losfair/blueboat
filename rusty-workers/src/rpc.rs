//! RPC interface definitions.

use crate::types::*;

macro_rules! impl_connect {
    ($ty:ident) => {
        impl $ty {
            pub async fn connect<A: tokio::net::ToSocketAddrs + Clone + Send + Sync + Unpin + 'static>(addr: A) -> GenericResult<Self> {
                use stubborn_io::{StubbornTcpStream};
                let tcp_stream = StubbornTcpStream::connect(addr).await?;
                let transport = tarpc::serde_transport::Transport::from((tcp_stream, crate::SerdeFormat::default()));
                let client = $ty::new(tarpc::client::Config::default(), transport).spawn()?;
                Ok(client)
            }
        }
    };
}

#[macro_export]
macro_rules! impl_listen {
    ($ty:ident, $interface:path, $concurrency:expr) => {
        impl $ty {
            pub async fn listen<A: $crate::tokio::net::ToSocketAddrs, F: FnMut() -> $ty>(addr: A, mut init: F) -> $crate::types::GenericResult<()> {
                use $interface;
                use $crate::futures::StreamExt;
                use $crate::tarpc::server::Channel;
                let mut listener = $crate::tarpc::serde_transport::tcp::listen(addr, $crate::SerdeFormat::default).await?;
                listener
                    .filter_map(|r| $crate::futures::future::ready(r.ok())) // Ignore accept errors.
                    .map($crate::tarpc::server::BaseChannel::with_defaults)
                    .map(move |channel| {
                        let server = init();
                        channel.respond_with(server.serve()).execute()
                    })
                    .buffer_unordered($concurrency)
                    .for_each(|_| async {})
                    .await;
                Ok(())
            }
        }
    };
}

/// The Workers runtime.
#[tarpc::service]
pub trait RuntimeService {
    /// Spawn a worker.
    async fn spawn_worker(appid: String, configuration: WorkerConfiguration, code: String) -> GenericResult<WorkerHandle>;

    /// Terminates a worker.
    async fn terminate_worker(handle: WorkerHandle) -> GenericResult<()>;

    /// List active workers.
    async fn list_workers() -> GenericResult<Vec<WorkerHandle>>;

    /// Issue a "fetch" event.
    async fn fetch(handle: WorkerHandle, req: RequestObject) -> GenericResult<ResponseObject>;
}

impl_connect!(RuntimeServiceClient);

/// Fetch service.
#[tarpc::service]
pub trait FetchService {
    /// Fetch from a remote URL.
    /// 
    /// Result is wrapped twice because we want to be able to send custom errors to client.
    async fn fetch(req: RequestObject) -> GenericResult<Result<ResponseObject, String>>;
}

impl_connect!(FetchServiceClient);
