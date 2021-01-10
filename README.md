# rusty-workers

[![Build and Test](https://github.com/losfair/rusty-workers/workflows/Build%20and%20Test/badge.svg)](https://github.com/losfair/rusty-workers/actions)

A cloud-native distributed serverless workers platform.

This is a **work in progress**.

## Features

- [x] JavaScript and WebAssembly engine powered by V8
- [x] Fetch API
- [x] Dynamic execution cluster scheduler
- [x] Kubernetes integration
- [x] Key-value store API
- [ ] Web Crypto API

## Getting started

See `run_all.sh` as an example of getting everything up and running. Here are some commands
to start each service manually:

```bash

# Start TiKV Placement Driver.
pd-server --name=pd1 \
    --data-dir=playground/pd1 \
    --client-urls="http://127.0.0.1:2379" \
    --peer-urls="http://127.0.0.1:2380" \
    --initial-cluster="pd1=http://127.0.0.1:2380" \
    --log-file=playground/pd1.log

# Start TiKV instances.
tikv-server --pd-endpoints="127.0.0.1:2379" \
    --addr="127.0.0.1:20160" \
    --status-addr="127.0.0.1:20181" \
    --data-dir=playground/tikv1 \
    --log-file=playground/tikv1.log
tikv-server --pd-endpoints="127.0.0.1:2379" \
    --addr="127.0.0.1:20161" \
    --status-addr="127.0.0.1:20182" \
    --data-dir=playground/tikv2 \
    --log-file=playground/tikv2.log
tikv-server --pd-endpoints="127.0.0.1:2379" \
    --addr="127.0.0.1:20162" \
    --status-addr="127.0.0.1:20183" \
    --data-dir=playground/tikv3 \
    --log-file=playground/tikv3.log

# Start `fetchd`, the fetch service.
#
# `fetchd` is responsible for performing `fetch()` requests on behalf of apps. Since the
# apps are not trusted, it's recommended to put `fetchd` behind a firewall that disallows
# access to internal resources.
rusty-workers-fetchd --rpc-listen 127.0.0.1:3000

# Start `runtime`, the execution engine.
#
# The runtime service handles execution of apps and consumes a lot of CPU and memory resources.
# Each runtime process can execute multiple apps concurrently inside different V8 sandboxes.
rusty-workers-runtime --rpc-listen 127.0.0.1:3001 \
    --tikv-cluster 127.0.0.1:2379

# Start `proxy`, the request scheduler with an HTTP frontend.
#
# Both runtime and fetch service addresses can be load-balanced (routed to different backing
# instances for each connection).
rusty-workers-proxy \
    --fetch-service 127.0.0.1:3000 \
    --http-listen 0.0.0.0:3080 \
    --tikv-cluster 127.0.0.1:2379 \
    --runtimes 127.0.0.1:3001
```

## Deployment

### Kubernetes

rusty-workers is designed to be scalable and cloud-native. To deploy onto Kubernetes, generate your configuration files with `k8s_rewrite.sh`.

Documentation TODO.
