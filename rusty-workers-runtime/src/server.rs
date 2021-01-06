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
    async fn id(self, _: tarpc::context::Context) -> RuntimeId {
        self.runtime.id()
    }

    async fn spawn_worker(self, _: tarpc::context::Context, appid: String, configuration: WorkerConfiguration, code: String) -> GenericResult<WorkerHandle> {
        self.runtime.spawn(appid, code, &configuration).await
    }

    async fn terminate_worker(self, _: tarpc::context::Context, handle: WorkerHandle) -> GenericResult<()> {
        self.runtime.terminate(&handle).await
    }

    async fn list_workers(self, _: tarpc::context::Context) -> GenericResult<Vec<WorkerHandle>> {
        self.runtime.list().await
    }

    async fn fetch(self, _: tarpc::context::Context, handle: WorkerHandle, req: RequestObject) -> GenericResult<ResponseObject> {
        self.runtime.fetch(&handle, req).await
    }

    async fn load(self, _: tarpc::context::Context) -> GenericResult<u16> {
        self.runtime.load().await
    }
}

rusty_workers::impl_listen!(RuntimeServer, rusty_workers::rpc::RuntimeService);
