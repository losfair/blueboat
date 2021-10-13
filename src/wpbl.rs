use std::net::IpAddr;

use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use tokio::sync::Mutex;

pub struct WpblDb {
  conn: Mutex<Connection>,
}

impl WpblDb {
  pub fn open(path: &str) -> Result<Self> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(Self {
      conn: Mutex::new(conn),
    })
  }

  pub async fn in_blocklist(&self, ip: IpAddr) -> Result<bool> {
    let ip = match ip {
      IpAddr::V4(x) => hex::encode_upper(&x.octets()),
      IpAddr::V6(x) => hex::encode_upper(&x.octets()),
    };
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let conn = self.conn.lock().await;
    let mut stmt = conn.prepare_cached(
      r#"
    select rangestart, rangeend from wpbl
      where expiry > ? and rangestart <= ?
      order by rangestart desc limit 1
    "#,
    )?;
    let res: Option<(String, String)> = stmt
      .query_row(params![&now, &ip], |row| Ok((row.get(0)?, row.get(1)?)))
      .optional()?;
    if let Some((start, end)) = res {
      if ip >= start && ip <= end {
        Ok(true)
      } else {
        Ok(false)
      }
    } else {
      Ok(false)
    }
  }
}
