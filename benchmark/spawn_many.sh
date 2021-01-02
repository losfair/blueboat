#!/bin/bash

BINARY="$(dirname $0)/../target/release/rusty-workers-cli"

bench_once()
{
    echo "bench $1"
    worker_id=`("$BINARY" runtime spawn "$(dirname $0)/hello_world.js" | jq --raw-output ".Ok.id") || return 1`
    for j in {1..10}; do
        #echo "bench $1: worker_id $worker_id"
        output=`"$BINARY" runtime fetch "$worker_id" || return 1`
        http_status=`(echo "$output" | jq ".Ok.status") || return 1`
        if [ "$http_status" != "200" ]; then
            echo "[-] Bad http status: $output"
            return 1
        fi
    done
    "$BINARY" runtime terminate "$worker_id" > /dev/null || return 1
    echo "bench $1 succeeded"
}

while [ "1" = "1" ]; do
    for i in {1..50}; do
        bench_once $i &
        sleep 0.01
    done
    wait
done
