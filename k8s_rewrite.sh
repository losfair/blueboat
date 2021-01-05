#!/bin/bash

generate_runtime_deployments()
{
    for (( i=1; i<=$NUM_RUNTIMES; ++i)); do
        cp "./k8s.$SUFFIX/deployments/runtime-X.yaml" "./k8s.$SUFFIX/deployments/runtime-$i.yaml" || exit 1
        sed -i "s/runtime-X/runtime-$i/g" "./k8s.$SUFFIX/deployments/runtime-$i.yaml" || exit 1
    done
    rm "./k8s.$SUFFIX/deployments/runtime-X.yaml" || exit 1
}

generate_runtime_services()
{
    for (( i=1; i<=$NUM_RUNTIMES; ++i)); do
        cp "./k8s.$SUFFIX/services/runtime-X.yaml" "./k8s.$SUFFIX/services/runtime-$i.yaml" || exit 1
        sed -i "s/runtime-X/runtime-$i/g" "./k8s.$SUFFIX/services/runtime-$i.yaml" || exit 1
        sed -i "s/__RUNTIME_IP__/$NET_PREFIX.2.$i/g" "./k8s.$SUFFIX/services/runtime-$i.yaml" || exit 1
    done
    rm "./k8s.$SUFFIX/services/runtime-X.yaml" || exit 1
}

rewrite_proxy()
{
    local ADDRLIST=""
    for (( i=1; i<=$NUM_RUNTIMES; ++i)); do
        if [ "$i" != "1" ]; then
            ADDRLIST="$ADDRLIST,"
        fi
        ADDRLIST="$ADDRLIST$NET_PREFIX.2.$i:3000"
    done

    sed -i "s/__RUNTIME_ADDRESSES__/$ADDRLIST/g" "./k8s.$SUFFIX/deployments/proxy.yaml" || exit 1
}

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

if [ -z "$NUM_RUNTIMES" ]; then
    echo "[-] NUM_RUNTIMES not defined"
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

generate_runtime_deployments 
generate_runtime_services
rewrite_proxy

cd "./k8s.$SUFFIX" || exit 1
echo "#!/bin/sh" > apply.sh || exit 1
echo "cd \"\`dirname \$0\`\"" >> apply.sh || exit 1
find . -name "*.yaml" -exec 'echo' 'kubectl apply -f' '{}' ';' >> apply.sh || exit 1
chmod +x apply.sh || exit 1
