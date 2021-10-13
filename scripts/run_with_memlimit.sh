#!/bin/bash

if [ -z "$MYSQL_CONN_STRING" ]; then
  MYSQL_CONN_STRING=""
fi

set -eu

exec sudo systemd-run --scope \
  -p MemoryMax="$MEM_LIMIT" -p MemorySwapMax=0 \
  --setenv="MYSQL_CONN_STRING=$MYSQL_CONN_STRING" \
  --uid="$(whoami)" "$@"
