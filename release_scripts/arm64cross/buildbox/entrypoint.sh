#!/bin/bash

set -euxo pipefail
. ~/.cargo/env
. ~/.nvm/nvm.sh
cd /root
git clone /hostsrc ./blueboat
cd blueboat
cd jsland
pnpm i
cd ..
SKIA_NINJA_COMMAND=/usr/bin/ninja BLUEBOAT_DEB=1 ./build.sh
cp -r ./target /artifact/
