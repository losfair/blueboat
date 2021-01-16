use crate::error::*;
use rusty_v8 as v8;
use std::cell::Cell;
use std::convert::TryFrom;
use std::ops::Deref;
use std::ops::Range;

pub struct JsArrayBufferViewRef {
    backing: v8::SharedRef<v8::BackingStore>,
    range: Range<usize>,
}

impl JsArrayBufferViewRef {
    pub fn new(scope: &mut v8::HandleScope<'_>, other: v8::Local<'_, v8::Value>) -> JsResult<Self> {
        if let Ok(x) = v8::Local::<'_, v8::ArrayBuffer>::try_from(other) {
            let backing = x.get_backing_store();
            let len = backing.len();
            Ok(Self {
                backing,
                range: 0..len,
            })
        } else if let Ok(x) = v8::Local::<'_, v8::ArrayBufferView>::try_from(other) {
            let buf = x.buffer(scope).ok_or_else(|| {
                JsError::new(
                    JsErrorKind::TypeError,
                    Some("cannot get backing ArrayBuffer".into()),
                )
            })?;
            let backing = buf.get_backing_store();
            Ok(Self {
                backing,
                range: x.byte_offset()..x.byte_offset() + x.byte_length(),
            })
        } else {
            Err(JsError::new(
                JsErrorKind::TypeError,
                Some("value cannot be converted to JsArrayBufferViewRef".into()),
            ))
        }
    }

    pub fn read_to_vec(&self, max_length: usize) -> Option<Vec<u8>> {
        let source: &[Cell<u8>] = self;
        if source.len() > max_length {
            return None;
        }
        let mut buf = Vec::with_capacity(source.len());
        buf.extend(source.iter().map(|x| x.get()));
        Some(buf)
    }
}

impl Deref for JsArrayBufferViewRef {
    type Target = [Cell<u8>];
    fn deref(&self) -> &Self::Target {
        let cells: &[Cell<u8>] = &self.backing;
        &cells[self.range.clone()]
    }
}
