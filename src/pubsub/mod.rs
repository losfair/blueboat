use std::sync::Arc;

use once_cell::sync::OnceCell;

use self::mq::MessageQueue;

pub mod mq;
pub mod sse;

pub static MQ: OnceCell<Arc<MessageQueue>> = OnceCell::new();
