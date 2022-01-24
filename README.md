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

- Batteries-included standard library.

Blueboat exposes a wide range of "standard library" features including data validation, encoding, cryptography, graphics and background tasks, aside from the regular Web platform API - see [this tracking issue](https://github.com/losfair/blueboat/issues/65) for details.

- Designed for multi-app and multi-tenant environments.

Blueboat handles the routing of incoming HTTP requests, automatically scales apps up and down based on load, and securely isolates apps from different tenants using [seccomp](https://man7.org/linux/man-pages/man2/seccomp.2.html).

- Native support for stateful apps.

Apps have native access to a strongly-consistent distributed key-value database based on [FoundationDB](https://github.com/apple/foundationdb).

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

Apache-2.0
