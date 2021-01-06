#!/bin/bash

. "`dirname $0`/docker_config.sh.inc"

TMP="/tmp/rw-build-`uuidgen || exit 1`"
mkdir "$TMP" || exit 1

cleanup()
{
    echo "Cleanup up temporary directory $TMP"
    rm -r "$TMP"
}

trap cleanup EXIT

cd "`dirname $0`"

cp ./target/release/rusty-workers-{proxy,runtime,fetchd} "$TMP/" || exit 1
cp -r ./docker-test "$TMP/" || exit 1

cp ./baseenv.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${TAG_PREFIX}baseenv" "$TMP" || exit 1

cp ./fetchd.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${TAG_PREFIX}fetchd:$VERSION" "$TMP" || exit 1

cp ./runtime.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${TAG_PREFIX}runtime:$VERSION" "$TMP" || exit 1

cp ./proxy.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${TAG_PREFIX}proxy:$VERSION" "$TMP" || exit 1

cp ./all.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${TAG_PREFIX}all:$VERSION" "$TMP" || exit 1

echo "Build done."
