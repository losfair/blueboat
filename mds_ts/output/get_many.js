let t=data.primary?createPrimaryTransaction():createReplicaTransaction();output=data.keys.map(a=>t.Get(a)).map(a=>base64Encode(a.Wait()));
