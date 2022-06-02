use anyhow::Result;
use indexmap::IndexMap;

pub struct MdsConfig {
  pub clusters: IndexMap<String, MdsClusterConfig>,
}

#[derive(Clone)]
pub struct MdsClusterConfig {
  pub path: String,
  pub prefix: String,
}

impl MdsConfig {
  pub fn parse(input: &str) -> Result<Self> {
    let mut clusters: IndexMap<String, MdsClusterConfig> = Default::default();
    for seg in input.split(",") {
      let seg = seg.trim();
      if let Some((name, entry)) = seg.trim().split_once("=") {
        if let Some((path, prefix)) = entry.split_once(":") {
          let entry = MdsClusterConfig {
            path: path.to_string(),
            prefix: prefix.to_string(),
          };
          clusters.insert(name.to_string(), entry);
        } else {
          anyhow::bail!("invalid cluster entry: {}", seg);
        }
      } else {
        anyhow::bail!("invalid cluster config: {}", seg);
      }
    }
    Ok(Self { clusters })
  }
}
