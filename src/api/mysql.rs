use std::{collections::HashMap, rc::Weak, sync::Arc};

use anyhow::Result;
use mysql_async::{prelude::Queryable, TxOpts};
use num_traits::FromPrimitive;
use std::convert::TryFrom;
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use v8;

use crate::{
  api::util::v8_invoke_callback,
  app_mysql::{AppMysql, ValueSpec},
  exec::{Executor, ExecutorMysqlState},
  v8util::FunctionCallbackArgumentsExt,
};

#[derive(Error, Debug)]
#[error("bad spec")]
struct BadSpec;

pub fn api_mysql_exec(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  #[derive(Error, Debug)]
  #[error("unknown error in mysql_exec")]
  struct Unknown;

  #[derive(Error, Debug)]
  #[error("bad query parameter")]
  struct BadQueryParam;

  let key = v8::Local::<v8::String>::try_from(args.get(1))?.to_rust_string_lossy(scope);
  let stmt = v8::Local::<v8::String>::try_from(args.get(2))?.to_rust_string_lossy(scope);
  let sql_args = v8::Local::<v8::Object>::try_from(args.get(3))?;
  let spec = args.get(4);
  let spec = spec
    .to_rust_string_lossy(scope)
    .as_bytes()
    .iter()
    .copied()
    .map(ValueSpec::from_u8)
    .collect::<Option<Vec<ValueSpec>>>()
    .ok_or(BadSpec)?;
  let callback = v8::Global::new(scope, args.load_function_at(5)?);
  let mut arg_map: HashMap<String, mysql_async::Value> = HashMap::new();
  let prop_names = sql_args.get_own_property_names(scope).ok_or(Unknown)?;
  let prop_count = prop_names.length();
  for i in 0..prop_count {
    let k = v8::Local::<v8::String>::try_from(prop_names.get_index(scope, i).ok_or(Unknown)?)?;
    let v = sql_args.get(scope, k.into()).ok_or(Unknown)?;
    let v = v8::Local::<v8::Array>::try_from(v)?;
    let spec = v
      .get_index(scope, 0)
      .ok_or(BadQueryParam)?
      .to_rust_string_lossy(scope)
      .as_bytes()
      .get(0)
      .copied()
      .ok_or(BadQueryParam)?;
    let spec = ValueSpec::from_u8(spec).ok_or(BadSpec)?;
    let v = v.get_index(scope, 1).ok_or(BadQueryParam)?;
    let k = k.to_rust_string_lossy(scope);
    let v = AppMysql::cast_value_from_js(scope, v, spec)?;
    arg_map.insert(k, v);
  }
  let exec = Executor::try_current_result()?;
  let exec_2 = exec.clone();
  Executor::spawn(&exec_2, async move {
    let res = run_mysql(&exec, key, stmt, arg_map).await;
    Executor::enter(&exec, move |scope| {
      let res = decode_mysql(scope, res, spec);
      v8_invoke_callback("mysql_exec", scope, res, &callback);
    });
  });
  Ok(())
}

pub fn api_mysql_start_transaction(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let key = v8::Local::<v8::String>::try_from(args.get(1))?.to_rust_string_lossy(scope);
  let callback = v8::Global::new(scope, args.load_function_at(2)?);
  let exec = Executor::try_current_result()?;
  let exec_2 = exec.clone();
  Executor::spawn(&exec_2, async move {
    let conn = get_mysql_state(&exec, &key).await;
    let res = match conn {
      Ok(mut x) => {
        let txn = x.pool.start_transaction(TxOpts::new()).await;
        match txn {
          Ok(txn) => {
            x.txn = Some(txn);
            Ok(())
          }
          Err(e) => Err(anyhow::Error::from(e)),
        }
      }
      Err(e) => Err(e),
    };
    Executor::enter(&exec, move |scope| {
      let res = res.map(|_| v8::Local::<v8::Value>::from(v8::undefined(scope)));
      v8_invoke_callback("mysql_start_transaction", scope, res, &callback);
    });
  });
  Ok(())
}

pub fn api_mysql_end_transaction(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  _retval: v8::ReturnValue,
) -> Result<()> {
  let key = v8::Local::<v8::String>::try_from(args.get(1))?.to_rust_string_lossy(scope);
  let commit = v8::Local::<v8::Boolean>::try_from(args.get(2))?.boolean_value(scope);
  let callback = v8::Global::new(scope, args.load_function_at(3)?);
  let exec = Executor::try_current_result()?;
  let exec_2 = exec.clone();
  Executor::spawn(&exec_2, async move {
    let conn = get_mysql_state(&exec, &key).await;
    let res = match conn {
      Ok(mut x) => {
        if let Some(txn) = x.txn.take() {
          if commit {
            txn.commit().await.map_err(anyhow::Error::from)
          } else {
            txn.rollback().await.map_err(anyhow::Error::from)
          }
        } else {
          Ok(())
        }
      }
      Err(e) => Err(e),
    };
    Executor::enter(&exec, move |scope| {
      let res = res.map(|_| v8::Local::<v8::Value>::from(v8::undefined(scope)));
      v8_invoke_callback("mysql_end_transaction", scope, res, &callback);
    });
  });
  Ok(())
}

fn decode_mysql<'s>(
  scope: &mut v8::HandleScope<'s>,
  res: Result<Vec<Vec<mysql_async::Value>>>,
  spec: Vec<ValueSpec>,
) -> Result<v8::Local<'s, v8::Value>> {
  #[derive(Error, Debug)]
  #[error("cannot decode result column at row {0} column {1}: {2}")]
  struct CastRichError(usize, usize, anyhow::Error);

  #[derive(Error, Debug)]
  #[error("spec length of {0} does not match row length of {1}")]
  struct SpecLengthMismatch(usize, usize);

  let res = res?;

  let mut out: Vec<v8::Local<v8::Value>> = Vec::with_capacity(res.len());
  for (i, x) in res.iter().enumerate() {
    if x.len() != spec.len() {
      return Err(SpecLengthMismatch(spec.len(), x.len()).into());
    }

    let mut buf: Vec<v8::Local<v8::Value>> = Vec::with_capacity(x.len());
    for (j, x) in x.iter().enumerate() {
      let x = match AppMysql::cast_value_to_js(scope, x, spec[j]) {
        Ok(x) => x,
        Err(e) => {
          return Err(CastRichError(i, j, e).into());
        }
      };
      buf.push(x);
    }
    out.push(v8::Array::new_with_elements(scope, &buf).into());
  }

  Ok(v8::Array::new_with_elements(scope, &out).into())
}

async fn run_mysql(
  e: &Weak<Executor>,
  key: String,
  stmt: String,
  args: HashMap<String, mysql_async::Value>,
) -> Result<Vec<Vec<mysql_async::Value>>> {
  let mut state = get_mysql_state(e, &key).await?;
  let params = if args.is_empty() {
    mysql_async::Params::Empty
  } else {
    mysql_async::Params::Named(args)
  };
  let res = if let Some(txn) = &mut state.txn {
    txn.exec_iter(&stmt, params)
  } else {
    let conn = state.ensure_conn().await?;
    conn.exec_iter(&stmt, params)
  }
  .await?;
  let mut out: Vec<Vec<mysql_async::Value>> = vec![];
  res
    .for_each_and_drop(|x| {
      out.push(x.unwrap());
    })
    .await?;
  Ok(out)
}

async fn get_mysql_state(
  e: &Weak<Executor>,
  k: &str,
) -> Result<OwnedMutexGuard<ExecutorMysqlState>> {
  #[derive(Error, Debug)]
  #[error("no such mysql connection")]
  struct NoSuchConn;

  // Keep `Rc<Executor>` alive as short as possible
  let (m, ctx) = match e.upgrade() {
    Some(x) => (x.mysql.clone(), x.ctx),
    None => return Err(NoSuchConn.into()),
  };

  let mut m = m.lock().await;
  if let Some(x) = m.get(k) {
    let x = x.clone();
    drop(m);
    return Ok(x.lock_owned().await);
  }

  let (k, v) = match ctx.mysql.get_key_value(k) {
    Some(x) => x,
    None => return Err(NoSuchConn.into()),
  };

  let state = ExecutorMysqlState {
    conn: None,
    txn: None,
    pool: v.pool(),
  };
  let state = Arc::new(AsyncMutex::new(state));
  let g = state.clone().try_lock_owned().unwrap();
  m.insert(k.as_str(), state);
  drop(m);

  Ok(g)
}

impl ExecutorMysqlState {
  async fn ensure_conn(&mut self) -> Result<&mut mysql_async::Conn> {
    if self.conn.is_none() {
      let conn = self.pool.get_conn().await?;
      self.conn = Some(conn);
    }
    Ok(self.conn.as_mut().unwrap())
  }
}
