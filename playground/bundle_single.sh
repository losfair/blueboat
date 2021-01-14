#!/bin/sh

TMP="/tmp/build-`uuidgen || exit 1`"
mkdir $TMP || exit 1

cp "$1" "$TMP/index.js" || exit 1
CURDIR=`pwd`
cd "$TMP" && tar c . > "$CURDIR/../bundles/$1.tar"
cd "$CURDIR" || exit 1
rm -r $TMP
