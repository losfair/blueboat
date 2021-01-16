use rusty_v8 as v8;
use rusty_workers::types::*;

#[derive(Debug)]
pub struct JsError {
    pub kind: JsErrorKind,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsErrorKind {
    Error,
    TypeError,
    ReferenceError,
}

pub type JsResult<T> = Result<T, JsError>;

impl JsError {
    pub fn new(kind: JsErrorKind, message: Option<String>) -> Self {
        Self { kind, message }
    }

    pub fn error<M: Into<String>>(m: M) -> Self {
        Self::new(JsErrorKind::Error, Some(m.into()))
    }

    pub fn build<'s>(&self, scope: &mut v8::HandleScope<'s>) -> v8::Local<'s, v8::Value> {
        use JsErrorKind as K;
        let message = match self.message {
            Some(ref x) => v8::String::new(scope, x).unwrap_or_else(|| {
                debug!("JsError::build: cannot build error message");
                v8::String::empty(scope)
            }),
            None => v8::String::empty(scope),
        };
        match self.kind {
            K::Error => v8::Exception::error(scope, message),
            K::TypeError => v8::Exception::type_error(scope, message),
            K::ReferenceError => v8::Exception::reference_error(scope, message),
        }
    }
}

impl From<GenericError> for JsError {
    fn from(other: GenericError) -> Self {
        match other {
            GenericError::Conversion => {
                Self::new(JsErrorKind::TypeError, Some("conversion error".into()))
            }
            GenericError::Typeck { expected } => Self::new(
                JsErrorKind::TypeError,
                Some(format!("typeck error: expected {}", expected)),
            ),
            GenericError::Execution(ExecutionError::MemoryLimitExceeded) => {
                Self::new(JsErrorKind::Error, Some(format!("memory limit exceeded")))
            }
            _ => Self::new(JsErrorKind::Error, Some("generic error".into())),
        }
    }
}

impl From<v8::DataError> for JsError {
    fn from(other: v8::DataError) -> Self {
        match other {
            v8::DataError::BadType { actual, expected } => Self::new(
                JsErrorKind::TypeError,
                Some(format!("expected {}, got {}", expected, actual)),
            ),
            v8::DataError::NoData { expected } => Self::new(
                JsErrorKind::ReferenceError,
                Some(format!("expected {}", expected)),
            ),
        }
    }
}
