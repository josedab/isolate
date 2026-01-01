//! # Isolate: Secure Sandbox Runtime
//!
//! Isolate is a lightweight, secure sandbox runtime designed to execute untrusted
//! code with strong isolation guarantees. It combines WebAssembly (WASM) isolation
//! with capability-based security and resource controls.
//!
//! ## Features
//!
//! - **Fast Cold Start**: <5ms sandbox creation
//! - **Memory Safety**: Rust implementation eliminates runtime vulnerabilities
//! - **Multi-Language**: Execute any WASM-compiled language
//! - **Capability-Based**: Fine-grained permission control
//! - **Resource Limits**: CPU, memory, I/O quotas with enforcement
//! - **Snapshot/Restore**: Sub-millisecond warm starts
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

pub mod ai;
pub mod audit;
pub mod capability;
pub mod carbon;
pub mod chaos;
pub mod compose;
pub mod config;
pub mod debug;
pub mod enclave;
pub mod engine;
pub mod error;
pub mod gpu;
pub mod hotpatch;
pub mod http;
pub mod ipc;
pub mod k8s;
pub mod mesh;
pub mod metrics;
pub mod nlp;
pub mod plugin;
pub mod pool;
pub mod predict;
pub mod provenance;
pub mod resource;
pub mod sandbox;
pub mod secrets;
pub mod security;
pub mod signing;
pub mod snapshot;
pub mod telemetry;
pub mod verify;
pub mod wasi2;
pub mod workflow;

// Re-export main types at crate root
pub use config::{SandboxConfig, SandboxConfigBuilder};
pub use error::{Error, Result};
pub use sandbox::{Output, Sandbox, SandboxId, SandboxState};
