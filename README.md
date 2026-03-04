# Isolate: Secure Sandbox Runtime

[![CI](https://github.com/josedab/isolate/workflows/CI/badge.svg)](https://github.com/josedab/isolate/actions)
[![Security](https://github.com/josedab/isolate/workflows/Security/badge.svg)](https://github.com/josedab/isolate/actions)
[![codecov](https://codecov.io/gh/josedab/isolate/branch/main/graph/badge.svg)](https://codecov.io/gh/josedab/isolate)
[![dependency status](https://deps.rs/repo/github/josedab/isolate/status.svg)](https://deps.rs/repo/github/josedab/isolate)
[![Crates.io](https://img.shields.io/crates/v/isolate-core.svg)](https://crates.io/crates/isolate-core)
[![Documentation](https://docs.rs/isolate-core/badge.svg)](https://docs.rs/isolate-core)
[![License](https://img.shields.io/crates/l/isolate-core.svg)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.75.0-blue.svg)](https://blog.rust-lang.org/2023/12/28/Rust-1.75.0.html)

A lightweight, secure sandbox runtime written in Rust for executing untrusted WASM code with strong isolation guarantees.

## Features

| Production Ready | Preview (API only) | Experimental |
|:-----------------|:-------------------|:-------------|
| Core sandbox, capabilities, resource limits, metrics, pool, networking, policy engine | Platform services, billing, deployment, observability, agent, federation | Snapshots, WASI Preview2, Kubernetes, chaos testing |

### Feature Flag Maturity

Enable optional modules with `cargo add isolate-core --features <flag>`:

| Feature Flag | Status | Description |
|:-------------|:------:|:------------|
| *(default)* | ✅ Stable | Core sandbox, capabilities, resource limits, metrics |
| `pool` | ✅ Stable | Warm sandbox pool with predictive autoscaling |
| `networking` | ✅ Stable | HTTP client and network policy enforcement |
| `policy-engine` | ✅ Stable | Core policy rules, audit logging, composition |
| `agent` | 👁️ Preview | AI agent framework (in-memory, no real LLM provider) |
| `platform` | 👁️ Preview | Admin, storage, workflow, hosting, infra (in-memory backends) |
| `observability` | 👁️ Preview | Dashboard, tracing, analytics (config generation, no real export) |
| `billing` | 👁️ Preview | Billing and cost tracking (in-memory, no payment provider) |
| `deployment` | 👁️ Preview | Auto-scaling, registries, hot reload (in-memory) |
| `federation` | 👁️ Preview | Federated registry (in-memory gossip, no real network) |
| `extras` | 👁️ Preview | AI sandbox, carbon tracking, JS runtime (in-memory simulations) |
| `snapshots` | 🧪 Experimental | Copy-on-write snapshot/restore |
| `wasi-preview2` | 🧪 Experimental | WASI Component Model support |
| `kubernetes` | 🧪 Experimental | K8s operator and CRD support |
| `otel-telemetry` | 🧪 Experimental | OpenTelemetry distributed tracing |
| `debug-support` | 🧪 Experimental | Live debugging and time-travel replay |
| `module-signing` | 🧪 Experimental | Cryptographic module signing (Ed25519) |
| `chaos-testing` | 🧪 Experimental | Fault injection for resilience testing |
| `gpu-compute` | ⚠️ Stub | GPU acceleration (simulated, not real hardware) |
| `distributed-mesh` | ⚠️ Stub | Multi-node clustering (network stubs only) |
| `hotpatch` | ⚠️ Stub | Hot code patching (simulated only) |
| `full` | — | Meta-feature: enables everything above |

> Features marked 👁️ **Preview** have designed APIs backed by in-memory simulations.
> Evaluate them for API feedback but do not rely on them for production.
>
> Features marked ⚠️ **Stub** are scaffolding only. Do not use.

- **Fast Cold Start**: <5ms sandbox creation (vs 125ms+ for microVMs)
- **Memory Safety**: Rust implementation eliminates runtime vulnerabilities
- **Multi-Language**: Execute any WASM-compiled language (Rust, C/C++, Go, AssemblyScript, etc.)
- **Capability-Based Security**: Fine-grained permission control with default-deny
- **Resource Limits**: CPU, memory, I/O quotas with enforcement
- **Snapshot/Restore**: Sub-millisecond warm starts

## Quick Start

> ⏱️ **First build:** ~2 minutes · **Hot rebuild:** ~5 seconds · **Run example:** instant

### Try It Now

```bash
git clone https://github.com/josedab/isolate.git && cd isolate
cargo run --package isolate-core --example basic_sandbox
```

> 📁 **More examples** in [`isolate-core/examples/`](isolate-core/examples/) — including
> capabilities, error handling, resource limits, and multi-sandbox patterns.

### Installation

```bash
cargo add isolate-core
```

### Basic Usage

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Load a WASM module
    let wasm_bytes = std::fs::read("module.wasm")?;

    // Configure the sandbox with capabilities and limits
    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .memory_limit(128 * 1024 * 1024)  // 128MB
        .cpu_time_limit(std::time::Duration::from_secs(30))
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()?;

    // Create and run the sandbox
    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit code: {}", output.exit_code);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
```

## Architecture

```mermaid
flowchart TB
    subgraph Client["Client Application"]
        API[Public API]
    end

    subgraph Isolate["Isolate Runtime"]
        direction TB
        Config[SandboxConfig Builder]
        Sandbox[Sandbox Manager]

        subgraph Security["Security Layer"]
            Cap[Capability Enforcer]
            Audit[Audit Logger]
        end

        subgraph Engine["Execution Engine"]
            Wasmtime[Wasmtime Runtime]
            WASI[WASI Layer]
        end

        subgraph Resources["Resource Control"]
            Fuel[Fuel Metering]
            Memory[Memory Limits]
            IO[I/O Quotas]
            Time[Timeout Control]
        end

        Metrics[Prometheus Metrics]
    end

    subgraph WASM["WASM Module"]
        Code[User Code]
    end

    API --> Config
    Config --> Sandbox
    Sandbox --> Cap
    Sandbox --> Engine
    Cap --> Audit
    Engine --> Resources
    Wasmtime --> WASI
    WASI --> Code
    Resources --> Metrics
```

### Execution Flow

```mermaid
sequenceDiagram
    participant App as Application
    participant SB as Sandbox
    participant Cap as Capability Enforcer
    participant Engine as WASM Engine
    participant WASM as WASM Module

    App->>SB: create(config)
    SB->>Engine: compile(wasm_bytes)
    Engine-->>SB: CompiledModule
    SB-->>App: Sandbox (Ready)

    App->>SB: run(input)
    SB->>Cap: check_capabilities()
    Cap-->>SB: Ok
    SB->>Engine: instantiate()
    Engine->>WASM: _start()
    WASM-->>Engine: exit_code
    Engine-->>SB: ExecutionResult
    SB-->>App: Output
```

## Capability System

Isolate uses a capability-based security model with default-deny:

```rust
use isolate_core::capability::*;

let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // Filesystem: read-only access to /data
    .capability(Capability::filesystem_read("/data"))
    // Network: HTTP client to specific hosts
    .capability(Capability::http_client(vec!["api.example.com"]))
    // Environment: specific variables only
    .capability(Capability::env_var("API_KEY"))
    .build()?;
```

## Resource Limits

Control CPU, memory, and I/O usage:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // Memory limits
    .memory_limit(128 * 1024 * 1024)      // 128MB heap
    .stack_size(1024 * 1024)               // 1MB stack
    // CPU limits
    .fuel(1_000_000)                       // Instruction fuel
    .cpu_time_limit(Duration::from_secs(30))
    // Timeouts
    .wall_time_limit(Duration::from_secs(60))
    .build()?;
```

## CLI Usage

```bash
# Run a WASM module
isolate run module.wasm
# => Hello from WASM!

# Run with capabilities
isolate run module.wasm \
    --cap-fs-read /data \
    --cap-http api.example.com \
    --memory-limit 128M \
    --timeout 30s
# => [sandbox] exit_code=0 duration=142ms fuel=483201

# Run with input
echo "input data" | isolate run module.wasm
# => Processed 10 bytes
```

## gRPC Server

Start the gRPC server for remote sandbox management:

```bash
isolate-server --addr 0.0.0.0:50051
```

## Docker

### Quick Start with Docker Compose

```bash
# Start the server (builds image automatically)
docker compose up -d

# View logs
docker compose logs -f

# Stop
docker compose down
```

The server will be available at `localhost:50051` (gRPC).

### Production Docker Image

Build and run the production image directly:

```bash
# Build
docker build -t isolate-server .

# Run
docker run -d -p 50051:50051 \
  -e RUST_LOG=info \
  --name isolate-server \
  isolate-server
```

### Development with Hot Reload

Use the dev profile for automatic rebuilds on code changes:

```bash
docker compose --profile dev up isolate-server-dev
```

This mounts the source directory and uses `cargo-watch` for hot reloading.

## Experimental Features

The following modules are included but considered **experimental** and not production-ready. Their APIs may change significantly in future releases:

| Module | Description | Status |
|--------|-------------|--------|
| `gpu` | WebGPU sandboxed compute | Simplified simulation |
| `mesh` | Distributed sandbox clustering | Network stubs only |
| `enclave` | TEE integration (SGX/SEV/TrustZone) | Simulated TEE |
| `hotpatch` | Hot code patching | Simulation only |
| `verify` | Formal verification | Simplified methods |
| `security` | Linux seccomp/Landlock | Skeleton implementation |

These modules are exported for early feedback and experimentation. Do not rely on them for production workloads.

## Documentation

Generate and view the API documentation:

```bash
cargo doc --open --no-deps
```

## Security Model

Isolate provides defense-in-depth security:

1. **WASM Sandbox**: Linear memory isolation, type-safe calls, validated bytecode
2. **Capability System**: Default deny, explicit grants, audit logging
3. **Resource Limits**: Fuel metering, memory bounds, time limits
4. **OS Isolation** (Linux): seccomp-bpf, Landlock LSM, namespaces

## Performance Targets

| Metric | Target |
|--------|--------|
| Cold Start (p50) | <3ms |
| Cold Start (p99) | <5ms |
| Warm Start (p50) | <500us |
| Warm Start (p99) | <1ms |
| Memory Overhead | <5MB |

## Development

### Prerequisites

- Rust 1.75.0 or later
- [protobuf compiler (`protoc`)](https://grpc.io/docs/protoc-installation/) (required for `isolate-server`)
- [just](https://github.com/casey/just) (optional, for task running)
- Python 3.9+ with development headers (optional, only for `isolate-python` bindings)

> **Note:** The `isolate-python` crate requires a Python development environment and is
> excluded from the default workspace build. Standard `cargo build` and `cargo test`
> work without Python installed. To include Python bindings, use `cargo test --workspace`.

### Quick Commands

```bash
# Run all checks (works without `just` installed)
cargo xtask check

# Equivalent using just (optional)
# just check

# Run tests (default members, no Python dependency required)
cargo test

# Run tests including Python bindings (requires python3-dev)
cargo test --workspace --all-features

# Run benchmarks
cargo bench --package isolate-core

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings

# Generate docs
cargo doc --no-deps --all-features --open
```

### Running Fuzz Tests

```bash
# Install cargo-fuzz (requires nightly)
cargo +nightly install cargo-fuzz

# Run a fuzz target
cd fuzz
cargo +nightly fuzz run fuzz_wasm_module
```

## Contributing

We welcome contributions! See the [Quickstart guide](docs/QUICKSTART.md) to go from clone to first passing test in 60 seconds, read [CONTRIBUTING.md](CONTRIBUTING.md) for the full guidelines, or check [DEVELOPMENT.md](DEVELOPMENT.md) for a quick command reference.

### Good First Issues

Look for issues labeled [`good first issue`](https://github.com/josedab/isolate/labels/good%20first%20issue) to get started.

## Comparison with Alternatives

| Feature | Isolate | Wasmtime (bare) | microVMs |
|---------|---------|-----------------|----------|
| Cold Start | <5ms | <5ms | 125ms+ |
| Memory Overhead | <5MB | ~2MB | 128MB+ |
| Capability System | Built-in | Manual | Varies |
| Resource Metering | Built-in | Manual | OS-level |
| Multi-tenant | Yes | Manual | Yes |
| Language | Rust | Rust | Various |

## License

MIT OR Apache-2.0

## Acknowledgments

Built on top of the excellent [Wasmtime](https://wasmtime.dev/) runtime by the Bytecode Alliance.
