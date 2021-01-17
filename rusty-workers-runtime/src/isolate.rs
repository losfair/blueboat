//! V8 isolate owner threads and pools.

use crate::mm::MemoryPool;
use rusty_v8 as v8;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Semaphore};

/// JavaScript-side runtime.
static LIBRT: &'static str = include_str!("../../librt/dist/main.js");

pub struct Poison;

pub struct IsolateThreadPool {
    /// Manage the pool as a stack to optimize for cache.
    threads: std::sync::Mutex<Vec<IsolateThread>>,

    /// Pool acq/rel notifier.
    notifier: Semaphore,
}

/// A handle to a thread that owns an v8::Isolate.
struct IsolateThread {
    job_tx: mpsc::Sender<IsolateJob>,
}

#[derive(Debug, Clone)]
pub struct IsolateConfig {
    pub max_memory_bytes: usize,

    pub host_entry_threshold_memory_bytes: usize,
}

struct ThreadGuard<'a> {
    pool: &'a IsolateThreadPool,
    th: Option<IsolateThread>,
}

impl<'a> Drop for ThreadGuard<'a> {
    fn drop(&mut self) {
        self.pool
            .threads
            .lock()
            .unwrap()
            .push(self.th.take().unwrap());
    }
}

impl<'a> std::ops::Deref for ThreadGuard<'a> {
    type Target = IsolateThread;
    fn deref(&self) -> &Self::Target {
        self.th.as_ref().unwrap()
    }
}

pub type IsolateJob = Box<dyn FnOnce(&mut v8::ContextScope<'_, v8::HandleScope<'_>>) + Send>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct IsolateGeneration(pub u64);

#[derive(Clone)]
pub struct IsolateGenerationBox(pub Arc<std::sync::Mutex<IsolateGeneration>>);

#[derive(Clone)]
pub struct MemoryPoolBox(pub Arc<MemoryPool>);

impl IsolateThreadPool {
    pub async fn new(size: usize, config: IsolateConfig) -> Self {
        let start_time = std::time::Instant::now();
        let threads: Vec<IsolateThread> =
            futures::future::join_all((0..size).map(|_| IsolateThread::new(config.clone()))).await;
        let end_time = std::time::Instant::now();
        info!(
            "isolate pool of size {} initialized in {:?}",
            size,
            end_time.duration_since(start_time)
        );
        Self {
            threads: std::sync::Mutex::new(threads),
            notifier: Semaphore::new(size),
        }
    }

    pub async fn run<
        R: Send + 'static,
        F: FnOnce(&mut v8::ContextScope<'_, v8::HandleScope<'_>>) -> R + Send + 'static,
    >(
        &self,
        job: F,
    ) -> R {
        let _permit = self.notifier.acquire().await;
        let th = self
            .threads
            .lock()
            .unwrap()
            .pop()
            .expect("IsolateThreadPool::run: got permit but no thread available");

        // Return the thread back to the pool in case of async cancellation.
        // Drop order ensure that `permit` is released after `guard`.
        let guard = ThreadGuard {
            pool: self,
            th: Some(th),
        };

        let (ret_tx, ret_rx) = oneshot::channel();
        guard
            .job_tx
            .send(Box::new(|scope| {
                drop(ret_tx.send(job(scope)));
            }))
            .await
            .map_err(|_| "cannot send to job_tx")
            .unwrap();
        ret_rx.await.unwrap()
    }
}

impl IsolateThread {
    pub async fn new(config: IsolateConfig) -> Self {
        let (job_tx, mut job_rx) = mpsc::channel(1);
        std::thread::spawn(move || loop {
            isolate_worker(&config, &mut job_rx);
            std::thread::sleep(std::time::Duration::from_millis(100));
            info!("restarting isolate worker");
        });
        Self { job_tx }
    }
}

fn isolate_worker(config: &IsolateConfig, job_rx: &mut mpsc::Receiver<IsolateJob>) {
    // Don't allocate any budget for arraybuffers at start.
    let pool = crate::mm::MemoryPool::new(0);

    let params = v8::Isolate::create_params()
        .array_buffer_allocator(pool.clone().get_allocator())
        .heap_limits(0, config.max_memory_bytes);

    // Must not be moved
    let mut isolate = v8::Isolate::new(params);

    let librt_persistent;

    // Compile librt.
    // Many unwraps here! but since we are initializing it should be fine.
    {
        let mut isolate_scope = v8::HandleScope::new(&mut isolate);
        let context = v8::Context::new(&mut isolate_scope);
        let mut context_scope = v8::ContextScope::new(&mut isolate_scope, context);
        let scope = &mut v8::HandleScope::new(&mut context_scope);

        let librt = v8::String::new(scope, LIBRT).unwrap();
        let librt = v8::Script::compile(scope, librt, None)
            .unwrap()
            .get_unbound_script(scope);
        librt_persistent = v8::Global::new(scope, librt);
    }

    let generation = IsolateGenerationBox(Arc::new(std::sync::Mutex::new(IsolateGeneration(0))));
    isolate.set_slot(generation.clone());

    isolate.set_slot(MemoryPoolBox(pool));

    loop {
        let job = match job_rx.blocking_recv() {
            Some(x) => x,
            None => break,
        };

        // Reset arraybuffer memory pool budget for librt initialization.
        // Hardcoded to 16 MiB here.
        isolate
            .get_slot::<MemoryPoolBox>()
            .unwrap()
            .0
            .reset(1048576 * 16);

        // Enter context.
        let mut isolate_scope = v8::HandleScope::new(&mut isolate);
        let context = v8::Context::new(&mut isolate_scope);
        let mut context_scope = v8::ContextScope::new(&mut isolate_scope, context);

        // Run librt initialization.
        {
            let scope = &mut v8::HandleScope::new(&mut context_scope);

            let global_key = v8::String::new(scope, "global").unwrap();
            let global_obj = scope.get_current_context().global(scope);
            global_obj.set(scope, global_key.into(), global_obj.into());

            let librt = v8::Local::<'_, v8::UnboundScript>::new(scope, librt_persistent.clone())
                .bind_to_current_context(scope);
            librt.run(scope);
        }

        job(&mut context_scope);

        // Release scopes.
        drop(context_scope);
        drop(isolate_scope);

        if isolate.get_slot::<Poison>().is_some() {
            error!("isolated poisoned, dropping worker");
            return;
        }

        // Cleanup instance state so that we can reuse it.
        // Keep in sync with InstanceHandle::do_remote_termination.

        // Acquire generation lock.
        let mut generation = generation.0.lock().unwrap();

        // Advance generation.
        generation.0 += 1;

        // Cleanup termination.
        isolate.cancel_terminate_execution();

        // Drop the lock.
        drop(generation);

        // Cleanup instance-specific state + run GC.
        let cleanup_start = std::time::Instant::now();
        if let Err(e) = crate::executor::Instance::cleanup(&mut isolate) {
            // Cleanup failed - maybe a GC failure.
            error!("cleanup failed, dropped worker: {:?}", e);
            return;
        }
        let cleanup_end = std::time::Instant::now();
        info!(
            "Isolate recycled. Cleanup time: {:?}",
            cleanup_end.duration_since(cleanup_start)
        );
    }
}
