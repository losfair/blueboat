use std::collections::HashMap;

use itertools::Itertools;

pub fn group_kv_rows_by_prefix<'a>(
  rows: &[(String, Vec<u8>)],
) -> Vec<(&str, HashMap<&str, &[u8]>)> {
  rows
    .iter()
    .filter_map(|(k, v)| k.rsplit_once('/').map(|(key, prop)| (key, prop, v)))
    .group_by(|(k, _, _)| *k)
    .into_iter()
    .map(|(k, group)| {
      let mut props: HashMap<&str, &[u8]> = HashMap::new();
      for k in group {
        props.insert(k.1, k.2);
      }
      (k, props)
    })
    .collect()
}
