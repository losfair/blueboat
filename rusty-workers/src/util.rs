use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::Rng;

pub fn rand_bytes(num_bytes: usize) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let mut buf = vec![0u8; num_bytes];
    for b in &mut buf {
        *b = rng.gen();
    }
    buf
}

pub fn rand_hex(num_bytes: usize) -> String {
    hex::encode(rand_bytes(num_bytes))
}

pub fn current_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0))
        .as_millis() as u64
}
