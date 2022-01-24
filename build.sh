#!/bin/bash

set -eo pipefail

if [ "$DEBUG_MODE" = "1" ]; then
  echo "[*] Building debug mode binary."
  release_arg=""
  target_subdir="debug"
else
  release_arg="--release"
  target_subdir="release"
fi

if [ "$SKIP_PREBUILD" = "1" ]; then
  echo "[*] Prebuild skipped."
else
  echo "[*] Running pre-build."
  mkdir -p jsland/dist
  echo "" > jsland/dist/bundle.js
  echo -n "" > jsland.snapshot
  cargo build $release_arg
fi

echo "[*] Generating schema."
MKIMAGE_PRINT_SCHEMA=1 ./target/$target_subdir/blueboat_mkimage > jsland/native_schema.json

echo "[*] Building jsland."
cd jsland
rm -r dist || true
mkdir dist
node ./scripts/build_schema.mjs
pnpm run build
cd ..
./jsland-types/build.sh

echo "[*] Generating jsland snapshot."
RUST_LOG=debug ./target/$target_subdir/blueboat_mkimage -i ./jsland/dist/bundle.js -o jsland.snapshot

if [ "$SKIP_FINAL_BUILD" = "1" ]; then
  echo "[*] Final build skipped."
  exit 0
fi

echo "[*] Running final build."
cargo build $release_arg

if [ "$BLUEBOAT_DEB" = "1" ]; then
if [ "$DEBUG_MODE" = "1" ]; then
  echo "[-] Cannot build DEB in debug mode."
else
  echo "[*] Building DEB package."
  cargo deb
fi
fi

echo "[+] Build completed."
