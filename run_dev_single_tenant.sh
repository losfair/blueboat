#!/bin/bash

set -e

export RUST_LOG=info

./target/release/blueboat_server \
  -l "0.0.0.0:3001" \
  --mds mt=/etc/foundationdb/fdb.cluster:mt \
  --pubsub-cluster /etc/foundationdb/fdb.cluster \
  --pubsub-prefix mt-pubsub \
  --single-tenant "$1"
