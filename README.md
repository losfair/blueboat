# Blueboat

![CI](https://github.com/losfair/blueboat/actions/workflows/ci.yml/badge.svg)

Blueboat is an open-source alternative to Cloudflare Workers and aims to be a developer-friendly, multi-tenant platform for serverless web applications.

- [Features](#features)
- [Deploy](#deploy-your-own-blueboat-instance)

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

## Features

Supported and planned features:

- Standard JavaScript features supported by V8
- A subset of the Web platform API
  - [x] `fetch()`, `Request` and `Response` objects
  - [x] `TextEncoder`, `TextDecoder`
  - [ ] Timers
    - [x] `setTimeout`, `clearTimeout`
    - [ ] `setInterval`, `clearInterval`
  - [x] `URL`, `URLSearchParams`
  - [x] `crypto.getRandomValues`
  - [x] `crypto.randomUUID`
  - [ ] `crypto.subtle`
  - [ ] `console`
    - [x] `console.log()`
    - [x] `console.warn()`
    - [x] `console.error()`
- Request router
  - [x] The `Router` object
- Cryptography extensions
  - [x] Ed25519 and X25519: `NativeCrypto.Ed25519`, `NativeCrypto.X25519`
  - [x] JWT signing and verification: `NativeCrypto.JWT`
  - [x] Hashing: `NativeCrypto.digest`
- Graphics API
  - [x] Canvas (`Graphics.Canvas`)
  - [x] Layout constraint solver based on Z3 (`Graphics.Layout`)
- Template API
  - [x] [Tera](https://github.com/Keats/tera) template rendering: `Template.render()`
- Encoding and decoding
  - [x] `Codec.hexencode()`, `Codec.hexdecode()`
  - [x] `Codec.b64encode()`, `Codec.b64decode()`
- Embedded datasets
  - [x] MIME type guessing: `Dataset.Mime.guessByExt()`
- Background tasks
  - [x] `Background.atMostOnce()`
  - [x] `Background.atLeastOnce()`
  - [x] `Background.delayed()`
- Data validation
  - [x] JSON Type Definition validation: `Validation.JTD`
- Text utilities
  - [x] YAML serialization and deserialization: `TextUtil.Yaml.parse()`, `TextUtil.Yaml.stringify()`
  - [x] Markdown rendering: `TextUtil.Markdown.renderToHtml()`
- Native API to external services
  - [x] MySQL client
  - [x] Apple Push Notification Service (APNS) client

The entire API definition is published as the [blueboat-types](https://www.npmjs.com/package/blueboat-types) package.

## Deploy your own Blueboat instance

Blueboat depends on various external services such as [FoundationDB](https://www.foundationdb.org/) and [Kafka](https://kafka.apache.org/) to operate reliably as a distributed system, but you don't need all these services to run Blueboat on a single machine. Just pull [b6t](https://github.com/losfair/b6t) - this contains all necessary dependencies to get Blueboat up and running.

After starting `b6t` you need to set up a reverse proxy such as Nginx or [Caddy](https://caddyserver.com/) to accept external traffic. Blueboat loads the application's metadata from the S3 key specified in the `X-Blueboat-Metadata` header, and this information should be provided by the reverse proxy.

An example Caddy config looks like:

```
hello.blueboat.example.com {
  reverse_proxy blueboat:3000 {
    # S3 path to application metadata
    header_up X-Blueboat-Metadata "test/metadata.json"
    header_up X-Blueboat-Client-Ip "{remote_host}"

    # Prevent faked request id
    header_up -X-Blueboat-Request-Id
  }
}
```

### Automatic deployments

`b6t`'s readme describes a simple way to manually deploy apps, but if you need an automatic multi-tenant deployment service, you can deploy the [bbcp](https://github.com/losfair/bbcp) app (which itself runs on Blueboat) and use the [bbcli](https://github.com/losfair/bbcli) tool.

## License

Currently AGPL-3.0, but see [#56](https://github.com/losfair/blueboat/issues/56).
