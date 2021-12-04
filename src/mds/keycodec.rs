use anyhow::Result;

pub fn encode_path(path: &str) -> Result<String> {
  let path = path.as_bytes().split(|&x| x == b'/').collect::<Vec<_>>();
  let mut segs: Vec<Vec<u8>> = Vec::with_capacity(path.len());
  for seg in path {
    if seg.contains(&0x00) {
      anyhow::bail!("path segment contains null byte");
    }
    let mut out = Vec::with_capacity(seg.len() + 2);
    out.push(0x02);
    out.extend_from_slice(seg);
    out.push(0x00);
    segs.push(out);
  }
  Ok(String::from_utf8(segs.concat()).unwrap())
}

pub fn decode_path(path: &str) -> Result<String> {
  let mut out = String::new();
  let mut i = 0;
  let path = path.as_bytes();
  while i < path.len() {
    if path[i] != 0x02 {
      anyhow::bail!("path segment does not start with 0x02");
    }
    let mut j = i + 1;
    while j < path.len() && path[j] != 0x00 {
      j += 1;
    }
    if j == path.len() {
      anyhow::bail!("path segment does not end with 0x00");
    }
    out.push_str(std::str::from_utf8(&path[i + 1..j])?);
    out.push('/');
    i = j + 1;
  }
  out.pop();
  Ok(out)
}
