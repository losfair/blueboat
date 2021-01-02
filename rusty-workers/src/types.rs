use serde::{Serialize, Deserialize};
use thiserror::Error;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerConfiguration {
    pub executor: ExecutorConfiguration,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutorConfiguration {
    pub max_memory_mb: u32,
    pub max_time_ms: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorkerHandle {
    pub id: String,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct RequestObject {
    pub cache: String,
    pub context: String,
    pub credentials: String,
    pub destination: String,
    pub headers: BTreeMap<String, Vec<String>>,
    pub method: String,
    pub mode: String,
    pub redirect: String,
    pub referrer: String,

    #[serde(rename = "referrerPolicy")]
    pub referrer_policy: String,

    pub url: String,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct ResponseObject {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Error)]
pub enum GenericError {
    #[error("io: {0}")]
    Io(String),

    #[error("executor: {0}")]
    Executor(String),

    #[error("v8 unknown error")]
    V8Unknown,

    #[error("script throws exception")]
    ScriptThrowsException,

    #[error("limits exceeded")]
    LimitsExceeded,

    #[error("no such worker")]
    NoSuchWorker,

    #[error("other: {0}")]
    Other(String),
}

pub type GenericResult<T> = Result<T, GenericError>;

impl From<std::io::Error> for GenericError {
    fn from(other: std::io::Error) -> Self {
        Self::Io(format!("{:?}", other))
    }
}

impl WorkerHandle {
    pub fn generate() -> Self {
        Self {
            id: crate::util::rand_hex(16),
        }
    }
}