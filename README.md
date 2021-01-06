# rusty-workers

[![Build and Test](https://github.com/losfair/rusty-workers/workflows/Build%20and%20Test/badge.svg)](https://github.com/losfair/rusty-workers/actions)

A cloud-native distributed serverless workers platform.

This is a **work in progress**.

## Features

- [x] JavaScript and WebAssembly engine powered by V8
- [x] Fetch API
- [x] Dynamic execution cluster scheduler
- [x] Kubernetes integration
- [ ] Key-value store API
- [ ] Web Crypto API

## Deployment

### Docker

Single-container Docker deployment is available for development or small self-hosted instances.

Documentation TODO.

### Kubernetes

rusty-workers is designed to be scalable and cloud-native. To deploy onto Kubernetes, generate your configuration files with `k8s_rewrite.sh`.

Documentation TODO.
