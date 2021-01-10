#!/bin/bash

TMP="/tmp/deploy-`uuidgen`"

if [ ! -f "$1" ]; then
    echo "[-] Missing deploy config"
    exit 1
fi

. "$1"

if [ -z "$REMOTE" ]; then
    echo "[-] Missing REMOTE"
    exit 1
fi

if [ -z "$TIKV_PD" ]; then
    echo "[-] Missing TIKV_PD"
    exit 1
fi

if [ -z "$2" ]; then
    echo "[-] Missing config"
    exit 1
fi

if [ -z "$3" ]; then
    echo "[-] Missing bundle"
    exit 1
fi

ssh "$REMOTE" "mkdir $TMP" || exit 1
scp "$2" "$REMOTE:$TMP/config.toml" || exit 1
scp "$3" "$REMOTE:$TMP/bundle.tar" || exit 1
ssh "$REMOTE" "RUST_LOG=info rusty-workers-cli app --tikv-pd \"$TIKV_PD\" add-app $TMP/config.toml --bundle $TMP/bundle.tar"
ssh "$REMOTE" "rm -r $TMP" || exit 1
