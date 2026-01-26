//! # Isolate Embed: Minimal Embeddable WASM Sandbox
//!
//! A single-crate embeddable library for running WASM modules in a secure
//! sandbox. Designed for embedding in C/C++/Rust applications with:
//!
//! - **Zero async requirement**: Fully synchronous API
//! - **Minimal dependencies**: Only Wasmtime + thiserror
//! - **Simple API**: Create → Run → Get output
//! - **C FFI layer**: Optional `cffi` feature for C/C++ embedding
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use isolate_embed::{Sandbox, SandboxConfig};
//!
//! let wasm_bytes = std::fs::read("module.wasm").unwrap();
//! let config = SandboxConfig::new(&wasm_bytes)
//!     .memory_limit(64 * 1024 * 1024)
//!     .fuel(1_000_000);
//!
//! let mut sandbox = Sandbox::create(config).unwrap();
//! let output = sandbox.run(&[]).unwrap();
//!
//! println!("Exit code: {}", output.exit_code);
//! println!("Stdout: {}", output.stdout_str());
//! ```

mod sandbox;
pub use sandbox::*;

#[cfg(feature = "cffi")]
pub mod ffi;
