use crossbeam::channel::{Sender, Receiver};

pub struct Semaphore {
    tx: Sender<()>,
    rx: Receiver<()>,
}

pub struct Permit<'a> {
    tx: &'a Sender<()>,
}

impl Semaphore {
    pub fn new(n: usize) -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        for _ in 0..n {
            tx.send(()).unwrap();
        }
        Self {
            tx,
            rx,
        }
    }

    pub fn acquire_timeout<'a>(&'a self, timeout: std::time::Duration) -> Option<Permit<'a>> {
        self.rx.recv_timeout(timeout).ok()?;
        Some(Permit { tx: &self.tx })
    }
}

impl<'a> Drop for Permit<'a> {
    fn drop(&mut self) {
        self.tx.send(()).unwrap();
    }
}