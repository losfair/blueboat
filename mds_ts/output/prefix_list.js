let t=data.primary?createPrimaryTransaction():createReplicaTransaction();output=t.PrefixList(data.prefix,data.opts).Collect().map(([a,e])=>[base64Encode(a),base64Encode(e)]);
