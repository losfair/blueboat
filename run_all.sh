#!/bin/sh

export RUST_LOG=rusty_workers=info,rusty_workers_fetchd=info,rusty_workers_runtime=info,rusty_workers_proxy=info

# https://stackoverflow.com/questions/360201/how-do-i-kill-background-processes-jobs-when-my-shell-script-exits
trap "exit" INT TERM
trap "kill 0" EXIT

../pd/bin/pd-server --name=pd1 \
    --data-dir=playground/pd1 \
    --client-urls="http://127.0.0.1:2379" \
    --peer-urls="http://127.0.0.1:2380" \
    --initial-cluster="pd1=http://127.0.0.1:2380" \
    --log-file=playground/pd1.log &

sleep 5

../tikv/target/release/tikv-server --pd-endpoints="127.0.0.1:2379" \
    --addr="127.0.0.1:20160" \
    --status-addr="127.0.0.1:20181" \
    --data-dir=playground/tikv1 \
    --log-file=playground/tikv1.log &

sleep 1

cd playground
./build_bundles.sh
cd ..

./target/release/rusty-workers-fetchd --rpc-listen 127.0.0.1:3000 &
./target/release/rusty-workers-runtime --rpc-listen 127.0.0.1:3001 \
    --tikv-cluster 127.0.0.1:2379 \
    --max-num-of-instances 50 \
    --isolate-pool-size 60 \
    --execution-concurrency 20 \
    --max-concurrent-requests 50 &
./target/release/rusty-workers-proxy \
    --fetch-service 127.0.0.1:3000 \
    --http-listen 0.0.0.0:3080 \
    --tikv-cluster 127.0.0.1:2379 \
    --runtimes 127.0.0.1:3001 \
    --dropout-rate 0.0002 \
    --max-time-ms 2000 \
    --max-ready-instances-per-app 80 &

wait
