use rusty_v8 as v8;

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
        Self {
            kind,
            message,
        }
    }

    pub fn type_error() -> Self {
        Self::new(JsErrorKind::TypeError, None)
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

pub fn wrap_callback<'s, F: FnOnce(&mut v8::HandleScope<'s>) -> JsResult<()>>(scope: &mut v8::HandleScope<'s>, f: F) {
    match f(scope) {
        Ok(()) => {}
        Err(e) => {
            let exception = e.build(scope);
            scope.throw_exception(exception);
        }
    }
}