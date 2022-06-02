#!/bin/bash

set -e
cd "$(dirname $0)"
rm -r ./target/debian || true
find ./docker -name "*.deb" -type f -exec rm '{}' ';'
BLUEBOAT_DEB=1 ./build.sh
deb_file="$(find ./target/debian/ -type f)"
cp ${deb_file} ./docker/blueboat.deb
cd docker
docker build -t losfair/blueboat .
