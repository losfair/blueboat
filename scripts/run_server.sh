#!/bin/bash

set -e

if [ "$DEBUG_MODE" = "1" ]; then
  export RUST_LOG=debug
  target_subdir=debug
else
  export RUST_LOG=info
  target_subdir=release
fi

AWS_ACCESS_KEY_ID=minioadmin AWS_SECRET_ACCESS_KEY=minioadmin \
  SMRAPP_BLUEBOAT_FONT_DIR=~/Projects/blueboat-fonts/ \
  exec ./target/$target_subdir/blueboat_server \
  -l 127.0.0.1:2290 \
  --s3-bucket test --s3-region us-east-1 --s3-endpoint http://127.0.0.1:9000 \
  --db "$MYSQL_CONN_STRING" \
  --mmdb-city ~/Projects/blueboat-mmdb/GeoLite2-City.mmdb \
  --wpbl-db ~/Projects/blueboat-wpbl/wpbl.db
