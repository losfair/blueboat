let t = createPrimaryTransaction();
let c = data.checks;
let ok = c.map(x => t.Get(x[0])).map(x => base64Encode(x.Wait()))
  .reduce((a, b, i) => a && b === c[i][1], true);
if(ok) {
  data.sets.forEach(([k, v]) => {
    if(v === null) t.Delete(k);
    else t.Set(k, base64Decode(v));
  });
  data.versionstamped_sets.forEach(([origKey, v]) => {
    const origBytes = new Uint8Array(stringToArrayBuffer(origKey));
    const key = new Uint8Array(origBytes.length + 11);
    key.set(origBytes);
    key[origBytes.length] = 0x32; // Type tag for versionstamp
    t.SetVersionstampedKey(key.buffer, base64Decode(v), origBytes.length + 1);
  });
  const committed = t.Commit().Wait();
  output = {
    versionstamp: base64Encode(committed.Versionstamp),
  };
} else {
  output = null;
}
