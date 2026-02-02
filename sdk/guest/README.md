# Isolate Guest SDKs

Guest SDKs for writing WASM modules that run inside Isolate sandboxes.

## Overview

These SDKs provide idiomatic language bindings for writing code that executes
**inside** an Isolate sandbox. They are distinct from the [client SDKs](../README.md)
which communicate with the Isolate server from outside.

Guest SDKs offer:

- **Host capability bindings** — Typed access to filesystem, network, and environment
  capabilities granted by the sandbox configuration.
- **Structured I/O** — JSON-based stdin/stdout protocol for passing structured data
  into and out of sandboxed modules.
- **Project templates** — Ready-to-use project scaffolding for quick starts in each
  supported language.

## WIT Interface

The guest interface is defined in [`wit/isolate-guest.wit`](wit/isolate-guest.wit)
using the [WebAssembly Interface Types (WIT)](https://component-model.bytecodealliance.org/design/wit.html)
format. This defines the contract between guest modules and the Isolate host runtime.

### Interfaces

| Interface | Description |
|-----------|-------------|
| `isolate:guest/io` | Read input bytes, write output bytes or JSON |
| `isolate:guest/env` | Access environment variables |
| `isolate:guest/fs` | Read and write files in preopened directories |
| `isolate:guest/log` | Structured logging (info, warn, error) |

## Available SDKs

| Language | Directory | Status |
|----------|-----------|--------|
| Rust | [`rust/`](rust/) | Beta |
| Go | [`go/`](go/) | Beta |
| Python | [`python/`](python/) | Beta |

## I/O Protocol

Guest modules communicate with the host using a JSON-based protocol over stdin/stdout:

1. **Input**: The host writes a JSON payload to the guest's stdin.
2. **Processing**: The guest reads the input, performs its work using granted capabilities.
3. **Output**: The guest writes a JSON payload to stdout.
4. **Exit code**: 0 for success, non-zero for failure.

```
Host                    Guest Module
  │                         │
  │── JSON stdin ──────────►│
  │                         │── process ──►
  │                         │◄── done ─────
  │◄── JSON stdout ────────│
  │◄── exit code ──────────│
```

## Building Guest Modules

Guest modules must be compiled to WebAssembly (WASM). Each SDK README contains
language-specific instructions. The general workflow is:

1. Write your guest module using the SDK.
2. Compile to `wasm32-wasip1` (or `wasm32-wasi` depending on toolchain).
3. Run inside Isolate with the required capabilities.

```bash
# Example: run a compiled guest module
isolate run --capability stdout --capability stdin my_module.wasm
```
