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
//! ### Core Modules (always available)
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
//! | [`stability`] | Stability tracking utilities |
//!
//! ### Feature-Gated Module Groups
//!
//! Enable these via Cargo features to compile only what you need:
//!
//! | Feature | Modules | Description |
//! |---------|---------|-------------|
//! | `pool` | `pool`, `predict` | Warm sandbox pool with predictive autoscaling |
//! | `networking` | `http`, `network` | HTTP client and network policy |
//! | `agent` | `agent`, `llm` | AI agent framework for tool-using sandboxes |
//! | `policy-engine` | `policy`, `audit`, `compose`, `compliance`, `policy_builder`, `policy_file`, `policy_gen`, `policy_lang` | Policy rules, audit logging, composition, policy language |
//! | `platform` | All platform sub-features combined | Full platform services (meta-feature) |
//! | `platform-admin` | `admin`, `gateway`, `orchestrator` | Admin, gateway, orchestrator |
//! | `platform-storage` | `kv`, `sandbox_kv`, `vfs`, `secrets` | KV store, VFS, secrets |
//! | `platform-workflow` | `workflow`, `workflow_engine`, `pipeline` | Workflows and pipelines |
//! | `platform-provenance` | `provenance` | Supply-chain provenance tracking |
//! | `platform-comm` | `ipc`, `rpc`, `graphql_schema` | IPC, RPC, GraphQL |
//! | `platform-hosting` | `serverless`, `playground`, `saas` | Serverless and hosting |
//! | `platform-infra` | `iac` | Infrastructure as code |
//! | `extras` | `ai_exec`, `ai_sandbox`, `benchmark`, `carbon`, `enclave`, `jsrt`, `replay`, `security`, `transpiler`, `verify` | Additional integrations |
//! | `observability` | `dashboard`, `dashboard_api`, `observability`, `tracing_ctx`, `wasm_analytics` | Dashboard, tracing, analytics |
//! | `billing` | `billing`, `cloud_cost` | Billing and cloud cost tracking |
//! | `deployment` | `autoscale`, `deploy`, `hot_reload`, `module_registry`, `oci_registry` | Deployment and registry tools |
//! | `federation` | `federation`, `georep` | Federation and geo-replication |
//!
//! ### Advanced Feature-Gated Modules
//!
//! | Feature | Module | Status | Description |
//! |---------|--------|--------|-------------|
//! | `snapshots` | [`snapshot`] | 🧪 Experimental | Copy-on-write snapshots for warm starts |
//! | `wasi-preview2` | [`wasi2`] | 🧪 Experimental | WASI Preview 2 (Component Model) support |
//! | `debug-support` | [`debug`] | 🧪 Experimental | Live debugging and time-travel replay |
//! | `module-signing` | [`signing`] | 🧪 Experimental | Cryptographic module signing and verification |
//! | `kubernetes` | [`k8s`] | 🧪 Experimental | Kubernetes operator, CRDs, and Helm charts |
//! | `otel-telemetry` | [`telemetry`] | 🧪 Experimental | OpenTelemetry tracing integration |
//! | `distributed-mesh` | [`mesh`] | ⚠️ Simulated | Distributed sandbox clustering (stubs) |
//! | `gpu-compute` | [`gpu`] | ⚠️ Simulated | GPU acceleration (simulated) |
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
//! isolate-core = { version = "0.1", features = ["pool", "networking"] }
//! ```
//!
//! Module group features:
//! - `pool` - Warm sandbox pool with predictive autoscaling
//! - `networking` - HTTP client and network policy modules
//! - `agent` - AI agent framework
//! - `policy-engine` - Policy rules, audit logging, composition, policy language
//! - `platform` - All platform sub-features (meta-feature)
//! - `platform-admin` - Admin, gateway, orchestrator
//! - `platform-storage` - KV store, VFS, secrets
//! - `platform-workflow` - Workflows and pipelines
//! - `platform-provenance` - Supply-chain provenance
//! - `platform-comm` - IPC, RPC, GraphQL
//! - `platform-hosting` - Serverless, playground, SaaS
//! - `platform-infra` - Infrastructure as code
//! - `extras` - AI sandbox, benchmark, carbon tracking, enclave, JS runtime, security, verification
//! - `observability` - Dashboard, tracing context, WASM analytics
//! - `billing` - Billing and cloud cost tracking
//! - `deployment` - Autoscale, deploy, hot reload, module/OCI registry
//! - `federation` - Federation and geo-replication
//!
//! Experimental features:
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

#![warn(missing_docs)]

// Core modules (always available)
pub mod capability;
pub mod coldstart;
pub mod config;
pub mod engine;
pub mod error;
pub mod metrics;
pub mod profile;
pub mod ratelimit;
pub mod resource;
pub mod sandbox;
pub mod sandbox_profile;
pub mod stability;
pub mod version;

// Optional module groups (enabled via feature flags)
#[cfg(feature = "pool")]
pub mod pool;
#[cfg(feature = "pool")]
pub mod predict;

#[cfg(feature = "networking")]
pub mod http;
#[cfg(feature = "networking")]
pub mod network;

#[cfg(feature = "agent")]
pub mod agent;
#[cfg(feature = "agent")]
pub mod llm;

#[cfg(feature = "policy-engine")]
pub mod audit;
#[cfg(feature = "policy-engine")]
pub mod compliance;
#[cfg(feature = "policy-engine")]
pub mod compose;
#[cfg(feature = "policy-engine")]
pub mod policy;
#[cfg(feature = "policy-engine")]
pub mod policy_builder;
#[cfg(feature = "policy-engine")]
pub mod policy_file;
#[cfg(feature = "policy-engine")]
pub mod policy_gen;
#[cfg(feature = "policy-engine")]
pub mod policy_lang;

#[cfg(any(feature = "platform", feature = "platform-admin"))]
pub mod admin;
#[cfg(any(feature = "platform", feature = "platform-admin"))]
pub mod gateway;
#[cfg(any(feature = "platform", feature = "platform-comm"))]
pub mod graphql_schema;
#[cfg(any(feature = "platform", feature = "platform-infra"))]
pub mod iac;
#[cfg(any(feature = "platform", feature = "platform-comm"))]
pub mod ipc;
#[cfg(any(feature = "platform", feature = "platform-storage"))]
pub mod kv;
#[cfg(any(feature = "platform", feature = "platform-admin"))]
pub mod orchestrator;
#[cfg(any(feature = "platform", feature = "platform-workflow"))]
pub mod pipeline;
#[cfg(any(feature = "platform", feature = "platform-hosting"))]
pub mod playground;
#[cfg(any(feature = "platform", feature = "platform-provenance"))]
pub mod provenance;
#[cfg(any(feature = "platform", feature = "platform-comm"))]
pub mod rpc;
#[cfg(any(feature = "platform", feature = "platform-storage"))]
pub mod sandbox_kv;
#[cfg(any(feature = "platform", feature = "platform-storage"))]
pub mod secrets;
#[cfg(any(feature = "platform", feature = "platform-hosting", feature = "serverless"))]
pub mod serverless;
#[cfg(any(feature = "platform", feature = "platform-storage"))]
pub mod vfs;
#[cfg(any(feature = "platform", feature = "platform-workflow"))]
pub mod workflow;
#[cfg(any(feature = "platform", feature = "platform-workflow"))]
pub mod workflow_engine;

#[cfg(any(feature = "platform", feature = "platform-hosting"))]
pub mod saas;

#[cfg(feature = "extras")]
pub mod ai_exec;
#[cfg(feature = "extras")]
pub mod ai_sandbox;
#[cfg(feature = "extras")]
pub mod benchmark;
#[cfg(feature = "extras")]
pub mod carbon;
#[cfg(feature = "extras")]
pub mod enclave;
#[cfg(feature = "extras")]
pub mod jsrt;
#[cfg(feature = "extras")]
pub mod replay;
#[cfg(feature = "extras")]
#[doc(hidden)]
pub mod security;
#[cfg(feature = "extras")]
pub mod transpiler;
#[cfg(feature = "extras")]
#[doc(hidden)]
pub mod verify;

#[cfg(feature = "extras")]
pub mod bench_compare;

#[cfg(feature = "observability")]
pub mod dashboard;
#[cfg(feature = "observability")]
pub mod dashboard_api;
#[cfg(feature = "observability")]
pub mod observability;
#[cfg(feature = "observability")]
pub mod tracing_ctx;
#[cfg(feature = "observability")]
pub mod wasm_analytics;

#[cfg(feature = "billing")]
pub mod billing;
#[cfg(feature = "billing")]
pub mod cloud_cost;

#[cfg(feature = "deployment")]
pub mod autoscale;
#[cfg(feature = "deployment")]
pub mod deploy;
#[cfg(feature = "deployment")]
pub mod hot_reload;
#[cfg(feature = "deployment")]
pub mod module_registry;
#[cfg(feature = "deployment")]
pub mod oci_registry;
#[cfg(feature = "deployment")]
pub mod registry_security;

#[cfg(feature = "federation")]
pub mod federation;
#[cfg(feature = "federation")]
pub mod georep;

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
#[doc(hidden)]
pub mod hotpatch;

#[cfg(feature = "distributed-mesh")]
#[doc(hidden)]
pub mod mesh;

#[cfg(feature = "gpu-compute")]
#[doc(hidden)]
pub mod gpu;

#[cfg(feature = "chaos-testing")]
pub mod chaos;

// Re-export main types at crate root
pub use config::{SandboxConfig, SandboxConfigBuilder};
pub use error::{Error, Result};
pub use profile::LanguageProfile;
pub use sandbox::{Output, Sandbox, SandboxId, SandboxState};
pub use sandbox_profile::SandboxProfile;
