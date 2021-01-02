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
