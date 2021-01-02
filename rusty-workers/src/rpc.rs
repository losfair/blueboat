//! RPC interface definitions.

use crate::types::*;

macro_rules! impl_connect {
    ($ty:ident) => {
        impl $ty {
            pub async fn connect<A: tokio::net::ToSocketAddrs>(addr: A) -> GenericResult<Self> {
                let mut transport = tarpc::serde_transport::tcp::connect(addr, crate::SerdeFormat::default);
                transport.config_mut().max_frame_length(16 * 1048576);
                let mut client = $ty::new(tarpc::client::Config::default(), transport.await?).spawn()?;
                Ok(client)
            }
        }
    };
}

#[macro_export]
macro_rules! impl_listen {
    ($ty:ident, $interface:path) => {
        impl $ty {
            pub async fn listen<A: $crate::tokio::net::ToSocketAddrs, F: FnMut() -> $ty>(addr: A, mut init: F) -> $crate::types::GenericResult<()> {
                use $interface;
                use $crate::futures::StreamExt;
                use $crate::tarpc::server::Channel;
                let mut listener = $crate::tarpc::serde_transport::tcp::listen(addr, $crate::SerdeFormat::default).await?;
                listener.config_mut().max_frame_length(16 * 1048576);
                listener
                    .filter_map(|r| $crate::futures::future::ready(r.ok())) // Ignore accept errors.
                    .map($crate::tarpc::server::BaseChannel::with_defaults)
                    .map(move |channel| {
                        let server = init();
                        channel.respond_with(server.serve()).execute()
                    })
                    .buffer_unordered(100) // Max 100 channels.
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
    async fn spawn_worker(account: String, configuration: WorkerConfiguration, code: String) -> GenericResult<WorkerHandle>;

    /// Terminates a worker.
    async fn terminate_worker(handle: WorkerHandle) -> GenericResult<()>;

    /// List active workers.
    async fn list_workers() -> GenericResult<Vec<WorkerHandle>>;
}

impl_connect!(RuntimeServiceClient);
