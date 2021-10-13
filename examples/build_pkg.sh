#!/bin/bash

set -e
cd "$(dirname $0)"
cd pkg
tar c . > ../pkg.tar
cd ..
RUST_LOG=debug S3CMD_CFG=~/minio.creds ../scripts/build_and_upload.mjs \
  -f ./pkg.tar --s3_bucket test --s3_prefix pkg/
