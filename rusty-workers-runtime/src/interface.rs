use rusty_v8 as v8;
use rusty_workers::types::*;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::io::Read;

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
    KvGet { namespace: String, #[serde(default)] for_update: bool },
    KvPut { namespace: String },
    KvDelete { namespace: String },
    KvBeginTransaction,
    KvRollbackTransaction,
    KvCommitTransation,
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
    fn read_to_vec(&self, max_length: usize) -> Option<Vec<u8>>;
}

impl JsBuffer for v8::SharedRef<v8::BackingStore> {
    fn read_to_vec(&self, max_length: usize) -> Option<Vec<u8>> {
        let source: &[Cell<u8>] = self;
        if source.len() > max_length {
            return None;
        }
        let mut buf = Vec::with_capacity(source.len());
        buf.extend(source.iter().map(|x| x.get()));
        Some(buf)
    }
}

pub struct ReadableByteCellSlice<'a>(&'a [Cell<u8>]);

impl<'a> ReadableByteCellSlice<'a> {
    pub fn new(inner: &'a [Cell<u8>]) -> Self {
        Self(inner)
    }
}

impl<'a> Read for ReadableByteCellSlice<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read_len = buf.len().min(self.0.len());
        for i in 0..read_len {
            buf[i] = self.0[i].get();
        }
        self.0 = &self.0[read_len..];
        Ok(read_len)
    }
}
