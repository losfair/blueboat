//! RPC interface definitions.

use crate::types::*;

macro_rules! impl_connect {
    ($ty:ident) => {
        impl $ty {
            pub async fn connect<
                A: tokio::net::ToSocketAddrs + Clone + Send + Sync + Unpin + 'static,
            >(
                addr: A,
            ) -> GenericResult<Self> {
                use std::time::Duration;
                use stubborn_io::{ReconnectOptions, StubbornTcpStream};
                let opts = ReconnectOptions::new()
                    .with_exit_if_first_connect_fails(false)
                    .with_retries_generator(|| std::iter::repeat(Duration::from_secs(5)));
                let tcp_stream = StubbornTcpStream::connect_with_options(addr, opts).await?;
                let transport = tarpc::serde_transport::Transport::from((
                    tcp_stream,
                    crate::SerdeFormat::default(),
                ));
                let client = $ty::new(tarpc::client::Config::default(), transport).spawn()?;
                Ok(client)
            }

            pub async fn connect_noretry<A: tokio::net::ToSocketAddrs>(
                addr: A,
            ) -> GenericResult<Self> {
                let transport =
                    tarpc::serde_transport::tcp::connect(addr, crate::SerdeFormat::default);
                let client =
                    $ty::new(tarpc::client::Config::default(), transport.await?).spawn()?;
                Ok(client)
            }
        }
    };
}

#[macro_export]
macro_rules! impl_listen {
    ($ty:ident, $interface:path) => {
        impl $ty {
            pub async fn listen<A: $crate::tokio::net::ToSocketAddrs, F: FnMut() -> $ty>(
                addr: A,
                concurrency: usize,
                mut init: F,
            ) -> $crate::types::GenericResult<()> {
                use $crate::futures::StreamExt;
                use $crate::tarpc::server::Channel;
                use $interface;
                let mut listener =
                    $crate::tarpc::serde_transport::tcp::listen(addr, $crate::SerdeFormat::default)
                        .await?;
                listener
                    .filter_map(|r| $crate::futures::future::ready(r.ok())) // Ignore accept errors.
                    .map($crate::tarpc::server::BaseChannel::with_defaults)
                    .map(move |channel| {
                        let server = init();
                        channel.respond_with(server.serve()).execute()
                    })
                    .buffer_unordered(concurrency)
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
    /// Unique identifier. Specific to each instance and is not permanent.
    async fn id() -> RuntimeId;

    /// Spawn a worker.
    async fn spawn_worker(
        appid: String,
        configuration: WorkerConfiguration,
        bundle: Vec<u8>,
    ) -> GenericResult<WorkerHandle>;

    /// Terminates a worker.
    async fn terminate_worker(handle: WorkerHandle) -> bool;

    /// List active workers.
    async fn list_workers() -> GenericResult<Vec<WorkerHandle>>;

    /// Issue a "fetch" event.
    async fn fetch(handle: WorkerHandle, req: RequestObject) -> ExecutionResult<ResponseObject>;

    /// The current load of this runtime instance. 0-65535.
    async fn load() -> GenericResult<u16>;
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
