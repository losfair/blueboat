#!/bin/bash

set -e
cd "$(dirname $0)"
rm -r src || true
cp -r ../jsland/dist/src ./
echo "Copied jsland types."
