use rusty_workers::types::*;
use serde::{Deserialize, Serialize};
use rusty_v8 as v8;
use std::cell::Cell;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServiceCall {
    Sync(SyncCall),
    Async(AsyncCallV),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SyncCall {
    Log(String),
    Done,
    SendFetchResponse(ResponseObject),
    GetRandomValues(usize),
    GetFile(String),
}

pub struct AsyncCall {
    pub v: AsyncCallV,
    pub buffers: Vec<v8::SharedRef<v8::BackingStore>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AsyncCallV {
    SetTimeout(u64),
    Fetch(RequestObject),
    KvGet {
        namespace: String,
    },
    KvPut {
        namespace: String,
    },
    KvDelete {
        namespace: String,
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

pub trait JsBuffer {
    fn read_to_vec(&self) -> Vec<u8>;
}

impl JsBuffer for v8::SharedRef<v8::BackingStore> {
    fn read_to_vec(&self) -> Vec<u8> {
        let source: &[Cell<u8>] = self;
        let mut buf = Vec::with_capacity(source.len());
        buf.extend(source.iter().map(|x| x.get()));
        buf
    }
}