//! Playground execution API for running code snippets in sandboxed environments.
//!
//! Provides types and logic for playground-style execution that a web frontend
//! or API server can use to safely run untrusted code.
//!
//! # Example
//!
//! ```rust,no_run
//! use isolate_core::playground::{PlaygroundExecutor, PlaygroundRequest};
//! use std::time::Duration;
//!
//! # async fn example() -> isolate_core::Result<()> {
//! let executor = PlaygroundExecutor::new(4);
//! let wasm_bytes = std::fs::read("module.wasm")?;
//!
//! let request = PlaygroundRequest::builder("run-1", wasm_bytes)
//!     .timeout(Duration::from_secs(5))
//!     .build();
//!
//! let response = executor.execute(request).await?;
//! println!("stdout: {}", response.stdout);
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use crate::profile::LanguageProfile;
use crate::sandbox::Sandbox;
use crate::sandbox_profile::SandboxProfile;
use crate::SandboxConfig;

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Status of a playground execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Exited with code 0.
    Success,
    /// Exited with a non-zero code.
    NonZeroExit(i32),
    /// Exceeded time limit.
    Timeout,
    /// Exceeded memory limit.
    MemoryExceeded,
    /// Other execution error.
    Error(String),
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "Success"),
            Self::NonZeroExit(code) => write!(f, "NonZeroExit({})", code),
            Self::Timeout => write!(f, "Timeout"),
            Self::MemoryExceeded => write!(f, "MemoryExceeded"),
            Self::Error(msg) => write!(f, "Error: {}", msg),
        }
    }
}

/// A request to execute code in the playground.
#[derive(Debug, Clone)]
pub struct PlaygroundRequest {
    /// Unique ID for tracking.
    pub source_id: String,
    /// Pre-compiled WASM module bytes.
    pub wasm_bytes: Vec<u8>,
    /// Input data provided via stdin.
    pub stdin: Vec<u8>,
    /// Optional language hint for profile tuning.
    pub language: Option<LanguageProfile>,
    /// Override default timeout.
    pub timeout: Option<Duration>,
    /// Override default memory limit in bytes.
    pub memory_limit: Option<usize>,
}

impl PlaygroundRequest {
    /// Create a builder for a new playground request.
    pub fn builder(source_id: impl Into<String>, wasm_bytes: Vec<u8>) -> PlaygroundRequestBuilder {
        PlaygroundRequestBuilder::new(source_id, wasm_bytes)
    }
}

/// Builder for [`PlaygroundRequest`].
#[derive(Debug)]
#[must_use = "builders do nothing unless you call .build()"]
pub struct PlaygroundRequestBuilder {
    source_id: String,
    wasm_bytes: Vec<u8>,
    stdin: Vec<u8>,
    language: Option<LanguageProfile>,
    timeout: Option<Duration>,
    memory_limit: Option<usize>,
}

impl PlaygroundRequestBuilder {
    /// Create a new builder with required fields.
    pub fn new(source_id: impl Into<String>, wasm_bytes: Vec<u8>) -> Self {
        Self {
            source_id: source_id.into(),
            wasm_bytes,
            stdin: Vec::new(),
            language: None,
            timeout: None,
            memory_limit: None,
        }
    }

    /// Set stdin input data.
    pub fn stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = data.into();
        self
    }

    /// Set the language profile hint.
    pub fn language(mut self, profile: LanguageProfile) -> Self {
        self.language = Some(profile);
        self
    }

    /// Set the execution timeout.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Set the memory limit in bytes.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = Some(bytes);
        self
    }

    /// Build the playground request.
    pub fn build(self) -> PlaygroundRequest {
        PlaygroundRequest {
            source_id: self.source_id,
            wasm_bytes: self.wasm_bytes,
            stdin: self.stdin,
            language: self.language,
            timeout: self.timeout,
            memory_limit: self.memory_limit,
        }
    }
}

/// Response from a playground execution.
#[derive(Debug, Clone)]
pub struct PlaygroundResponse {
    /// The source ID from the request.
    pub source_id: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Captured stdout as a string.
    pub stdout: String,
    /// Captured stderr as a string.
    pub stderr: String,
    /// Wall-clock execution time.
    pub execution_time: Duration,
    /// Peak memory used in bytes.
    pub memory_used: usize,
    /// Execution outcome status.
    pub status: ExecutionStatus,
}

/// Executor for playground requests with concurrency control.
///
/// Uses a [`tokio::sync::Semaphore`] to limit the number of concurrent
/// sandbox executions.
pub struct PlaygroundExecutor {
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl PlaygroundExecutor {
    /// Create a new executor with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
        }
    }

    /// Return the number of currently active (in-flight) executions.
    pub fn active_count(&self) -> usize {
        self.max_concurrent - self.semaphore.available_permits()
    }

    /// Execute a playground request and return the response.
    ///
    /// Blocks until a semaphore permit is available, then runs the WASM
    /// module inside a sandbox configured with [`SandboxProfile::Playground`].
    pub async fn execute(&self, request: PlaygroundRequest) -> Result<PlaygroundResponse> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| Error::Execution(format!("semaphore closed: {}", e)))?;

        let start = Instant::now();
        let source_id = request.source_id.clone();

        let mut builder = SandboxConfig::builder()
            .module(&request.wasm_bytes)?
            .use_profile(SandboxProfile::Playground);

        if let Some(lang) = request.language {
            builder = builder.apply_profile(lang);
        }

        if let Some(timeout) = request.timeout {
            builder = builder.wall_time_limit(timeout);
        }

        if let Some(mem) = request.memory_limit {
            builder = builder.memory_limit(mem);
        }

        let config = builder.build()?;

        let mut sandbox = Sandbox::create(config).await?;
        let result = sandbox.run(&request.stdin).await;
        let execution_time = start.elapsed();

        match result {
            Ok(output) => {
                let status = if output.exit_code == 0 {
                    ExecutionStatus::Success
                } else {
                    ExecutionStatus::NonZeroExit(output.exit_code)
                };

                Ok(PlaygroundResponse {
                    source_id,
                    exit_code: output.exit_code,
                    stdout: output.stdout_str(),
                    stderr: output.stderr_str(),
                    execution_time,
                    memory_used: output.resource_usage.peak_memory,
                    status,
                })
            }
            Err(e) => {
                let status = match &e {
                    Error::Timeout(_) => ExecutionStatus::Timeout,
                    Error::MemoryLimitExceeded { .. } => ExecutionStatus::MemoryExceeded,
                    other => ExecutionStatus::Error(other.to_string()),
                };

                Ok(PlaygroundResponse {
                    source_id,
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: e.to_string(),
                    execution_time,
                    memory_used: 0,
                    status,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::*;
    const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");
    const EXIT_42_WASM: &[u8] = include_bytes!("../tests/fixtures/exit_42.wasm");

    #[test]
    fn test_builder_creates_valid_request() {
        let req = PlaygroundRequest::builder("test-1", vec![0x00, 0x61, 0x73, 0x6d])
            .build();

        assert_eq!(req.source_id, "test-1");
        assert_eq!(req.wasm_bytes, vec![0x00, 0x61, 0x73, 0x6d]);
    }

    #[test]
    fn test_default_values() {
        let req = PlaygroundRequest::builder("test-2", vec![1, 2, 3])
            .build();

        assert!(req.stdin.is_empty());
        assert!(req.language.is_none());
        assert!(req.timeout.is_none());
        assert!(req.memory_limit.is_none());
    }

    #[test]
    fn test_request_with_all_optional_fields() {
        let req = PlaygroundRequest::builder("full", vec![0x00])
            .stdin(b"hello input".to_vec())
            .language(LanguageProfile::Rust)
            .timeout(Duration::from_secs(5))
            .memory_limit(64 * 1024 * 1024)
            .build();

        assert_eq!(req.stdin, b"hello input");
        assert_eq!(req.language, Some(LanguageProfile::Rust));
        assert_eq!(req.timeout, Some(Duration::from_secs(5)));
        assert_eq!(req.memory_limit, Some(64 * 1024 * 1024));
    }

    #[test]
    fn test_executor_creation() {
        let executor = PlaygroundExecutor::new(8);
        assert_eq!(executor.max_concurrent, 8);
    }

    #[test]
    fn test_active_count_starts_at_zero() {
        let executor = PlaygroundExecutor::new(4);
        assert_eq!(executor.active_count(), 0);
    }

    #[test]
    fn test_execution_status_display() {
        assert_eq!(ExecutionStatus::Success.to_string(), "Success");
        assert_eq!(ExecutionStatus::NonZeroExit(1).to_string(), "NonZeroExit(1)");
        assert_eq!(ExecutionStatus::Timeout.to_string(), "Timeout");
        assert_eq!(ExecutionStatus::MemoryExceeded.to_string(), "MemoryExceeded");
        assert_eq!(
            ExecutionStatus::Error("oops".to_string()).to_string(),
            "Error: oops"
        );
    }

    #[test]
    fn test_playground_response_creation() {
        let resp = PlaygroundResponse {
            source_id: "resp-1".to_string(),
            exit_code: 0,
            stdout: "hello".to_string(),
            stderr: String::new(),
            execution_time: Duration::from_millis(42),
            memory_used: 1024,
            status: ExecutionStatus::Success,
        };

        assert_eq!(resp.source_id, "resp-1");
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "hello");
        assert_eq!(resp.status, ExecutionStatus::Success);
    }

    #[tokio::test]
    async fn test_executor_with_hello_wasm() {
        let executor = PlaygroundExecutor::new(2);
        let request = PlaygroundRequest::builder("hello-test", HELLO_WASM.to_vec())
            .build();

        let response = executor.execute(request).await.expect("execution should succeed");

        assert_eq!(response.source_id, "hello-test");
        assert!(
            response.stdout.contains("Hello"),
            "stdout should contain 'Hello', got: {}",
            response.stdout
        );
        assert_eq!(response.status, ExecutionStatus::Success);
        assert_eq!(response.exit_code, 0);
    }

    #[tokio::test]
    async fn test_executor_with_exit_42_wasm() {
        let executor = PlaygroundExecutor::new(2);
        let request = PlaygroundRequest::builder("exit42-test", EXIT_42_WASM.to_vec())
            .build();

        let response = executor.execute(request).await.expect("execution should succeed");

        assert_eq!(response.source_id, "exit42-test");
        assert_eq!(response.exit_code, 42);
        assert_eq!(response.status, ExecutionStatus::NonZeroExit(42));
    }

    #[tokio::test]
    async fn test_concurrent_requests_with_semaphore() {
        let executor = Arc::new(PlaygroundExecutor::new(2));

        let mut handles = Vec::new();
        for i in 0..4 {
            let exec = executor.clone();
            let handle = tokio::spawn(async move {
                let request = PlaygroundRequest::builder(
                    format!("concurrent-{}", i),
                    HELLO_WASM.to_vec(),
                )
                .build();
                exec.execute(request).await
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.expect("task should not panic"));
        }

        for result in &results {
            let resp = result.as_ref().expect("execution should succeed");
            assert_eq!(resp.status, ExecutionStatus::Success);
        }
        assert_eq!(results.len(), 4);
    }
}
