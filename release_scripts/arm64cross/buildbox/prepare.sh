#!/bin/bash

set -euxo pipefail

curl https://sh.rustup.rs -sSf | sh -s -- -y
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
. ~/.cargo/env
. ~/.nvm/nvm.sh
nvm install 16
cargo install cargo-deb
curl -f https://get.pnpm.io/v6.16.js | node - add --global pnpm
