# rusty-workers

[![Build and Test](https://github.com/losfair/rusty-workers/workflows/Build%20and%20Test/badge.svg)](https://github.com/losfair/rusty-workers/actions)

A cloud-native distributed serverless workers platform.

## Features

- [x] JavaScript and WebAssembly engine powered by V8
- [x] Fetch API
- [x] Highly scalable execution engine
- [x] Kubernetes integration
- [x] Strongly-consistent key-value store
- [ ] Transactional key-value store API
- [ ] Web Crypto API

## Getting started

### Prerequisites

- [Rust](https://www.rust-lang.org/) nightly >= 1.50
- [Node.js](https://nodejs.org/) and [npm](https://www.npmjs.com/)
- [TiKV](https://github.com/tikv/tikv) with [Placement Driver](https://github.com/tikv/pd) ([how to build](https://pingcap.com/blog/building-running-and-benchmarking-tikv-and-tidb/))

### Build

```bash
make librt-deps
make
```

Find the built binaries in `target/release` and copy them to one of your PATH directories:

- `rusty-workers-proxy`
- `rusty-workers-runtime`
- `rusty-workers-fetchd`
- `rusty-workers-cli`

### Start services

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

# Start TiKV nodes.
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

### Deploy your first application

rusty-workers does not come with its own management UI yet but you can interact with the cluster using `rusty-workers-cli`:

```bash
# Set TiKV PD address
export TIKV_PD="127.0.0.1:2379"

# Deploy an app
rusty-workers-cli app add-app ./counter.toml --bundle ./counter.js.tar

# Add a route to the app
rusty-workers-cli app add-route localhost --path /counter --appid 19640b0c-1dff-4b20-9599-0b4c4a11da3f

# List all routes
rusty-workers-cli app all-routes
```

## Deployment

### Kubernetes

rusty-workers is designed to be scalable and cloud-native. To deploy onto Kubernetes, generate your configuration files with `k8s_rewrite.sh`.

Documentation TODO.
