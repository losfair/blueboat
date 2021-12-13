use std::{ops::Range, time::Duration};

pub static TASK_LOCK_TTL: Duration = Duration::from_secs(60);
pub static TASK_LOCK_RENEWAL_TIMEOUT: Duration = Duration::from_secs(10);
pub const BACKOFF_RANGE_MS: Range<u64> = 1000..5000;
pub const BACKOFF_FAST_RANGE_MS: Range<u64> = 10..100;
