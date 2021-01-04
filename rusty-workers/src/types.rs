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
    pub max_io_concurrency: u32,
    pub max_io_per_request: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorkerHandle {
    pub id: String,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct RequestObject {
    pub headers: BTreeMap<String, Vec<String>>,
    pub method: String,
    pub url: String,
    pub body: Option<HttpBody>,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct ResponseObject {
    pub headers: BTreeMap<String, Vec<String>>,
    pub status: u16,
    pub body: HttpBody,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum HttpBody {
    Binary(Vec<u8>),
    Text(String),
}

impl Default for HttpBody {
    fn default() -> Self {
        Self::Text("".into())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Error)]
pub enum GenericError {
    #[error("io: {0}")]
    Io(String),

    #[error("executor: {0}")]
    Executor(String),

    #[error("v8 unknown error")]
    V8Unknown,

    #[error("I/O timed out")]
    IoTimeout,

    #[error("runtime throws exception")]
    RuntimeThrowsException,

    #[error("script throws exception")]
    ScriptThrowsException(String),

    #[error("script compile exception")]
    ScriptCompileException,

    #[error("time limit exceeded")]
    TimeLimitExceeded,

    #[error("memory limit exceeded")]
    MemoryLimitExceeded,

    #[error("I/O limit exceeded")]
    IoLimitExceeded,

    #[error("no such worker")]
    NoSuchWorker,

    #[error("try again")]
    TryAgain,

    #[error("type conversion failed")]
    Conversion,

    #[error("type checking failed - expected {expected}")]
    Typeck { expected: String },

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