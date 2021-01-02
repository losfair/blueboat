use rand::Rng;

pub fn rand_hex(num_bytes: usize) -> String {
    let mut rng = rand::thread_rng();
    let mut buf = vec![0u8; num_bytes];
    for b in &mut buf {
        *b = rng.gen();
    }
    hex::encode(buf)
}
