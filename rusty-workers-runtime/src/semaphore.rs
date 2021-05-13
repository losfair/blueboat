use crossbeam::channel::{Receiver, Sender};

pub struct Semaphore {
    tx: Sender<()>,
    rx: Receiver<()>,
}

pub struct Permit<'a> {
    tx: &'a Sender<()>,
}

pub struct OwnedPermit {
    tx: Sender<()>,
}

impl Semaphore {
    pub fn new(n: usize) -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        for _ in 0..n {
            tx.send(()).unwrap();
        }
        Self { tx, rx }
    }

    pub fn acquire_timeout<'a>(&'a self, timeout: std::time::Duration) -> Option<Permit<'a>> {
        self.rx.recv_timeout(timeout).ok()?;
        Some(Permit { tx: &self.tx })
    }

    #[allow(dead_code)]
    pub fn acquire_owned_timeout(&self, timeout: std::time::Duration) -> Option<OwnedPermit> {
        self.rx.recv_timeout(timeout).ok()?;
        Some(OwnedPermit {
            tx: self.tx.clone(),
        })
    }
}

impl<'a> Drop for Permit<'a> {
    fn drop(&mut self) {
        self.tx.send(()).unwrap();
    }
}

impl Drop for OwnedPermit {
    fn drop(&mut self) {
        // Maybe the receiver is already dropped
        let _ = self.tx.send(());
    }
}
