use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerConfiguration {
    pub max_memory_mb: u32,
    pub max_time_ms: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerHandle {
    pub id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Error)]
pub enum GenericError {
    #[error("io: {0}")]
    Io(String),

    #[error("other: {0}")]
    Other(String),
}

pub type GenericResult<T> = Result<T, GenericError>;

impl From<std::io::Error> for GenericError {
    fn from(other: std::io::Error) -> Self {
        Self::Io(format!("{:?}", other))
    }
}
