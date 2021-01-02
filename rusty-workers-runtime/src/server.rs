use rusty_workers::types::*;
use rusty_workers::tarpc;

#[derive(Clone)]
pub struct RuntimeServer;

#[tarpc::server]
impl rusty_workers::rpc::RuntimeService for RuntimeServer {
    async fn spawn_worker(self, _: tarpc::context::Context, account: String, configuration: WorkerConfiguration, code: String) -> GenericResult<WorkerHandle> {
        Err(GenericError::Other("not implemented".into()))
    }

    async fn terminate_worker(self, _: tarpc::context::Context, handle: WorkerHandle) -> GenericResult<()> {
        Err(GenericError::Other("not implemented".into()))
    }

    async fn list_workers(self, _: tarpc::context::Context) -> GenericResult<Vec<WorkerHandle>> {
        Err(GenericError::Other("not implemented".into()))
    }
}
