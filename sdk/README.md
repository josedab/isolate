# Isolate Client SDKs

Client SDKs for interacting with the Isolate sandbox runtime over gRPC.

## Available SDKs

| Language | Directory | Status | Description |
|----------|-----------|--------|-------------|
| Go | [`go/`](go/) | Stable | gRPC client for remote sandbox management |
| TypeScript | [`typescript/`](typescript/) | Stable | gRPC client with Zod validation |
| Python | [`python/`](python/) | Beta | gRPC client (sync + async) |
| Java | [`java/`](java/) | Planned | Not yet implemented |

> **Note:** For **in-process** Python embedding (no server needed), use
> [`isolate-python`](../isolate-python/) which provides native PyO3 bindings.

See each SDK's README for installation and usage instructions.

## Guest SDKs (for writing WASM modules)

| Language | Directory | Status |
|----------|-----------|--------|
| Rust | [`guest/rust/`](guest/rust/) | Beta |
| Go | [`guest/go/`](guest/go/) | Beta |
| Python | [`guest/python/`](guest/python/) | Beta |

Guest SDKs provide idiomatic language bindings for writing WASM modules that run inside Isolate sandboxes. See the [guest SDK README](guest/README.md) for details.
