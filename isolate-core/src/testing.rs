//! Test utilities for writing tests with Isolate.
//!
//! This module provides helper functions and constants that reduce boilerplate
//! when testing WASM sandbox execution. It's designed for use in both the
//! isolate-core test suite and by external crate consumers.
//!
//! # Quick Start
//!
//! ```rust
//! use isolate_core::testing::{EXIT_OK_WASM, TestSandboxBuilder};
//!
//! # #[tokio::main]
//! # async fn main() -> isolate_core::Result<()> {
//! let output = TestSandboxBuilder::new(EXIT_OK_WASM)
//!     .with_stdout()
//!     .fuel(1_000_000)
//!     .run()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use crate::capability::Capability;
use crate::config::SandboxConfig;
use crate::error::Result;
use crate::sandbox::{Output, Sandbox};
use std::time::Duration;

/// Minimal valid WASM module (8 bytes: magic + version, no sections).
///
/// This module is valid WASM but has no exports. Useful for testing
/// module validation and compilation but will fail at execution
/// because there is no `_start` entry point.
pub const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x01, 0x00, 0x00, 0x00, // version: 1
];

/// WASM module that calls proc_exit(0) — the simplest runnable module.
///
/// This module exports `_start` and immediately exits with code 0.
/// Use it when you need a module that actually runs successfully.
pub const EXIT_OK_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");

/// WASM module that writes "Hello from WASM!\n" to stdout.
///
/// Requires the stdout capability to capture output.
pub const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

/// WASM module that exits with code 42.
///
/// Useful for testing non-zero exit code handling.
pub const EXIT_42_WASM: &[u8] = include_bytes!("../tests/fixtures/exit_42.wasm");

/// A builder for quickly creating test sandboxes with sensible defaults.
///
/// Provides a simplified API over [`SandboxConfigBuilder`](crate::SandboxConfigBuilder)
/// with reasonable defaults for testing (1M fuel, 64MB memory, 10s timeout).
///
/// # Examples
///
/// ```rust
/// use isolate_core::testing::{EXIT_OK_WASM, TestSandboxBuilder};
///
/// # #[tokio::main]
/// # async fn main() -> isolate_core::Result<()> {
/// // Run a module with all defaults
/// let output = TestSandboxBuilder::new(EXIT_OK_WASM)
///     .run()
///     .await?;
/// assert!(output.success());
///
/// // Customize for specific test needs
/// let output = TestSandboxBuilder::new(EXIT_OK_WASM)
///     .with_stdout()
///     .fuel(500_000)
///     .memory_limit(32 * 1024 * 1024)
///     .label("test", "custom")
///     .run()
///     .await?;
/// assert!(output.success());
/// # Ok(())
/// # }
/// ```
pub struct TestSandboxBuilder {
    wasm: Vec<u8>,
    fuel: u64,
    memory_limit: usize,
    timeout: Duration,
    capabilities: Vec<Capability>,
    labels: Vec<(String, String)>,
    env: Vec<(String, String)>,
    args: Vec<String>,
    entry_point: Option<String>,
}

impl TestSandboxBuilder {
    /// Create a new test sandbox builder with the given WASM module bytes.
    ///
    /// Defaults: 1M fuel, 64MB memory, 10s timeout, no capabilities.
    pub fn new(wasm: &[u8]) -> Self {
        Self {
            wasm: wasm.to_vec(),
            fuel: 1_000_000,
            memory_limit: 64 * 1024 * 1024,
            timeout: Duration::from_secs(10),
            capabilities: Vec::new(),
            labels: Vec::new(),
            env: Vec::new(),
            args: Vec::new(),
            entry_point: None,
        }
    }

    /// Grant stdout capability.
    pub fn with_stdout(mut self) -> Self {
        self.capabilities.push(Capability::stdout());
        self
    }

    /// Grant stderr capability.
    pub fn with_stderr(mut self) -> Self {
        self.capabilities.push(Capability::stderr());
        self
    }

    /// Grant all stdio capabilities (stdin, stdout, stderr).
    pub fn with_stdio(mut self) -> Self {
        self.capabilities.push(Capability::stdin());
        self.capabilities.push(Capability::stdout());
        self.capabilities.push(Capability::stderr());
        self
    }

    /// Set the fuel limit.
    pub fn fuel(mut self, fuel: u64) -> Self {
        self.fuel = fuel;
        self
    }

    /// Set the memory limit in bytes.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = bytes;
        self
    }

    /// Set the wall-clock timeout.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = duration;
        self
    }

    /// Add a metadata label.
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        self
    }

    /// Add a capability.
    pub fn capability(mut self, cap: Capability) -> Self {
        self.capabilities.push(cap);
        self
    }

    /// Set an environment variable for the sandbox.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self.capabilities.push(Capability::env_all());
        self
    }

    /// Grant filesystem read capability for a path.
    pub fn with_fs_read(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.capabilities.push(Capability::filesystem_read(path));
        self
    }

    /// Set a custom entry point function name.
    pub fn entry_point(mut self, name: impl Into<String>) -> Self {
        self.entry_point = Some(name.into());
        self
    }

    /// Add a command-line argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Build the SandboxConfig without running.
    pub fn build_config(self) -> Result<SandboxConfig> {
        let mut builder = SandboxConfig::builder()
            .module(&self.wasm)?
            .fuel(self.fuel)
            .memory_limit(self.memory_limit)
            .wall_time_limit(self.timeout);

        for cap in self.capabilities {
            builder = builder.capability(cap);
        }

        for (k, v) in self.labels {
            builder = builder.label(k, v);
        }

        for (k, v) in self.env {
            builder = builder.env(k, v);
        }

        for arg in self.args {
            builder = builder.arg(arg);
        }

        if let Some(ep) = self.entry_point {
            builder = builder.entry_point(ep);
        }

        builder.build()
    }

    /// Build, create, and run the sandbox in one step.
    ///
    /// This is the most common test pattern: configure → create → run → inspect output.
    pub async fn run(self) -> Result<Output> {
        let config = self.build_config()?;
        let mut sandbox = Sandbox::create(config).await?;
        sandbox.run(&[]).await
    }
}

/// Quickly run a WASM module with default settings and return the output.
///
/// This is the shortest path from WASM bytes to execution output.
/// Uses 1M fuel, 64MB memory, 10s timeout, and stdout capability.
///
/// # Examples
///
/// ```rust
/// use isolate_core::testing::{quick_run, EXIT_OK_WASM};
///
/// # #[tokio::main]
/// # async fn main() -> isolate_core::Result<()> {
/// let output = quick_run(EXIT_OK_WASM).await?;
/// assert!(output.success());
/// # Ok(())
/// # }
/// ```
pub async fn quick_run(wasm: &[u8]) -> Result<Output> {
    TestSandboxBuilder::new(wasm).with_stdout().run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_wasm_valid() {
        // MINIMAL_WASM should be parseable
        let module = crate::config::WasmModule::from_bytes(MINIMAL_WASM.to_vec());
        assert!(module.is_ok());
    }

    #[test]
    fn test_test_sandbox_builder_defaults() {
        let builder = TestSandboxBuilder::new(EXIT_OK_WASM);
        assert_eq!(builder.fuel, 1_000_000);
        assert_eq!(builder.memory_limit, 64 * 1024 * 1024);
        assert_eq!(builder.timeout, Duration::from_secs(10));
        assert!(builder.capabilities.is_empty());
    }

    #[test]
    fn test_test_sandbox_builder_config() {
        let config = TestSandboxBuilder::new(EXIT_OK_WASM)
            .with_stdout()
            .fuel(500_000)
            .memory_limit(32 * 1024 * 1024)
            .timeout(Duration::from_secs(5))
            .label("test", "value")
            .build_config()
            .unwrap();

        assert_eq!(config.resources.cpu.fuel, Some(500_000));
        assert_eq!(config.resources.memory.heap_max, 32 * 1024 * 1024);
        assert!(!config.capabilities.is_empty());
        assert_eq!(config.metadata.get("test").unwrap(), "value");
    }

    #[tokio::test]
    async fn test_test_sandbox_builder_run() {
        let output = TestSandboxBuilder::new(EXIT_OK_WASM).run().await.unwrap();
        assert!(output.success());
    }

    #[tokio::test]
    async fn test_quick_run() {
        let output = quick_run(EXIT_OK_WASM).await.unwrap();
        assert!(output.success());
    }

    #[tokio::test]
    async fn test_hello_wasm_with_stdout() {
        let output = TestSandboxBuilder::new(HELLO_WASM).with_stdout().run().await.unwrap();
        assert!(output.success());
        assert_eq!(output.stdout_str(), "Hello from WASM!\n");
    }

    #[tokio::test]
    async fn test_exit_42_wasm() {
        let output = TestSandboxBuilder::new(EXIT_42_WASM).run().await.unwrap();
        assert_eq!(output.exit_code, 42);
        assert!(!output.success());
    }
}
