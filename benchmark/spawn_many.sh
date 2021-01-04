#!/bin/bash

BINARY="$(dirname $0)/../target/release/rusty-workers-cli"
SCRIPT="$1"

if [ -z "$SCRIPT" ]; then
    echo "[-] Missing script"
    exit 1
fi

bench_once()
{
    #echo "bench $1"
    worker_id=`("$BINARY" runtime spawn "$SCRIPT" | jq --raw-output ".Ok.id") || return 1`
    for j in {1..50}; do
        #echo "bench $1: worker_id $worker_id"
        output=`"$BINARY" runtime fetch "$worker_id" || return 1`
        http_status=`(echo "$output" | grep -F '"status":200') || return 1`
    done
    if [ "$(($RANDOM % 2))" = "1" ]; then
        "$BINARY" runtime terminate "$worker_id" > /dev/null || return 1
        #echo "terminated"
    else
        true
        #echo "not terminating"
    fi
    #echo "bench $1 succeeded"
}

while [ "1" = "1" ]; do
    start_time=$SECONDS
    for k in {1..10}; do
        for i in {1..150}; do
            bench_once $i &
            sleep 0.05
        done
        wait
    done
    elapsed=$(($SECONDS - $start_time))
    echo "Time of last 10 rounds: $elapsed"
done
