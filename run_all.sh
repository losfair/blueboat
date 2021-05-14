#!/bin/sh

export RUST_LOG=rusty_workers=info,rusty_workers_fetchd=info,rusty_workers_runtime=info,rusty_workers_proxy=info

# https://stackoverflow.com/questions/360201/how-do-i-kill-background-processes-jobs-when-my-shell-script-exits
trap "exit" INT TERM
trap "kill 0" EXIT

cd playground
./build_bundles.sh
cd ..

./target/release/rusty-workers-fetchd --rpc-listen 127.0.0.1:3200 &
./target/release/rusty-workers-runtime --rpc-listen 127.0.0.1:3201 \
    --db-url mysql://root@localhost:4000/rusty_workers \
    --max-num-of-instances 50 \
    --isolate-pool-size 60 \
    --execution-concurrency 20 \
    --max-concurrent-requests 50 &
./target/release/rusty-workers-proxy \
    --fetch-service 127.0.0.1:3200 \
    --http-listen 0.0.0.0:3280 \
    --db-url mysql://root@localhost:4000/rusty_workers \
    --runtimes 127.0.0.1:3201 \
    --dropout-rate 0.0002 \
    --max-time-ms 2000 \
    --max-ready-instances-per-app 80 &

wait
