use serde::{Serialize, Deserialize};
use rusty_workers::types::*;

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
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AsyncCall {
    Ping,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServiceEvent {
    Fetch(FetchEvent),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FetchEvent {
    pub request: RequestObject,
}
