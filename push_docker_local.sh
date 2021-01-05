#!/bin/sh

. "`dirname $0`/docker_config.sh.inc"
REGISTRY="localhost:5000"

tag_and_push()
{
    docker tag "${TAG_PREFIX}$1:$VERSION" "$REGISTRY/${TAG_PREFIX}$1" || exit 1
    docker push "$REGISTRY/${TAG_PREFIX}$1" || exit 1
}

tag_and_push fetchd
tag_and_push runtime
tag_and_push proxy
