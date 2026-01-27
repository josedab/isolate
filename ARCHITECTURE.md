# Architecture

This document orients new contributors to the Isolate codebase. Read this first.

## What Is Isolate?

A secure sandbox runtime for executing untrusted WebAssembly (WASM) code. It wraps
[Wasmtime](https://wasmtime.dev/) with capability-based security, resource metering,
and a production-ready API surface.

## Repository Layout

```
isolate/
├── isolate-core/           # Core library — the main crate
├── isolate-server/         # gRPC server wrapping isolate-core
├── isolate-cli/            # CLI tool for running WASM locally
├── isolate-python/         # Python bindings via PyO3 (excluded from default build)
├── sdk/                    # Client SDKs (Go, Java, Python, TypeScript)
├── proto/                  # Protocol buffer definitions
├── fuzz/                   # Fuzz testing targets
├── docs/                   # mdBook documentation site
└── website/                # Project website
```

## Core vs Optional Modules

The `isolate-core` crate is the heart of the project. **Not all modules are created
equal.** The crate uses Cargo feature flags to separate production-ready core from
optional/experimental functionality.

### Always-On Core (no feature flag required)

These modules compile by default and form the stable API:

| Module | File(s) | Purpose |
|--------|---------|---------|
| `sandbox` | `sandbox.rs` | Main API — create and run isolated WASM |
| `config` | `config.rs` | Builder pattern for sandbox configuration |
| `capability` | `capability/` | Capability-based permission system |
| `engine` | `engine/` | Wasmtime integration, module caching |
| `resource` | `resource/` | Resource limits and metering |
| `error` | `error.rs` | Error types with actionable suggestions |
| `metrics` | `metrics.rs` | Prometheus metrics |
| `stability` | `stability.rs` | Stability tracking |

**Start here:** Read `sandbox.rs` → `config.rs` → `capability/types.rs` → `engine/wasm.rs`.

### Feature-Gated Module Groups

Enable with `cargo build --features <feature>`:

| Feature | What it adds |
|---------|-------------|
| `pool` | Warm sandbox pool, predictive autoscaling |
| `networking` | HTTP client, network policy enforcement |
| `agent` | AI agent framework for tool-using sandboxes |
| `policy-engine` | Policy rules, audit logging, module composition |
| `platform` | Admin, gateway, orchestrator, KV store, secrets, IPC, marketplace, plugins, workflows, VFS, provenance |
| `extras` | AI execution, carbon tracking, enclave/TEE, JS runtime, OS security, formal verification |

### Experimental Features (not production ready)

| Feature | Status |
|---------|--------|
| `snapshots` | Copy-on-write snapshot/restore |
| `wasi-preview2` | WASI Component Model |
| `debug-support` | Time-travel debugging |
| `module-signing` | Cryptographic signing |
| `kubernetes` | K8s operator |
| `otel-telemetry` | OpenTelemetry tracing |
| `gpu-compute` | GPU acceleration (simulated) |
| `distributed-mesh` | Multi-node clustering (stubs) |
| `hotpatch` | Hot code patching (simulated) |
| `chaos-testing` | Fault injection |

Use `--features full` or `--all-features` to compile everything.

## Execution Flow

```
User Code
    │
    ▼
SandboxConfig::builder()    ← configure limits, capabilities, env
    │
    ▼
Sandbox::create(config)     ← compile WASM, create Wasmtime instance, setup WASI
    │
    ▼
sandbox.run(&input)         ← execute with timeout monitoring, fuel metering
    │
    ▼
Output { exit_code, stdout, stderr, resource_usage }
```

## Key Design Decisions

Architectural Decision Records live in `docs/adr/`. Highlights:

- **ADR-0001**: Wasmtime as the WASM runtime (performance, WASI support, Rust-native)
- **ADR-0002**: Capability-based security with default-deny
- **ADR-0003**: Multi-dimensional resource limiting (fuel + memory + I/O + wall time)
- **ADR-0010**: Builder pattern for configuration

## Dependencies

Core dependencies (always compiled):
- `wasmtime` / `wasmtime-wasi` — WASM runtime
- `tokio` — async runtime
- `serde` / `serde_json` — serialization
- `thiserror` — error types
- `prometheus` — metrics

Optional dependencies (only with feature flags):
- `opentelemetry*` — tracing (with `otel-telemetry`)
- `serde_yaml` — YAML config (with `platform` or `kubernetes`)
- `url` — URL parsing (with `networking`)
- `reqwest` — HTTP client (with `networking`)

## Getting Oriented

1. **Read the core:** `sandbox.rs` is ~400 lines and self-contained
2. **Run the tests:** `cargo test` runs core tests; `cargo test --all-features` runs everything
3. **Try the CLI:** `cargo run -p isolate-cli -- run isolate-core/tests/fixtures/hello.wasm --cap-stdout`
4. **Browse the ADRs:** `docs/adr/` explains the "why" behind architectural choices
