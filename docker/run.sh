#!/bin/bash

set -e

if [ -z "$SMRAPP_BLUEBOAT_FONT_DIR" ]; then
  export SMRAPP_BLUEBOAT_FONT_DIR="/opt/blueboat/fonts"
fi

exec /usr/bin/blueboat_server -l "$LISTEN_ADDR" \
  --s3-bucket "$S3_BUCKET" --s3-region "$S3_REGION" \
  --s3-endpoint "$S3_ENDPOINT" \
  --db "$MYSQL_CONN_STRING" \
  --mmdb-city "/opt/blueboat/mmdb/GeoLite2-City.mmdb" \
  --wpbl-db "/opt/blueboat/wpbl/wpbl.db"
