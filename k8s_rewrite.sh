#!/bin/sh

CONFIG="$1"
SUFFIX="$2"

if [ ! -f "$CONFIG" ]; then
    echo "[-] config file does not exist"
    exit 1
fi

if [ -z "$SUFFIX" ]; then
    echo "[-] suffix required"
    exit 1
fi

. "$CONFIG"

if [ -z "$NET_PREFIX" ]; then
    echo "[-] NET_PREFIX not defined"
    exit 1
fi

if [ -z "$PROXY_CONFIG_URL" ]; then
    echo "[-] PROXY_CONFIG_URL not defined"
    exit 1
fi

if [ -z "$EXTERNAL_IP" ]; then
    echo "[-] EXTERNAL_IP not defined"
    exit 1
fi

if [ -z "$IMAGE_PREFIX" ]; then
    echo "[-] IMAGE_PREFIX not defined"
    exit 1
fi

# Allow empty suffix
#if [ -z "$IMAGE_SUFFIX" ]; then
#    echo "[-] IMAGE_SUFFIX not defined"
#    exit 1
#fi

rm -r "./k8s.$SUFFIX"
cp -r "`dirname $0`/k8s" "./k8s.$SUFFIX" || exit 1

find "./k8s.$SUFFIX" -name "*.yaml" -exec sed -i "s#__NET_PREFIX__#$NET_PREFIX#g" '{}' ';'
find "./k8s.$SUFFIX" -name "*.yaml" -exec sed -i "s#__PROXY_CONFIG_URL__#$PROXY_CONFIG_URL#g" '{}' ';'
find "./k8s.$SUFFIX" -name "*.yaml" -exec sed -i "s#__EXTERNAL_IP__#$EXTERNAL_IP#g" '{}' ';'
find "./k8s.$SUFFIX" -name "*.yaml" -exec sed -i "s#__IMAGE_PREFIX__#$IMAGE_PREFIX#g" '{}' ';'
find "./k8s.$SUFFIX" -name "*.yaml" -exec sed -i "s#__IMAGE_SUFFIX__#$IMAGE_SUFFIX#g" '{}' ';'

