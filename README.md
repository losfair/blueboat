# rusty-workers

[![Build and Test](https://github.com/losfair/rusty-workers/workflows/Build%20and%20Test/badge.svg)](https://github.com/losfair/rusty-workers/actions)

A cloud-native distributed serverless workers platform.

## Features

- [x] JavaScript and WebAssembly engine powered by V8
- [x] Fetch API
- [x] Highly scalable execution engine
- [x] Kubernetes integration
- [x] Strongly-consistent key-value store
- [x] Transactional key-value store API
- [ ] Web Crypto API
- [ ] WebSocket server

## Getting started

### Prerequisites

- [Rust](https://www.rust-lang.org/) nightly >= 1.50
- [Node.js](https://nodejs.org/) and [npm](https://www.npmjs.com/)
- A MySQL-compatible database server. [TiDB](https://github.com/pingcap/tidb) is recommended for clusters but you can also use anything else, e.g. AWS Aurora or just MySQL itself.

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
    --db-url mysql://root@localhost:4000/rusty_workers

# Start `proxy`, the request scheduler with an HTTP frontend.
#
# Both runtime and fetch service addresses can be load-balanced (routed to different backing
# instances for each connection).
rusty-workers-proxy \
    --fetch-service 127.0.0.1:3000 \
    --http-listen 0.0.0.0:3080 \
    --db-url mysql://root@localhost:4000/rusty_workers \
    --runtimes 127.0.0.1:3001
```

### Deploy your first application

rusty-workers does not come with its own management UI yet but you can interact with the cluster using `rusty-workers-cli`:

```bash
export DB_URL="mysql://root@localhost:4000/rusty_workers"

# Create app configuration
cat > counter.toml << EOF
id = "19640b0c-1dff-4b20-9599-0b4c4a11da3f"
kv_namespaces = [
    { name = "test", id = "S7qrF3VatqaEsFCROU6wNA==" },
]
EOF

# Create app
cat > counter.js << EOF
addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(request) {
    let counter = await kv.test.get("counter");
    counter = (counter === null ? 0 : parseInt(counter)) + 1;

    await kv.test.put("counter", "" + counter);
    return new Response("New counter: " + counter);
}
EOF

# Deploy the app
rusty-workers-cli app add-single-file-app ./counter.toml --js ./counter.js

# Add a route to the app
rusty-workers-cli app add-route localhost --path /counter --appid 19640b0c-1dff-4b20-9599-0b4c4a11da3f

# Open a browser and navigate to http://localhost:3080/counter !
```

## Deployment

### Kubernetes

rusty-workers is designed to be scalable and cloud-native. To deploy onto Kubernetes, generate your configuration files with `k8s_rewrite.sh`.

Documentation TODO.
