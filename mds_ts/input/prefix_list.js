let t = data.primary ? createPrimaryTransaction() : createReplicaTransaction();
output = t.PrefixList(data.prefix, data.opts)
  .Collect()
  .map(([k, v]) => [base64Encode(k), base64Encode(v)]);
