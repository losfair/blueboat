#!/bin/bash

set -e

export AWS_ACCESS_KEY_ID="minioadmin"
export AWS_SECRET_ACCESS_KEY="minioadmin"
export RUST_LOG=info
export SMRAPP_BLUEBOAT_DISABLE_SECCOMP=1

./target/release/blueboat_server \
  -l "0.0.0.0:3000" \
  --mds mt=/etc/foundationdb/fdb.cluster:mt \
  --s3-bucket "bb-mt" --s3-region "us-east-1" \
  --s3-endpoint "http://127.0.0.1:1932" \
  --log-kafka com.example.blueboat.applog:0@localhost:9092 \
  --syslog-kafka com.example.blueboat.syslog:0@localhost:9092
