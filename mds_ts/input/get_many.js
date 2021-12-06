let t = data.primary ? createPrimaryTransaction() : createReplicaTransaction();
output = data.keys.map(k => t.Get(k))
  .map(x => base64Encode(x.Wait()));
