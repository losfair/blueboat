let t = createPrimaryTransaction();
let c = data.checks;
let ok = c.map(x => t.Get(x[0])).map(x => base64Encode(x.Wait()))
  .reduce((a, b, i) => a && b === c[i][1], true);
if(ok) {
  data.sets.forEach(([k, v]) => {
    if(v === null) t.Delete(k);
    else t.Set(k, base64Decode(v));
  });
  t.Commit().Wait();
}
output = ok;
