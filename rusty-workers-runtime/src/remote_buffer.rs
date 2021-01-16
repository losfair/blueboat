//! A group of utilities to help with handling remote arraybuffers safely in case of asynchronous
//! cancellation.

use rusty_v8 as v8;
use send_wrapper::SendWrapper;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

type RemoteBufferStrongRef = Arc<Mutex<Option<SendWrapper<v8::Global<v8::ArrayBuffer>>>>>;
type RemoteBufferWeakRef = Weak<Mutex<Option<SendWrapper<v8::Global<v8::ArrayBuffer>>>>>;

pub struct RemoteBufferSet {
    /// Strong references to the buffers. Indexed by their addresses.
    buffers: BTreeMap<usize, RemoteBufferStrongRef>,
}

pub struct RemoteBuffer {
    buffer: RemoteBufferWeakRef,
    backing: v8::SharedRef<v8::BackingStore>,
}

impl RemoteBuffer {
    pub fn backing(&self) -> &v8::SharedRef<v8::BackingStore> {
        &self.backing
    }

    pub fn unwrap_on_v8_thread(self) -> Option<v8::Global<v8::ArrayBuffer>> {
        if let Some(buffer) = self.buffer.upgrade() {
            Some(buffer.try_lock().unwrap().take().unwrap().take())
        } else {
            None
        }
    }
}

impl RemoteBufferSet {
    pub fn new() -> RemoteBufferSet {
        Self {
            buffers: BTreeMap::new(),
        }
    }
    pub fn allocate(
        &mut self,
        scope: &mut v8::HandleScope<'_>,
        len: usize,
    ) -> Option<RemoteBuffer> {
        if crate::mm::acquire_arraybuffer_precheck(scope, len).is_err() {
            None
        } else {
            let buf = v8::ArrayBuffer::new(scope, len);
            let backing = buf.get_backing_store();
            let buf = v8::Global::new(scope, buf);

            let buf = Arc::new(Mutex::new(Some(SendWrapper::new(buf))));
            let addr = Arc::as_ptr(&buf) as usize;
            let buf_weak = Arc::downgrade(&buf);
            self.buffers.insert(addr, buf);

            Some(RemoteBuffer {
                buffer: buf_weak,
                backing,
            })
        }
    }

    pub fn gc(&mut self) {
        let mut to_remove: Vec<usize> = vec![];
        for (&k, v) in self.buffers.iter() {
            if Arc::weak_count(v) == 0 {
                to_remove.push(k);
            }
        }

        if to_remove.len() > 0 {
            debug!(
                "RemoteBufferSet::gc: removing {} unused buffers",
                to_remove.len()
            );
            for k in to_remove {
                self.buffers.remove(&k);
            }
        }
    }
}
