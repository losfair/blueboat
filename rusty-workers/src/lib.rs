#[macro_use]
extern crate log;

pub mod app;
pub mod db;
pub mod rpc;
pub mod types;
pub mod util;

pub use futures;
pub use tarpc;
pub use tokio;
pub use tokio_serde;

// FIXME: MessagePack doesn't work (connection resets). Why?
pub use tokio_serde::formats::Bincode as SerdeFormat;

pub fn init() {
    info!("rusty-workers version {}", git_version::git_version!());
}
