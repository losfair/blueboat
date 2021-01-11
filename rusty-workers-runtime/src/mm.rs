use std::sync::atomic::{AtomicUsize, Ordering};
use std::ffi::c_void;
use rusty_v8 as v8;
use std::sync::Arc;

pub struct MemoryPool {
    remaining_bytes: AtomicUsize,
}

impl MemoryPool {
    pub fn new(n: usize) -> Arc<Self> {
        Arc::new(Self {
            remaining_bytes: AtomicUsize::new(n),
        })
    }

    /// Assuming that v8 background threads won't allocate...
    pub fn acquire_precheck(&self, n: usize) -> bool {
        if self.remaining_bytes.load(Ordering::Relaxed) < n {
            false
        } else {
            true
        }
    }

    pub fn reset(&self, n: usize) {
        self.remaining_bytes.store(n, Ordering::Relaxed);
    }

    pub fn acquire_bytes(&self, n: usize) -> bool {
        loop {
            let current = self.remaining_bytes.load(Ordering::Relaxed);
            if current < n {
                return false;
            }
    
            // TODO: Figure out a better ordering
            if self.remaining_bytes.compare_and_swap(current, current - n, Ordering::SeqCst) == current {
                return true;
            }
        }
    }

    pub fn release_bytes(&self, n: usize) {
        self.remaining_bytes.fetch_add(n, Ordering::SeqCst);
    }

    pub fn get_allocator(self: Arc<Self>) -> v8::UniqueRef<v8::Allocator> {
        unsafe {
            v8::new_rust_allocator(Arc::into_raw(self), VTABLE)
        }
    }
}

static VTABLE: &'static v8::RustAllocatorVtable<MemoryPool> = &v8::RustAllocatorVtable {
    allocate,
    allocate_uninitialized,
    free,
    reallocate,
    drop,
};

unsafe extern "C" fn allocate(pool: &MemoryPool, n: usize) -> *mut c_void {
    if !pool.acquire_bytes(n) {
        return std::ptr::null_mut();
    }
    Box::into_raw(vec![0u8; n].into_boxed_slice()) as *mut [u8] as *mut c_void
}

unsafe extern "C" fn allocate_uninitialized(pool: &MemoryPool, n: usize) -> *mut c_void {
    if !pool.acquire_bytes(n) {
        return std::ptr::null_mut();
    }
    let mut store = Vec::with_capacity(n);
    store.set_len(n);
    Box::into_raw(store.into_boxed_slice()) as *mut [u8] as *mut c_void
}

unsafe extern "C" fn free(pool: &MemoryPool, data: *mut c_void, n: usize) {
    pool.release_bytes(n);
    Box::from_raw(std::slice::from_raw_parts_mut(data as *mut u8, n));
}

unsafe extern "C" fn reallocate(
    pool: &MemoryPool,
    prev: *mut c_void,
    oldlen: usize,
    newlen: usize,
) -> *mut c_void {
    if newlen <= oldlen {
        pool.release_bytes(oldlen - newlen);
    } else {
        if !pool.acquire_bytes(newlen - oldlen) {
            return std::ptr::null_mut();
        }
    }
    let old_store =
        Box::from_raw(std::slice::from_raw_parts_mut(prev as *mut u8, oldlen));
    let mut new_store = Vec::with_capacity(newlen);
    let copy_len = oldlen.min(newlen);
    new_store.extend_from_slice(&old_store[..copy_len]);
    new_store.resize(newlen, 0u8);
    Box::into_raw(new_store.into_boxed_slice()) as *mut [u8] as *mut c_void
}

unsafe extern "C" fn drop(pool: *const MemoryPool) {
    Arc::from_raw(pool as *mut MemoryPool);
}