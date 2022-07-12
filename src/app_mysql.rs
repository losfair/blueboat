use std::time::{Duration, SystemTime};

use crate::v8util::create_uint8array_from_bytes;
use anyhow::Result;
use mysql_async::{Pool, Value};
use num_derive::FromPrimitive;
use std::convert::TryFrom;
use thiserror::Error;
use time::{Date, PrimitiveDateTime, Time};
use v8;

pub struct AppMysql {
  pool: Pool,
}

#[derive(FromPrimitive, Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
pub enum ValueSpec {
  Int = b'i',
  BigInt = b'I',
  Float = b'f',
  String = b's',
  Binary = b'b',
  Date = b'd',
}

#[derive(Error, Debug)]
#[error("mysql value cast error: {0}")]
struct CastError(&'static str);

impl AppMysql {
  pub fn new(pool: Pool) -> Self {
    Self { pool }
  }

  pub fn pool(&self) -> &Pool {
    &self.pool
  }

  pub fn cast_value_to_js<'s>(
    scope: &mut v8::HandleScope<'s>,
    v: &Value,
    spec: ValueSpec,
  ) -> Result<v8::Local<'s, v8::Value>> {
    #[derive(Error, Debug)]
    #[error("value too large")]
    struct ValueTooLarge;

    #[derive(Error, Debug)]
    #[error("spec type '{0}' does not match row type")]
    struct SpecMismatch(String);

    if matches!(v, Value::NULL) {
      return Ok(v8::null(scope).into());
    }

    let gen_mm = || anyhow::Error::from(SpecMismatch(format!("{:?}", spec)));

    match spec {
      ValueSpec::Binary => match v {
        Value::Bytes(x) => {
          let view = create_uint8array_from_bytes(scope, x);
          Ok(view.into())
        }
        _ => Err(gen_mm()),
      },
      ValueSpec::Int => match v {
        Value::Int(x) => Ok(v8::Number::new(scope, i32::try_from(*x)? as f64).into()),
        Value::UInt(x) => Ok(v8::Number::new(scope, u32::try_from(*x)? as f64).into()),
        _ => Err(gen_mm()),
      },
      ValueSpec::Float => match v {
        Value::Float(x) => Ok(v8::Number::new(scope, *x as f64).into()),
        Value::Double(x) => Ok(v8::Number::new(scope, *x).into()),
        _ => Err(gen_mm()),
      },
      ValueSpec::BigInt => match v {
        Value::Int(x) => Ok(v8::BigInt::new_from_i64(scope, *x).into()),
        Value::UInt(x) => Ok(v8::BigInt::new_from_u64(scope, *x).into()),
        _ => Err(gen_mm()),
      },
      ValueSpec::String => match v {
        Value::Bytes(x) => Ok(
          v8::String::new_from_utf8(scope, x, v8::NewStringType::Normal)
            .ok_or(ValueTooLarge)?
            .into(),
        ),
        _ => Err(gen_mm()),
      },
      ValueSpec::Date => match v {
        Value::Date(year, month, day, hour, minutes, seconds, microseconds) => {
          let date = Date::try_from_ymd(*year as i32, *month, *day)?;
          let t = Time::try_from_hms_micro(*hour, *minutes, *seconds, *microseconds)?;
          let dt_ms = PrimitiveDateTime::new(date, t)
            .assume_utc()
            .unix_timestamp_nanos()
            / 1000_000;
          let ts = if let Ok(x) = u64::try_from(dt_ms) {
            x
          } else {
            0
          };
          let dt =
            v8::Date::new(scope, ts as f64).ok_or(CastError("cannot build date from timestamp"))?;
          Ok(dt.into())
        }
        _ => Err(gen_mm()),
      },
    }
  }

  pub fn cast_value_from_js<'s>(
    scope: &mut v8::HandleScope<'s>,
    x: v8::Local<'s, v8::Value>,
    spec: ValueSpec,
  ) -> Result<Value> {
    if x.is_null_or_undefined() {
      return Ok(Value::NULL);
    }

    match spec {
      ValueSpec::BigInt => {
        let x = v8::Local::<v8::BigInt>::try_from(x)?;
        if let (x, true) = x.i64_value() {
          Ok(Value::Int(x))
        } else if let (x, true) = x.u64_value() {
          Ok(Value::UInt(x))
        } else {
          Err(CastError("bigint out of range").into())
        }
      }
      ValueSpec::Binary => {
        let x = v8::Local::<v8::Uint8Array>::try_from(x)?;
        let mut buf = vec![0u8; x.byte_length()];
        x.copy_contents(&mut buf);
        Ok(Value::Bytes(buf))
      }
      ValueSpec::String => {
        let x = v8::Local::<v8::String>::try_from(x)?;
        let mut buf = vec![0u8; x.utf8_length(scope)];
        x.write_utf8(
          scope,
          &mut buf,
          None,
          v8::WriteOptions::NO_NULL_TERMINATION | v8::WriteOptions::REPLACE_INVALID_UTF8,
        );
        Ok(Value::Bytes(buf))
      }
      ValueSpec::Date => {
        let x = v8::Local::<v8::Date>::try_from(x)?;
        let ts = x.value_of();
        let dt = SystemTime::UNIX_EPOCH
          .checked_add(Duration::from_millis(ts as u64))
          .ok_or(CastError("invalid timestamp"))?;
        let dt = PrimitiveDateTime::from(dt);
        let dt = Value::Date(
          dt.year() as _,
          dt.month(),
          dt.day(),
          dt.hour(),
          dt.minute(),
          dt.second(),
          dt.microsecond(),
        );
        Ok(dt)
      }
      ValueSpec::Int => {
        let x = v8::Local::<v8::Number>::try_from(x)?.value();
        if x >= 0.0 && x <= u32::MAX as f64 && x.trunc() == x {
          Ok(Value::UInt(x as u64))
        } else if x >= i32::MIN as f64 && x <= i32::MAX as f64 && x.trunc() == x {
          Ok(Value::Int(x as i64))
        } else {
          Err(CastError("int out of range").into())
        }
      }
      ValueSpec::Float => {
        let x = v8::Local::<v8::Number>::try_from(x)?.value();
        Ok(Value::Double(x))
      }
    }
  }
}
