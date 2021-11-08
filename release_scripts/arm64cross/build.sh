#!/bin/bash

# https://stackoverflow.com/a/1115074
readlink () {
  python3 -c "import os,sys; print(os.path.realpath(sys.argv[1]))" "${1}"
}

set -euxo pipefail
cd "$(dirname $0)"

mkdir ./artifact || true
find ./artifact -mindepth 1 -delete

docker build --build-arg HTTP_PROXY \
  --build-arg HTTPS_PROXY \
  --build-arg http_proxy \
  --build-arg https_proxy \
  --platform linux/arm64 -t losfair/blueboat-arm64cross-buildbox ./buildbox

docker run --rm --platform linux/arm64 \
  -e http_proxy \
  -e https_proxy \
  -e HTTP_PROXY \
  -e HTTPS_PROXY \
  -v "$(readlink ../..)":/hostsrc:ro \
  -v "$(readlink ./artifact)":/artifact \
  losfair/blueboat-arm64cross-buildbox

cp ../../docker/run.sh ./releasebox/
cp ./artifact/target/debian/*.deb ./releasebox/blueboat.deb

docker build --platform linux/arm64 -t losfair/blueboat --squash \
  ./releasebox
