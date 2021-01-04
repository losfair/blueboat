use tokio::sync::oneshot;
use rusty_v8 as v8;
use rusty_workers::types::*;
use slab::Slab;
use serde::{Serialize, Deserialize};
use anyhow::Result;
use crate::interface::AsyncCall;
use std::time::Duration;
use serde_json::json;

pub struct IoWaiter {
    remaining_budget: u32,
    inflight: Slab<v8::Global<v8::Function>>,
    task: tokio::sync::mpsc::UnboundedSender<(usize, AsyncCall)>,
    result: std::sync::mpsc::Receiver<(usize, String)>,
    conf: ExecutorConfiguration,
}

pub struct IoProcessor {
    task: tokio::sync::mpsc::UnboundedReceiver<(usize, AsyncCall)>,
    result: std::sync::mpsc::Sender<(usize, String)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum IoTask {
    Ping,
}

struct IoResponseHandle {
    result: std::sync::mpsc::Sender<(usize, String)>,
    index: usize,
}

impl IoWaiter {
    pub fn new(conf: ExecutorConfiguration) -> (Self, IoProcessor) {
        let init_budget = conf.max_io_per_request;
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel();
        let waiter = IoWaiter {
            remaining_budget: init_budget,
            inflight: Slab::new(),
            task: task_tx,
            result: result_rx,
            conf,
        };
        let processor = IoProcessor {
            task: task_rx,
            result: result_tx,
        };
        (waiter, processor)
    }

    pub fn issue(&mut self, count_budget: bool, task: AsyncCall, cb: v8::Global<v8::Function>) -> GenericResult<()> {
        if count_budget {
            if self.remaining_budget == 0 {
                return Err(GenericError::IoLimitExceeded);
            }
            self.remaining_budget -= 1;
        }

        if self.inflight.len() >= self.conf.max_io_concurrency as usize {
            return Err(GenericError::IoLimitExceeded);
        }

        let index = self.inflight.insert(cb);
        match self.task.send((index, task)) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.inflight.remove(index);
                Err(GenericError::Other("io worker exited".into()))
            }
        }
    }

    pub fn wait(&mut self) -> GenericResult<(v8::Global<v8::Function>, String)> {
        let (index, result) = self.result.recv_timeout(std::time::Duration::from_secs(30)).map_err(|_| GenericError::IoTimeout)?;
        let req = self.inflight.remove(index);
        Ok((req, result))
    }
}

impl IoProcessor {
    async fn next(&mut self) -> Option<(AsyncCall, IoResponseHandle)> {
        let (index, task) = self.task.recv().await?;
        Some((task, IoResponseHandle {
            result: self.result.clone(),
            index,
        }))
    }

    pub async fn run(mut self) {
        use tokio::sync::watch;
        let (_kill_tx, kill_rx) = watch::channel(());

        loop {
            let (task, res) = match self.next().await {
                Some(x) => x,
                None => {
                    debug!("leaving IoProcessor::run");
                    break;
                }
            };
            let mut kill_rx = kill_rx.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = kill_rx.changed() => {
                        // killed
                    }
                    ret = handle_task(task) => {
                        match ret {
                            Ok(x) => res.respond(format!("{{\"Ok\":{}}}", x)),
                            Err(e) => {
                                debug!("io error: {:?}", e);
                                res.respond(format!("{{\"Err\":{}}}", "io error"));
                            }
                        }
                    }
                }
            });
        }
    }
}

impl IoResponseHandle {
    fn respond(self, data: String) {
        drop(self.result.send((self.index, data)));
    }
}

async fn handle_task(task: AsyncCall) -> Result<String> {
    match task {
        AsyncCall::SetTimeout(n) => {
            let dur = Duration::from_millis(n);
            tokio::time::sleep(dur).await;
            Ok("null".into())
        }
    }
}