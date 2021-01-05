#!/bin/bash

VERSION="0.1.0"
TMP="/tmp/rw-build-`uuidgen || exit 1`"
DOCKER_TAG_PREFIX="losfair/rusty-workers-"
mkdir "$TMP" || exit 1

cleanup()
{
    echo "Cleanup up temporary directory $TMP"
    rm -r "$TMP"
}

trap cleanup EXIT

cp ./target/release/rusty-workers-{proxy,runtime,fetchd} "$TMP/" || exit 1

cp ./baseenv.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${DOCKER_TAG_PREFIX}baseenv" "$TMP" || exit 1

cp ./fetchd.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${DOCKER_TAG_PREFIX}fetchd:$VERSION" "$TMP" || exit 1

cp ./runtime.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${DOCKER_TAG_PREFIX}runtime:$VERSION" "$TMP" || exit 1

cp ./proxy.Dockerfile "$TMP/Dockerfile" || exit 1
docker build -t "${DOCKER_TAG_PREFIX}proxy:$VERSION" "$TMP" || exit 1

echo "Build done."
