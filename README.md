# Blueboat

![CI](https://github.com/losfair/blueboat/actions/workflows/ci.yml/badge.svg)

Blueboat is an all-in-one, multi-tenant serverless JavaScript runtime.

A simple Blueboat application looks like:

```ts
Router.get("/", req => new Response("hello world"));

Router.get("/example", req => {
  return fetch("https://example.com");
});

Router.get("/yaml", req => {
  const res = TextUtil.Yaml.stringify({
    hello: "world",
  });
  return new Response(res);
});
```

- [Blueboat](#blueboat)
  - [Quickstart (single-tenant mode)](#quickstart-single-tenant-mode)
  - [The JavaScript API](#the-javascript-api)
    - [Web API compatibility](#web-api-compatibility)
    - [No local resources](#no-local-resources)
  - [Developing on Blueboat](#developing-on-blueboat)
    - [TypeScript type definitions](#typescript-type-definitions)
    - [API documentation](#api-documentation)
  - [Deploying Blueboat](#deploying-blueboat)
    - [Single-tenant](#single-tenant)
    - [Multi-tenant](#multi-tenant)
  - [Frameworks](#frameworks)
    - [Flareact](#flareact)
  - [Contributing](#contributing)
    - [Build locally](#build-locally)
    - [Build process internals](#build-process-internals)
  - [License](#license)

## Quickstart (single-tenant mode)

This pulls, builds, and runs the [hello-world](https://github.com/losfair/blueboat-examples/tree/main/hello-world) example.

Prerequisites: [cargo](https://github.com/rust-lang/cargo), npm, git, docker

```bash
cargo install boatctl
git clone https://github.com/losfair/blueboat-examples
cd blueboat-examples/hello-world
npm i && boat pack -o build.json
docker run --rm -d -p 127.0.0.1:3001:3001 \
  -v "$PWD:/app" \
  --entrypoint /usr/bin/blueboat_server \
  -e RUST_LOG=info \
  -e SMRAPP_BLUEBOAT_DISABLE_SECCOMP=1 \
  ghcr.io/losfair/blueboat:v0.3.1-alpha.5 \
  -l "0.0.0.0:3001" \
  --single-tenant "/app/build.json"
curl http://localhost:3001 # "hello world"
```

## The JavaScript API

### Web API compatibility

Blueboat prefers to keep compatibility with the Web API when doing so is reasonable.

* Things like `fetch`, `Request`, `Response` and `URL` are built-in.

### No local resources

Blueboat is a “distributed-system-native” runtime and prioritizes frictionless scalability over single-node performance. Local resources are abstracted out and replaced with their equivalents in a distributed system:

* Files → Key-value store
* Sockets → Event stream
* Single-file databases → Key-value store, `App.mysql` and `App.postgresql` object families
* FFI → WebAssembly

**Read and write files**

```ts
// Don't: The filesystem is a local resource
import { writeFileSync } from "fs"
writeFileSync("hello.txt", "Hello from Node")

// Do: Use the key-value store
const ns = new KV.Namespace("files")
await ns.set("hello.txt", "Hello from Blueboat")
```

**Stream data to client**

```ts
// Don't: Sockets are a local resource
import { WebSocketServer } from "ws";
const wss = new WebSocketServer({ port: 8080 });
wss.on("connection", (ws) => {
  ws.on("message", (data) => {
    console.log("received: %s", data);
  });
  ws.send("something");
});

// Do: Use the PubSub API
Router.post("/send_to_group", async req => {
  const { groupId, message } = await req.json();
  await App.pubsub.myChannel.publish(groupId, message)
  return new Response("ok");
});
```

**Use SQL databases**

```ts
// Don't: SQLite databases are stored on the local filesystem
import { DB } from "https://deno.land/x/sqlite/mod.ts";
const db = new DB("test.db");
const people = db.query("SELECT name FROM people");

// Do: Use `App.mysql` or `App.postgresql`
const people = App.mysql.myDatabase.exec("SELECT name FROM people", {}, "s");
```

## Developing on Blueboat

You can use your favorite JS/TS bundler to build your project for Blueboat. [webpack](https://github.com/webpack/webpack) works, and other bundlers like [esbuild](https://github.com/evanw/esbuild) and [bun](https://github.com/oven-sh/bun) should work too.

You can package and run your apps with single-tenant mode as described in the [Quickstart](#quickstart-single-tenant-mode) section, or deploy it on a multi-tenant service like [MagicBoat](https://magic.blueboat.io).

### TypeScript type definitions

Pull in the [blueboat-types](https://www.npmjs.com/package/blueboat-types) package in your TypeScript project, and add it to your `tsconfig.json`:

```json
{
  "compilerOptions": {
    "types": [
      "blueboat-types"
    ]
  }
}
```

### API documentation

While there isn't a lot of documentation on Blueboat's API yet, the global object declared in [jsland](https://github.com/losfair/blueboat/tree/main/jsland) can be seen as the source-of-truth of the public API.

Meanwhile, refer to [the tracking issue](https://github.com/losfair/blueboat/issues/65) for a high-level overview of the API.

## Deploying Blueboat

There are two options for deploying Blueboat apps.

### Single-tenant

This mode works well for local development and private self-hosting. The steps are described in [quickstart](#quickstart-single-tenant-mode).

### Multi-tenant

This is a fully optimized mode for multi-tenant operation. [See the guide](https://bluelogic.notion.site/Multi-tenant-Blueboat-deployment-f25c522955c04e59b5771954f8702c14) to deploy a multi-tenant environment yourself, or request access to our hosted environment, [MagicBoat](https://magic.blueboat.io).

## Frameworks

Simple web backends with JSON API and template-based rendering can be built without requiring any third-party frameworks. If you have more complex needs like server-side rendering React or just prefer a different style of router API, third-party frameworks are also available.

### Flareact

[Flareact](https://flareact.com/) is an edge-rendered React framework built for Cloudflare Workers. Since Blueboat and Workers both implement a large (and mostly overlapping) subset of the Web API, Flareact also works on Blueboat with some modifications in the entry script.

See the source code of [my blog](https://github.com/losfair/blog) as an example of using Flareact with Blueboat.

## Contributing

### Build locally

Clone the repository, and run `./build.sh`.

### Build process internals

Normally the build process is handled automatically by `build.sh`. Here are some internals, in case you need it.

Blueboat is built in three stages: *jsland build*, *rust prebuild* and *rust final build*. The *jsland build* stage bundles `/jsland/`; the *rust prebuild* stage generates a `blueboat_mkimage` binary that is used to generate `JSLAND_SNAPSHOT` from `/jsland/`; and the *rust final build* stage generates the final `blueboat_server` binary.

Please refer to [the CI script](https://github.com/losfair/blueboat/blob/main/.github/workflows/ci.yml) for a reproducible set of steps.

## License

Apache-2.0
