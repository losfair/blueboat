#!/bin/bash

set -e
cd "$(dirname $0)"
cd pkg
npx esbuild index.ts --bundle --outfile=index.js --target=es2021
tar c . > ../pkg.tar
cd ..

RUST_LOG=debug S3CMD_CFG=~/blueboat_test_s3.creds ../scripts/build_and_upload.mjs \
  -f ./pkg.tar --s3_bucket apps --s3_prefix pkg/ \
  --kv ./pkg.kv.json
