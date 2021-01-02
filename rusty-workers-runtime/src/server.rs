use rusty_workers::types::*;
use rusty_workers::tarpc;
use crate::runtime::Runtime;
use std::sync::Arc;

#[derive(Clone)]
pub struct RuntimeServer {
    pub runtime: Arc<Runtime>,
}

#[tarpc::server]
impl rusty_workers::rpc::RuntimeService for RuntimeServer {
    async fn spawn_worker(self, _: tarpc::context::Context, account: String, configuration: WorkerConfiguration, code: String) -> GenericResult<WorkerHandle> {
        self.runtime.spawn(code, &configuration).await
    }

    async fn terminate_worker(self, _: tarpc::context::Context, handle: WorkerHandle) -> GenericResult<()> {
        self.runtime.terminate(&handle).await
    }

    async fn list_workers(self, _: tarpc::context::Context) -> GenericResult<Vec<WorkerHandle>> {
        self.runtime.list().await
    }
}

rusty_workers::impl_listen!(RuntimeServer, rusty_workers::rpc::RuntimeService);
