#!/bin/bash

# This script bootstraps a Graviton EC2 instance with Ubuntu 20.04 and builds Blueboat on it.

set -euxo pipefail

sudo apt update
sudo apt install -y \
  git libseccomp-dev build-essential libsqlite3-dev \
  pkg-config libssl-dev libclang-dev clang libz3-dev \
  python2 python ninja-build libfontconfig1-dev curl

git clone https://github.com/losfair/blueboat
curl https://sh.rustup.rs -sSf | sh -s -- -y
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
. ~/.cargo/env
. ~/.nvm/nvm.sh
nvm install 16
cargo install cargo-deb
curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm
cd blueboat
cd jsland
pnpm i
cd ..
SKIA_NINJA_COMMAND=/usr/bin/ninja BLUEBOAT_DEB=1 ./build.sh
