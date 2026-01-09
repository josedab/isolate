//! # Isolate: Secure Sandbox Runtime
//!
//! Isolate is a lightweight, secure sandbox runtime designed to execute untrusted
//! code with strong isolation guarantees. It combines WebAssembly (WASM) isolation
//! with capability-based security and resource controls.
//!
//! ## Features
//!
//! - **Fast Cold Start**: <5ms sandbox creation (vs 125ms+ for microVMs)
//! - **Memory Safety**: Rust implementation eliminates runtime vulnerabilities
//! - **Multi-Language**: Execute any WASM-compiled language (Rust, C/C++, Go, etc.)
//! - **Capability-Based Security**: Fine-grained permission control with default-deny
//! - **Resource Limits**: CPU fuel, memory, I/O quotas with enforcement
//! - **Snapshot/Restore**: Sub-millisecond warm starts (feature-gated)
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
//!
//! # async fn example() -> isolate_core::Result<()> {
//! // Load a WASM module
//! let wasm_bytes = std::fs::read("module.wasm")?;
//!
//! // Configure the sandbox
//! let config = SandboxConfig::builder()
//!     .module(&wasm_bytes)?
//!     .memory_limit(128 * 1024 * 1024)  // 128MB
//!     .cpu_time_limit(std::time::Duration::from_secs(30))
//!     .capability(Capability::stdout())
//!     .build()?;
//!
//! // Create and run the sandbox
//! let mut sandbox = Sandbox::create(config).await?;
//! let output = sandbox.run(&[]).await?;
//!
//! println!("Exit code: {}", output.exit_code);
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! The library is organized into several key modules:
//!
//! ### Core Modules
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`sandbox`] | Main API for creating and running sandboxes |
//! | [`config`] | Configuration builder for sandbox settings |
//! | [`capability`] | Capability-based security system |
//! | [`error`] | Error types and result aliases |
//! | [`engine`] | WASM execution engine (Wasmtime integration) |
//! | [`resource`] | Resource limiting and metering |
//! | [`metrics`] | Prometheus metrics integration |
//! | [`audit`] | Cryptographic audit logging |
//!
//! ### Additional Modules
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`pool`] | Multi-tenant resource pooling |
//! | [`workflow`] | Multi-sandbox pipeline orchestration |
//! | [`http`] | HTTP client capability implementation |
//! | [`ipc`] | Inter-process communication |
//! | [`secrets`] | Secret management |
//! | [`security`] | OS-level security (seccomp, Landlock) |
//!
//! ### Feature-Gated Modules
//!
//! These modules require feature flags to enable:
//!
//! | Feature | Module | Description |
//! |---------|--------|-------------|
//! | `snapshots` | [`snapshot`] | Copy-on-write snapshots for warm starts |
//! | `wasi-preview2` | [`wasi2`] | WASI Preview 2 (Component Model) support |
//! | `debug-support` | [`debug`] | Live debugging and time-travel replay |
//! | `module-signing` | [`signing`] | Cryptographic module signing |
//! | `kubernetes` | [`k8s`] | Kubernetes operator and Helm charts |
//! | `otel-telemetry` | [`telemetry`] | OpenTelemetry tracing integration |
//! | `distributed-mesh` | [`mesh`] | Distributed sandbox clustering |
//! | `gpu-compute` | [`gpu`] | GPU acceleration (experimental) |
//!
//! ## Execution Flow
//!
//! ```text
//! ┌─────────────────┐
//! │ SandboxConfig   │ ◄── Configure limits, capabilities, environment
//! │ ::builder()     │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ Sandbox::create │ ◄── Compile WASM, create instance, setup WASI
//! │     (async)     │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ sandbox.run()   │ ◄── Execute with resource monitoring
//! │     (async)     │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │     Output      │ ◄── exit_code, stdout, stderr, resource_usage
//! └─────────────────┘
//! ```
//!
//! ## Security Model
//!
//! Isolate provides defense-in-depth security through multiple layers:
//!
//! 1. **WASM Isolation**: Linear memory, validated bytecode, type-safe calls
//! 2. **Capability System**: Default-deny permissions, explicit grants
//! 3. **Resource Limits**: Fuel metering, memory bounds, I/O quotas
//! 4. **OS Isolation** (Linux): seccomp-bpf, Landlock LSM, namespaces
//!
//! See the [`capability`] module for details on available permissions.
//!
//! ## Feature Flags
//!
//! Enable optional functionality via Cargo features:
//!
//! ```toml
//! [dependencies]
//! isolate-core = { version = "0.1", features = ["snapshots", "otel-telemetry"] }
//! ```
//!
//! Available features:
//! - `snapshots` - Copy-on-write snapshot/restore
//! - `wasi-preview2` - WASI Component Model support
//! - `debug-support` - Debugging and time-travel
//! - `module-signing` - Cryptographic signing
//! - `kubernetes` - K8s operator support
//! - `otel-telemetry` - OpenTelemetry integration
//! - `ai-detection` - ML-based anomaly detection
//! - `distributed-mesh` - Multi-node clustering
//! - `gpu-compute` - GPU acceleration
//! - `chaos-testing` - Fault injection testing
//! - `full` - Enable all features

// Core modules (always available)
pub mod audit;
pub mod capability;
pub mod carbon;
pub mod compose;
pub mod config;
pub mod enclave;
pub mod engine;
pub mod error;
pub mod http;
pub mod ipc;
pub mod metrics;
pub mod plugin;
pub mod pool;
pub mod predict;
pub mod provenance;
pub mod resource;
pub mod sandbox;
pub mod secrets;
pub mod security;
pub mod verify;
pub mod workflow;

// Feature-gated experimental modules
#[cfg(feature = "snapshots")]
pub mod snapshot;

#[cfg(feature = "wasi-preview2")]
pub mod wasi2;

#[cfg(feature = "debug-support")]
pub mod debug;

#[cfg(feature = "module-signing")]
pub mod signing;

#[cfg(feature = "kubernetes")]
pub mod k8s;

#[cfg(feature = "otel-telemetry")]
pub mod telemetry;

#[cfg(feature = "ai-detection")]
pub mod ai;

#[cfg(feature = "nlp-policies")]
pub mod nlp;

#[cfg(feature = "hotpatch")]
pub mod hotpatch;

#[cfg(feature = "distributed-mesh")]
pub mod mesh;

#[cfg(feature = "gpu-compute")]
pub mod gpu;

#[cfg(feature = "chaos-testing")]
pub mod chaos;

// Re-export main types at crate root
pub use config::{SandboxConfig, SandboxConfigBuilder};
pub use error::{Error, Result};
pub use sandbox::{Output, Sandbox, SandboxId, SandboxState};
