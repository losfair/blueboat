use rusty_v8 as v8;
use lru_time_cache::LruCache;
use rusty_workers::types::*;
use std::time::Duration;
use crate::executor::Instance;
use tokio::sync::Mutex as AsyncMutex;

pub struct Runtime {
    instances: AsyncMutex<LruCache<WorkerHandle, Instance>>,
}

pub fn init() {
    let platform = v8::new_default_platform().unwrap();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
}

impl Runtime {
    pub fn new() -> Self {
        Runtime {
            instances: AsyncMutex::new(LruCache::with_expiry_duration_and_capacity(Duration::from_secs(600), 100)), // arbitrary choices
        }
    }

}
