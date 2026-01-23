# isolate-core

[![Crates.io](https://img.shields.io/crates/v/isolate-core.svg)](https://crates.io/crates/isolate-core)
[![Documentation](https://docs.rs/isolate-core/badge.svg)](https://docs.rs/isolate-core)
[![License](https://img.shields.io/crates/l/isolate-core.svg)](../LICENSE-MIT)

The core library for the Isolate secure sandbox runtime. This crate provides the fundamental APIs for creating and running isolated WebAssembly sandboxes with capability-based security and resource controls.

## Features

- **Fast Cold Start**: <5ms sandbox creation
- **Capability-Based Security**: Default-deny permissions model
- **Resource Limits**: Memory, CPU fuel, I/O quotas, timeouts
- **Output Capture**: Stdout/stderr capture with streaming support
- **Module Caching**: Compiled modules are cached for reuse
- **Prometheus Metrics**: Built-in observability

## Installation

```toml
[dependencies]
isolate-core = "0.1"
```

## Quick Start

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
use std::time::Duration;

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Load WASM module
    let wasm_bytes = std::fs::read("module.wasm")?;

    // Configure sandbox
    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .memory_limit(128 * 1024 * 1024)  // 128MB
        .fuel(10_000_000)                  // CPU limit
        .wall_time_limit(Duration::from_secs(30))
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()?;

    // Create and run
    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit: {}", output.exit_code);
    println!("Output: {}", output.stdout_str());

    Ok(())
}
```

## Capabilities

Sandboxes have no permissions by default. Grant capabilities explicitly:

```rust
use isolate_core::capability::Capability;

let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // I/O
    .capability(Capability::stdout())
    .capability(Capability::stderr())
    .capability(Capability::stdin())
    // Filesystem
    .capability(Capability::filesystem_read("/data"))
    .capability(Capability::filesystem_write("/output"))
    // Network
    .capability(Capability::http_client(vec!["api.example.com"]))
    .capability(Capability::dns_resolve())
    // Time & Random
    .capability(Capability::system_clock())
    .capability(Capability::secure_random())
    // Environment
    .capability(Capability::env_var("API_KEY"))
    .build()?;
```

## Resource Limits

Control resource consumption:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // Memory
    .memory_limit(256 * 1024 * 1024)  // 256MB heap
    // CPU
    .fuel(100_000_000)                 // Instruction fuel
    .cpu_time_limit(Duration::from_secs(60))
    // Time
    .wall_time_limit(Duration::from_secs(120))
    // I/O
    .io_read_limit(10 * 1024 * 1024)   // 10MB input
    .io_write_limit(10 * 1024 * 1024)  // 10MB output
    .build()?;
```

## Feature Flags

Optional functionality via Cargo features:

| Feature | Description |
|---------|-------------|
| `snapshots` | Copy-on-write snapshot/restore for sub-ms warm starts |
| `wasi-preview2` | WASI Component Model support |
| `debug-support` | Live debugging and time-travel replay |
| `module-signing` | Cryptographic module signing/verification |
| `kubernetes` | Kubernetes operator integration |
| `otel-telemetry` | OpenTelemetry tracing |
| `distributed-mesh` | Multi-node clustering |
| `gpu-compute` | GPU acceleration (experimental) |
| `chaos-testing` | Fault injection for testing |
| `full` | Enable all features |

Enable features in Cargo.toml:

```toml
[dependencies]
isolate-core = { version = "0.1", features = ["snapshots", "otel-telemetry"] }
```

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                    Public API                         │
│  Sandbox, SandboxConfig, Capability, Output, Error   │
└──────────────────────┬───────────────────────────────┘
                       │
┌──────────────────────┼───────────────────────────────┐
│                      │                                │
│  ┌───────────────────┴───────────────────────────┐   │
│  │            Capability Enforcer                 │   │
│  │     (Permission checks, Audit logging)         │   │
│  └───────────────────┬───────────────────────────┘   │
│                      │                                │
│  ┌───────────────────┴───────────────────────────┐   │
│  │              WASM Engine                       │   │
│  │   (Wasmtime, Module caching, WASI context)    │   │
│  └───────────────────┬───────────────────────────┘   │
│                      │                                │
│  ┌───────────────────┴───────────────────────────┐   │
│  │            Resource Metering                   │   │
│  │   (Fuel, Memory, I/O, Time enforcement)       │   │
│  └───────────────────────────────────────────────┘   │
│                                                       │
│                   isolate-core                        │
└──────────────────────────────────────────────────────┘
```

## Module Structure

| Module | Description |
|--------|-------------|
| `sandbox` | Sandbox lifecycle management |
| `config` | Configuration builder |
| `capability` | Capability types and enforcement |
| `engine` | Wasmtime integration |
| `resource` | Resource limits and metering |
| `error` | Error types |
| `metrics` | Prometheus metrics |
| `audit` | Tamper-evident audit logging |

## Testing

```bash
# Run all tests
cargo test --package isolate-core

# Run with all features
cargo test --package isolate-core --all-features

# Run specific test
cargo test --package isolate-core test_sandbox_execution
```

## Benchmarks

```bash
# Run benchmarks
cargo bench --package isolate-core

# Run specific benchmark
cargo bench --package isolate-core -- cold_start
```

## Documentation

```bash
# Generate and open documentation
cargo doc --package isolate-core --no-deps --open
```

## License

MIT OR Apache-2.0
