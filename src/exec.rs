use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  future::Future,
  rc::{Rc, Weak},
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};

use crate::{ctx::BlueboatCtx, ipc::BlueboatIpcRes};
use anyhow::Result;
use rusty_v8 as v8;
use thiserror::Error;
use tokio::{
  sync::{watch, Mutex as AsyncMutex, OwnedMutexGuard, OwnedRwLockWriteGuard, RwLock},
  task::spawn_local,
};

pub struct Executor {
  pub ctx: &'static BlueboatCtx,
  v8_ctx: v8::Global<v8::Context>,

  async_kill: AsyncKill,
  _async_kill_owner: OwnedRwLockWriteGuard<()>,

  async_completion: AsyncKill,
  async_completion_owner: RefCell<Option<OwnedRwLockWriteGuard<()>>>,

  spawn_activity: Arc<AsyncMutex<()>>,
  spawn_activity_maybe_owner: Weak<OwnedMutexGuard<()>>,

  completed_result: RefCell<Option<BlueboatIpcRes>>,
  pub busy_duration: Cell<Duration>,
  pub request_id: String,
  logseq: Cell<i32>,
  cancel: watch::Receiver<()>,
  pub mysql: Rc<AsyncMutex<HashMap<&'static str, Arc<AsyncMutex<ExecutorMysqlState>>>>>,
}

pub struct ExecutorMysqlState {
  pub conn: Option<mysql_async::Conn>,
  pub txn: Option<mysql_async::Transaction<'static>>,
  pub pool: &'static mysql_async::Pool,
}

#[derive(Clone)]
struct AsyncKill(Arc<RwLock<()>>);

#[derive(Clone)]
pub struct SpawnActivityOwner(Rc<OwnedMutexGuard<()>>);

thread_local! {
  static CURRENT: RefCell<Option<Weak<Executor>>> = RefCell::new(None);
}

pub struct CurrentExecutorGuard(());

impl Drop for CurrentExecutorGuard {
  fn drop(&mut self) {
    CURRENT.with(|x| {
      *x.borrow_mut() = None;
    });
  }
}

impl Executor {
  pub fn try_current() -> Option<Weak<Self>> {
    CURRENT.with(|x| (*x.borrow()).clone())
  }

  pub fn try_current_result() -> Result<Weak<Self>> {
    #[derive(Error, Debug)]
    #[error("no current executor")]
    struct NoCurrentExecutor;

    Ok(Self::try_current().ok_or(NoCurrentExecutor)?)
  }

  fn make_current(self: &Rc<Self>) -> CurrentExecutorGuard {
    CURRENT.with(|x| {
      let mut x = x.borrow_mut();
      assert!(x.is_none());
      *x = Some(self.downgrade());
    });
    CurrentExecutorGuard(())
  }

  /// We won't have more than 2**31 logs in a single request... Right?
  pub fn allocate_logseq(&self) -> i32 {
    let v = self.logseq.get();
    self.logseq.set(v + 1);
    v
  }

  pub fn get_cancel(&self) -> watch::Receiver<()> {
    self.cancel.clone()
  }

  pub fn new(
    ctx: &'static BlueboatCtx,
    request_id: String,
    cancel: watch::Receiver<()>,
  ) -> Result<(Rc<Self>, SpawnActivityOwner)> {
    let v8_ctx = ctx.grab_v8_context();
    let async_kill = AsyncKill(Arc::new(RwLock::new(())));
    let _async_kill_owner = async_kill.0.clone().try_write_owned().unwrap();
    let async_completion = AsyncKill(Arc::new(RwLock::new(())));
    let async_completion_owner =
      RefCell::new(Some(async_completion.0.clone().try_write_owned().unwrap()));

    let spawn_activity = Arc::new(AsyncMutex::new(()));
    let spawn_activity_owner =
      SpawnActivityOwner(Rc::new(spawn_activity.clone().try_lock_owned().unwrap()));
    let spawn_activity_maybe_owner = Rc::downgrade(&spawn_activity_owner.0);
    let me = Rc::new(Self {
      ctx,
      v8_ctx,
      async_kill,
      _async_kill_owner,
      async_completion,
      async_completion_owner,
      spawn_activity,
      spawn_activity_maybe_owner,
      completed_result: RefCell::new(None),
      busy_duration: Cell::new(Duration::ZERO),
      request_id,
      logseq: Cell::new(0),
      cancel,
      mysql: Rc::new(AsyncMutex::new(HashMap::new())),
    });
    Ok((me, spawn_activity_owner))
  }

  pub async fn wait_for_completion(self: &Rc<Self>) -> Option<BlueboatIpcRes> {
    tokio::select! {
      biased;
      _ = self.async_completion.wait() => Some(self.completed_result.borrow_mut().take().unwrap()),
      _ = self.spawn_activity.lock() => None,
    }
  }

  pub fn spawn<F: Future<Output = ()> + 'static>(me: &Weak<Self>, f: F) {
    let me = match me.upgrade() {
      Some(x) => x,
      None => return,
    };
    let k = me.async_kill.clone();
    let spawn_activity = if let Some(x) = me.spawn_activity_maybe_owner.upgrade() {
      x
    } else {
      log::debug!("spawn: failed to get activity");
      return;
    };
    spawn_local(async move {
      let _spawn_activity = spawn_activity;
      tokio::select! {
        biased;
        _ = k.wait() => {}
        _ = f => {}
      }
    });
  }

  pub fn downgrade(self: &Rc<Self>) -> Weak<Self> {
    Rc::downgrade(self)
  }

  pub fn complete(me: &Weak<Self>, value: BlueboatIpcRes) {
    if let Some(me) = me.upgrade() {
      *me.completed_result.borrow_mut() = Some(value);
      *me.async_completion_owner.borrow_mut() = None;
    }
  }

  pub fn enter<F: FnOnce(&mut v8::HandleScope) -> R, R>(me: &Weak<Self>, f: F) -> Option<R> {
    let me = me.upgrade()?;
    let start = Instant::now();
    let _g = me.make_current();
    let mut isolate = me
      .ctx
      .isolate
      .try_lock()
      .expect("failed to get lock on isolate");
    let handle = isolate.thread_safe_handle();
    let mut isolate_scope = v8::HandleScope::new(&mut *isolate);
    let context = v8::Local::new(&mut isolate_scope, &me.v8_ctx);
    let scope = &mut v8::ContextScope::new(&mut isolate_scope, context);
    let scope = &mut v8::HandleScope::new(scope);

    let mut cancel = me.get_cancel();

    // I'm not clear about the guarantees of Tokio `abort()` - so let's add our own guard, just to be sure.
    let abort_guard = Arc::new(AtomicBool::new(false));
    let abort_guard_2 = abort_guard.clone();
    let terminate_report = Arc::new(AtomicBool::new(false));
    let terminate_report_2 = terminate_report.clone();

    let computation_watcher = me.ctx.computation_watcher.spawn(async move {
      let _ = cancel.changed().await;

      // Grace period before performing a synchronous termination. Restarting the context is an
      // expensive operation.
      tokio::time::sleep(Duration::from_millis(100)).await;

      if !abort_guard.load(Ordering::Relaxed) {
        handle.terminate_execution();
        terminate_report.store(true, Ordering::Relaxed);
      }
    });
    let (ret, exc) = {
      let mut catch = v8::TryCatch::new(scope);
      let ret = f(&mut catch);
      (ret, catch.exception())
    };
    me.busy_duration
      .set(me.busy_duration.get() + start.elapsed());

    // Abort the task, and double-confirm.
    computation_watcher.abort();
    abort_guard_2.store(true, Ordering::Relaxed);

    // If this is an abnormal termination, don't reuse the context. And the exception is useless.
    if terminate_report_2.load(Ordering::Relaxed) {
      log::error!("Resetting V8 context of app {}.", me.ctx.key);
      me.ctx.reset_v8_context(scope);
      return None;
    } else {
      if let Some(x) = exc {
        log::error!(
          "app {}: uncaught exception: {}",
          me.ctx.key,
          x.to_rust_string_lossy(scope)
        );
        return None;
      }
    }
    Some(ret)
  }
}

impl AsyncKill {
  async fn wait(&self) {
    self.0.read().await;
  }
}
