#!/bin/sh

cd scripts
find . -name "*.js" -exec ../bundle_single.sh '{}' ';'
