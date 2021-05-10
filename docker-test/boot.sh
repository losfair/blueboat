#!/bin/bash

echo "Container boot script for CI testing."
echo "NOT YET IMPLEMENTED"
exit 0

nginx

/usr/bin/rusty-workers-fetchd --rpc-listen 127.0.0.1:3000 &

/usr/bin/rusty-workers-runtime --rpc-listen 127.0.0.1:3001 \
    --max-concurrent-requests 20 \
    --max-num-of-instances 50 \
    &

/usr/bin/rusty-workers-proxy --http-listen 0.0.0.0:8080 \
    --config http://127.0.0.1/config.toml \
    --fetch-service 127.0.0.1:3000 \
    --runtimes 127.0.0.1:3001 \
    &

sleep 10 # Wait for things to start

curl http://localhost:8080/hello | grep "Hello, world!"
if [ ! "$?" = "0" ]; then
    echo "Got unexpected response from server."
    exit 1
fi
