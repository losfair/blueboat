use rusty_workers::types::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServiceCall {
    Sync(SyncCall),
    Async(AsyncCall),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SyncCall {
    Log(String),
    Done,
    SendFetchResponse(ResponseObject),
    GetRandomValues(usize),
    GetFile(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AsyncCall {
    SetTimeout(u64),
    Fetch(RequestObject),
    KvGet {
        namespace: String,
        key: Vec<u8>,
    },
    KvPut {
        namespace: String,
        key: Vec<u8>,
        value: Vec<u8>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServiceEvent {
    Fetch(FetchEvent),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FetchEvent {
    pub request: RequestObject,
}
