use itertools::Itertools;
use parking_lot::{Mutex, MutexGuard};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::future::Future;
use std::hash::Hash;

pub struct GenerationalCache<K, V> {
  name: &'static str,
  inner: Mutex<CacheImpl<K, V>>,
}

pub struct CacheImpl<K, V> {
  fresh: HashMap<K, V>,
  young: HashMap<K, V>,
  old: HashMap<K, V>,
}

pub struct GenerationalCacheStats<K> {
  pub fresh: Vec<K>,
  pub young: Vec<K>,
  pub old: Vec<K>,
}

impl<K: Debug> Display for GenerationalCacheStats<K> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "fresh: {:?}\nyoung: {:?}\nold: {:?}\n",
      self.fresh, self.young, self.old
    )
  }
}

impl<K: Eq + Hash, V: Clone> CacheImpl<K, V> {
  pub fn get<Q>(&mut self, k: &Q) -> Option<V>
  where
    Q: ?Sized,
    K: Borrow<Q>,
    Q: Eq + Hash,
  {
    if let Some(v) = self.fresh.get(k) {
      return Some(v.clone());
    }

    if let Some(v) = self.young.get(k) {
      return Some(v.clone());
    }

    // old generation can be promoted to young generation
    if let Some((k, v)) = self.old.remove_entry(k) {
      self.young.insert(k, v.clone());
      return Some(v);
    }

    None
  }

  pub fn insert(&mut self, k: K, v: V) {
    self.young.remove(&k);
    self.old.remove(&k);
    self.fresh.insert(k, v);
  }

  pub fn remove<Q>(&mut self, k: &Q)
  where
    Q: ?Sized,
    K: Borrow<Q>,
    Q: Eq + Hash,
  {
    self.fresh.remove(k);
    self.young.remove(k);
    self.old.remove(k);
  }
}

impl<K: Eq + Hash, V: Clone> GenerationalCache<K, V> {
  pub fn new(name: &'static str) -> Self {
    Self {
      name,
      inner: Mutex::new(CacheImpl {
        fresh: HashMap::new(),
        young: HashMap::new(),
        old: HashMap::new(),
      }),
    }
  }

  pub fn lock<'a>(&'a self) -> MutexGuard<'a, CacheImpl<K, V>> {
    self.inner.lock()
  }

  pub fn get<Q>(&self, k: &Q) -> Option<V>
  where
    Q: ?Sized,
    K: Borrow<Q>,
    Q: Eq + Hash,
  {
    self.inner.lock().get(k)
  }

  pub fn insert(&self, k: K, v: V) {
    self.inner.lock().insert(k, v)
  }

  pub fn remove<Q>(&self, k: &Q)
  where
    Q: ?Sized,
    K: Borrow<Q>,
    Q: Eq + Hash,
  {
    self.inner.lock().remove(k)
  }
}

impl<K: Eq + Hash + Clone, V: Clone> GenerationalCache<K, V> {
  pub fn stats(&self) -> GenerationalCacheStats<K> {
    let inner = self.inner.lock();
    GenerationalCacheStats {
      fresh: inner.fresh.keys().cloned().collect_vec(),
      young: inner.young.keys().cloned().collect_vec(),
      old: inner.old.keys().cloned().collect_vec(),
    }
  }

  /// Only preserve the fresh generation.
  pub fn emergency_evict(&self) {
    let mut inner = self.inner.lock();
    let old_size = inner.old.len();
    let young_size = inner.young.len();
    inner.old = HashMap::new();
    inner.young = HashMap::new();
    if old_size != 0 || young_size != 0 {
      log::warn!(
        "GenerationalCache[{}]: Emergency eviction: evicted {} old entries and {} young entries.",
        self.name,
        old_size,
        young_size
      );
    }
  }

  pub async fn sweep<F: FnMut(K, V) -> Fut, Fut: Future<Output = Option<V>>>(&self, mut f: F) {
    let evicted: usize;

    // `preserved` will contain the previous young generation.
    let mut preserved = {
      let mut inner = self.inner.lock();

      // Retire the old generation & move young->old, fresh->young
      evicted = inner.old.len();
      inner.old = std::mem::replace(&mut inner.young, HashMap::new());
      inner.young = std::mem::replace(&mut inner.fresh, HashMap::new());
      inner
        .old
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect_vec()
    };

    for (k, v) in &mut preserved {
      *v = f(k.clone(), v.take().unwrap()).await;
    }

    // Refresh the cache.
    let mut inner = self.inner.lock();
    let total = preserved.len();
    let mut updated = 0usize;
    for (k, v) in preserved.into_iter() {
      if let Some(v) = v {
        if let Some(x) = inner.fresh.get_mut(&k) {
          *x = v;
          updated += 1;
        } else if let Some(x) = inner.young.get_mut(&k) {
          *x = v;
          updated += 1;
        } else if let Some(x) = inner.old.get_mut(&k) {
          *x = v;
          updated += 1;
        }
      } else {
        inner.fresh.remove(&k);
        inner.young.remove(&k);
        inner.old.remove(&k);
      }
    }
    if total != 0 || evicted != 0 {
      log::debug!(
        "GenerationalCache[{}]: Updated {}/{} preserved entries and evicted {} old entries.",
        self.name,
        updated,
        total,
        evicted,
      );
    }
  }
}
