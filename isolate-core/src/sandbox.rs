//! Sandbox management and lifecycle.
//!
//! This module provides the main [`Sandbox`] type for creating and running
//! isolated WASM code.
//!
//! # Overview
//!
//! A sandbox provides secure, isolated execution of WebAssembly modules with:
//! - **Memory isolation**: Each sandbox has its own linear memory
//! - **Capability-based security**: Explicit permission grants for I/O operations
//! - **Resource limits**: CPU time, memory, and I/O quotas
//! - **Output capture**: Stdout and stderr are captured and returned
//!
//! # Example
//!
//! ```no_run
//! use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
//! use std::time::Duration;
//!
//! # async fn example() -> isolate_core::Result<()> {
//! // Load WASM module bytes
//! let wasm_bytes = std::fs::read("my_module.wasm")?;
//!
//! // Configure the sandbox with capabilities and limits
//! let config = SandboxConfig::builder()
//!     .module(&wasm_bytes)?
//!     .memory_limit(64 * 1024 * 1024)  // 64 MB
//!     .fuel(10_000_000)                 // CPU limit
//!     .wall_time_limit(Duration::from_secs(30))
//!     .capability(Capability::stdout()) // Allow stdout
//!     .capability(Capability::stderr()) // Allow stderr
//!     .env("API_KEY", "secret")         // Environment variable
//!     .build()?;
//!
//! // Create and run the sandbox
//! let mut sandbox = Sandbox::create(config).await?;
//! let output = sandbox.run(&[]).await?;
//!
//! // Check results
//! if output.success() {
//!     println!("Output: {}", output.stdout_str());
//! } else {
//!     eprintln!("Error (exit {}): {}", output.exit_code, output.stderr_str());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Lifecycle
//!
//! A sandbox goes through the following states:
//!
//! 1. **Creating** - Module is being compiled
//! 2. **Ready** - Sandbox is ready to execute
//! 3. **Running** - Currently executing WASM code
//! 4. **Terminated** - Execution completed (or failed)
//!
//! After termination, the sandbox cannot be reused.

use crate::capability::CapabilityEnforcer;
use crate::config::{ModuleHash, SandboxConfig};
use crate::engine::{CompiledModule, WasmEngine, WasmInstance};
use crate::error::{Error, Result};
use crate::lineage::{ExecutionLog, ExecutionTrace};
use crate::metrics::SandboxMetrics;
use crate::ratelimit::SharedRateLimiter;
use crate::resource::{ResourceMeter, ResourceUsage};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Lifecycle event types for sandbox hook notifications.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SandboxEvent {
    /// Sandbox has been created and is in Ready state.
    Created {
        /// Sandbox ID.
        sandbox_id: SandboxId,
        /// Cold start duration (compilation + setup).
        cold_start: Duration,
    },
    /// Sandbox execution is starting.
    RunStarted {
        /// Sandbox ID.
        sandbox_id: SandboxId,
    },
    /// Sandbox execution completed successfully.
    RunCompleted {
        /// Sandbox ID.
        sandbox_id: SandboxId,
        /// Execution output.
        output: Output,
    },
    /// Sandbox execution failed with an error.
    RunFailed {
        /// Sandbox ID.
        sandbox_id: SandboxId,
        /// Error message.
        error: String,
    },
    /// Sandbox has been terminated.
    Terminated {
        /// Sandbox ID.
        sandbox_id: SandboxId,
        /// Lifetime metrics.
        metrics: SandboxMetrics,
    },
}

/// Trait for receiving sandbox lifecycle events.
///
/// Implement this trait to be notified of sandbox state transitions.
/// All methods have default no-op implementations, so you only need to
/// override the events you care about.
///
/// # Examples
///
/// ```
/// use isolate_core::sandbox::{SandboxHooks, SandboxEvent};
///
/// struct LoggingHooks;
///
/// impl SandboxHooks for LoggingHooks {
///     fn on_event(&self, event: &SandboxEvent) {
///         match event {
///             SandboxEvent::Created { sandbox_id, cold_start } => {
///                 println!("Sandbox {} created in {:?}", sandbox_id, cold_start);
///             }
///             SandboxEvent::RunCompleted { sandbox_id, output } => {
///                 println!("Sandbox {} completed with exit code {}", sandbox_id, output.exit_code);
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait SandboxHooks: Send + Sync {
    /// Called for any sandbox lifecycle event.
    fn on_event(&self, event: &SandboxEvent);
}

/// A no-op hooks implementation (default).
struct NoOpHooks;
impl SandboxHooks for NoOpHooks {
    fn on_event(&self, _event: &SandboxEvent) {}
}

/// Unique identifier for a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(pub Uuid);

impl SandboxId {
    /// Create a new random sandbox ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SandboxId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SandboxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for SandboxId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// State of a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SandboxState {
    /// Sandbox is being created.
    Creating,
    /// Sandbox is ready to run.
    Ready,
    /// Sandbox is currently running.
    Running,
    /// Sandbox execution is paused.
    Paused,
    /// Sandbox has terminated.
    Terminated,
}

impl std::fmt::Display for SandboxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Ready => write!(f, "ready"),
            Self::Running => write!(f, "running"),
            Self::Paused => write!(f, "paused"),
            Self::Terminated => write!(f, "terminated"),
        }
    }
}

/// Output from sandbox execution.
///
/// # Examples
///
/// ```
/// use isolate_core::Output;
/// use isolate_core::resource::ResourceUsage;
/// use std::time::Duration;
///
/// let output = Output {
///     exit_code: 0,
///     stdout: b"Hello, World!".to_vec(),
///     stderr: Vec::new(),
///     duration: Duration::from_millis(42),
///     resource_usage: ResourceUsage::default(),
/// };
///
/// assert!(output.success());
/// assert_eq!(output.stdout_str(), "Hello, World!");
/// assert_eq!(output.stderr_str(), "");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    /// Exit code (0 for success).
    pub exit_code: i32,
    /// Captured stdout.
    #[serde(with = "serde_bytes")]
    pub stdout: Vec<u8>,
    /// Captured stderr.
    #[serde(with = "serde_bytes")]
    pub stderr: Vec<u8>,
    /// Execution duration.
    pub duration: Duration,
    /// Resource usage.
    pub resource_usage: ResourceUsage,
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit={}, stdout={}B, stderr={}B, duration={:.1}ms, fuel={}",
            self.exit_code,
            self.stdout.len(),
            self.stderr.len(),
            self.duration.as_secs_f64() * 1000.0,
            self.resource_usage.fuel_consumed,
        )
    }
}

impl Output {
    /// Check if the execution was successful (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    /// Get stdout as a string (lossy UTF-8 conversion).
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Get stderr as a string (lossy UTF-8 conversion).
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// Parse stdout as JSON lines (newline-delimited JSON).
    ///
    /// Returns successfully parsed JSON values, skipping non-JSON lines.
    /// Useful for structured logging from sandboxes.
    pub fn structured_stdout(&self) -> Vec<serde_json::Value> {
        let text = String::from_utf8_lossy(&self.stdout);
        text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    /// Parse stderr as JSON lines.
    pub fn structured_stderr(&self) -> Vec<serde_json::Value> {
        let text = String::from_utf8_lossy(&self.stderr);
        text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    /// Get stdout split into lines.
    pub fn stdout_lines(&self) -> Vec<String> {
        let text = String::from_utf8_lossy(&self.stdout);
        text.lines().map(String::from).collect()
    }

    /// Parse the entire stdout as a typed JSON value.
    ///
    /// Deserializes the full stdout contents as `T` using serde_json.
    /// Useful when the sandbox produces a single JSON object as output.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::Output;
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize, Debug, PartialEq)]
    /// struct Result { value: i32 }
    ///
    /// let output = Output {
    ///     exit_code: 0,
    ///     stdout: br#"{"value": 42}"#.to_vec(),
    ///     stderr: Vec::new(),
    ///     duration: Duration::ZERO,
    ///     resource_usage: ResourceUsage::default(),
    /// };
    ///
    /// let parsed: Result = output.parse_stdout_as().unwrap();
    /// assert_eq!(parsed.value, 42);
    /// ```
    pub fn parse_stdout_as<T: serde::de::DeserializeOwned>(
        &self,
    ) -> std::result::Result<T, serde_json::Error> {
        serde_json::from_slice(&self.stdout)
    }

    /// Parse the entire stderr as a typed JSON value.
    pub fn parse_stderr_as<T: serde::de::DeserializeOwned>(
        &self,
    ) -> std::result::Result<T, serde_json::Error> {
        serde_json::from_slice(&self.stderr)
    }

    /// Get the size of stdout in bytes.
    pub fn stdout_size(&self) -> usize {
        self.stdout.len()
    }

    /// Get the size of stderr in bytes.
    pub fn stderr_size(&self) -> usize {
        self.stderr.len()
    }

    /// Get combined stdout and stderr as bytes (stdout first, then stderr).
    ///
    /// Useful for capturing all output when the distinction doesn't matter.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::Output;
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    ///
    /// let output = Output {
    ///     exit_code: 0,
    ///     stdout: b"hello ".to_vec(),
    ///     stderr: b"world".to_vec(),
    ///     duration: Duration::ZERO,
    ///     resource_usage: ResourceUsage::default(),
    /// };
    ///
    /// assert_eq!(output.combined_output(), b"hello world");
    /// ```
    pub fn combined_output(&self) -> Vec<u8> {
        let mut combined = Vec::with_capacity(self.stdout.len() + self.stderr.len());
        combined.extend_from_slice(&self.stdout);
        combined.extend_from_slice(&self.stderr);
        combined
    }

    /// Get combined stdout and stderr as a string (lossy UTF-8 conversion).
    pub fn combined_output_str(&self) -> String {
        String::from_utf8_lossy(&self.combined_output()).into_owned()
    }

    /// Get a truncated preview of stdout, capped at `max_bytes`.
    ///
    /// Truncates at a valid UTF-8 boundary to avoid splitting multi-byte
    /// characters. Appends "..." when truncated.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::Output;
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    ///
    /// let output = Output {
    ///     exit_code: 0,
    ///     stdout: b"Hello, World!".to_vec(),
    ///     stderr: Vec::new(),
    ///     duration: Duration::ZERO,
    ///     resource_usage: ResourceUsage::default(),
    /// };
    ///
    /// assert_eq!(output.truncated_stdout(5), "Hello...");
    /// assert_eq!(output.truncated_stdout(100), "Hello, World!");
    /// ```
    pub fn truncated_stdout(&self, max_bytes: usize) -> String {
        truncate_utf8_lossy(&self.stdout, max_bytes)
    }

    /// Get a truncated preview of stderr, capped at `max_bytes`.
    ///
    /// Truncates at a valid UTF-8 boundary and appends "..." when truncated.
    pub fn truncated_stderr(&self, max_bytes: usize) -> String {
        truncate_utf8_lossy(&self.stderr, max_bytes)
    }

    /// Get a human-readable one-line summary of this execution.
    ///
    /// Format: `exit=N duration=Xms mem=Y fuel=Z`
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::Output;
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    ///
    /// let output = Output {
    ///     exit_code: 0,
    ///     stdout: b"hello".to_vec(),
    ///     stderr: Vec::new(),
    ///     duration: Duration::from_millis(42),
    ///     resource_usage: ResourceUsage {
    ///         wall_time: Duration::from_millis(42),
    ///         ..Default::default()
    ///     },
    /// };
    /// let summary = output.summary();
    /// assert!(summary.contains("exit=0"));
    /// assert!(summary.contains("42.0ms"));
    /// ```
    pub fn summary(&self) -> String {
        format!(
            "exit={} {} stdout={} stderr={}",
            self.exit_code,
            self.resource_usage,
            crate::resource::format_bytes(self.stdout.len() as u64),
            crate::resource::format_bytes(self.stderr.len() as u64),
        )
    }

    /// Get a structured execution summary.
    ///
    /// Returns an [`ExecutionSummary`] with typed fields for programmatic
    /// access. The summary also implements `Display` for logging.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::Output;
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    ///
    /// let output = Output {
    ///     exit_code: 0,
    ///     stdout: b"hello".to_vec(),
    ///     stderr: Vec::new(),
    ///     duration: Duration::from_millis(42),
    ///     resource_usage: ResourceUsage {
    ///         fuel_consumed: 5000,
    ///         peak_memory: 1024 * 1024,
    ///         ..Default::default()
    ///     },
    /// };
    ///
    /// let summary = output.execution_summary();
    /// assert!(summary.success);
    /// assert_eq!(summary.exit_code, 0);
    /// assert_eq!(summary.stdout_bytes, 5);
    /// println!("{}", summary); // Formatted for logging
    /// ```
    pub fn execution_summary(&self) -> ExecutionSummary {
        ExecutionSummary {
            success: self.success(),
            exit_code: self.exit_code,
            duration: self.duration,
            fuel_consumed: self.resource_usage.fuel_consumed,
            peak_memory: self.resource_usage.peak_memory,
            stdout_bytes: self.stdout.len(),
            stderr_bytes: self.stderr.len(),
            bytes_read: self.resource_usage.bytes_read,
            bytes_written: self.resource_usage.bytes_written,
        }
    }
}

/// Structured summary of a sandbox execution.
///
/// Provides typed access to key execution metrics. Implements `Display`
/// for human-readable logging output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Whether the execution succeeded (exit code 0).
    pub success: bool,
    /// Process exit code.
    pub exit_code: i32,
    /// Wall-clock execution duration.
    pub duration: Duration,
    /// Total fuel consumed.
    pub fuel_consumed: u64,
    /// Peak memory usage in bytes.
    pub peak_memory: usize,
    /// Size of captured stdout in bytes.
    pub stdout_bytes: usize,
    /// Size of captured stderr in bytes.
    pub stderr_bytes: usize,
    /// Total bytes read during execution.
    pub bytes_read: u64,
    /// Total bytes written during execution.
    pub bytes_written: u64,
}

impl std::fmt::Display for ExecutionSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.success { "OK" } else { "FAIL" };
        write!(
            f,
            "[{}] exit={} duration={} fuel={} mem={} stdout={} stderr={}",
            status,
            self.exit_code,
            crate::resource::format_duration(self.duration),
            self.fuel_consumed,
            crate::resource::format_bytes(self.peak_memory as u64),
            crate::resource::format_bytes(self.stdout_bytes as u64),
            crate::resource::format_bytes(self.stderr_bytes as u64),
        )
    }
}

/// Truncate bytes at a valid UTF-8 boundary and return a string.
/// Appends "..." if truncation occurred.
fn truncate_utf8_lossy(bytes: &[u8], max_bytes: usize) -> String {
    if bytes.len() <= max_bytes {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let lossy = String::from_utf8_lossy(&bytes[..max_bytes]);
    // If the last character was replaced, we might have split a multi-byte char
    let truncated = lossy.trim_end_matches(char::REPLACEMENT_CHARACTER);
    format!("{}...", truncated)
}

mod serde_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            String::from_utf8_lossy(bytes).serialize(serializer)
        } else {
            bytes.serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            Ok(s.into_bytes())
        } else {
            Vec::<u8>::deserialize(deserializer)
        }
    }
}

/// A secure sandbox for executing WASM code.
///
/// A `Sandbox` provides isolated execution of WebAssembly modules with
/// capability-based security, resource limits, and output capture.
///
/// # Creating a Sandbox
///
/// ```no_run
/// # use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
/// # async fn example() -> isolate_core::Result<()> {
/// let wasm = std::fs::read("module.wasm")?;
/// let config = SandboxConfig::builder()
///     .module(&wasm)?
///     .capability(Capability::stdout())
///     .build()?;
///
/// let sandbox = Sandbox::create(config).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Sharing an Engine
///
/// For better performance when creating many sandboxes, share a [`WasmEngine`]:
///
/// ```no_run
/// # use isolate_core::{Sandbox, SandboxConfig, engine::WasmEngine};
/// # use std::sync::Arc;
/// # async fn example() -> isolate_core::Result<()> {
/// let engine = Arc::new(WasmEngine::new()?);
///
/// // Create multiple sandboxes sharing the same engine
/// let config1 = SandboxConfig::builder()
///     .module(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])?
///     .build()?;
/// let sandbox1 = Sandbox::create_with_engine(config1, engine.clone()).await?;
///
/// let config2 = SandboxConfig::builder()
///     .module(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])?
///     .build()?;
/// let sandbox2 = Sandbox::create_with_engine(config2, engine.clone()).await?;
///
/// // Both sandboxes share compiled module cache
/// assert_eq!(engine.cached_module_count(), 1);
/// # Ok(())
/// # }
/// ```
///
/// [`WasmEngine`]: crate::engine::WasmEngine
pub struct Sandbox {
    /// Unique identifier.
    id: SandboxId,
    /// Current state.
    state: SandboxState,
    /// Configuration.
    config: SandboxConfig,
    /// WASM engine (shared).
    engine: Arc<WasmEngine>,
    /// Compiled module.
    compiled: CompiledModule,
    /// WASM instance (created on run).
    instance: Mutex<Option<WasmInstance>>,
    /// Capability enforcer.
    enforcer: CapabilityEnforcer,
    /// Resource meter.
    meter: ResourceMeter,
    /// Rate limiter (if configured).
    rate_limiter: Option<SharedRateLimiter>,
    /// Metrics collector.
    metrics: SandboxMetrics,
    /// Creation time.
    created_at: Instant,
    /// Lifecycle hooks.
    hooks: Arc<dyn SandboxHooks>,
    /// Execution lineage log.
    execution_log: Arc<std::sync::Mutex<ExecutionLog>>,
}

impl Sandbox {
    /// Create a new sandbox with the given configuration.
    pub async fn create(config: SandboxConfig) -> Result<Self> {
        Self::create_with_engine(config, Arc::new(WasmEngine::new()?)).await
    }

    /// Create a new sandbox with a shared engine.
    pub async fn create_with_engine(
        config: SandboxConfig,
        engine: Arc<WasmEngine>,
    ) -> Result<Self> {
        let start = Instant::now();
        let id = SandboxId::new();

        tracing::debug!(sandbox_id = %id, "Creating sandbox");

        // Compile the module
        let compiled = engine.compile(&config.module)?;

        // Validate config against compiled module and emit warnings
        let warnings = config.validate_against_module(&compiled);
        for warning in &warnings {
            tracing::warn!(
                sandbox_id = %id,
                kind = ?warning.kind,
                suggestion = %warning.suggestion,
                "Config warning: {}",
                warning.message
            );
        }

        // Create capability enforcer
        let enforcer = CapabilityEnforcer::new(config.capabilities.clone(), id.0);

        // Create resource meter
        let meter = ResourceMeter::new(config.resources.clone());

        // Create metrics
        let metrics = SandboxMetrics::new(id);

        // Create rate limiter if configured
        let rate_limiter = if config.rate_limit.is_enabled() {
            Some(SharedRateLimiter::new(config.rate_limit.clone()))
        } else {
            None
        };

        let cold_start = start.elapsed();
        tracing::info!(
            sandbox_id = %id,
            cold_start_ms = cold_start.as_secs_f64() * 1000.0,
            module_hash = %compiled.hash(),
            "Sandbox created"
        );

        Ok(Self {
            id,
            state: SandboxState::Ready,
            config,
            engine,
            compiled,
            instance: Mutex::new(None),
            enforcer,
            meter,
            rate_limiter,
            metrics,
            created_at: start,
            hooks: Arc::new(NoOpHooks),
            execution_log: Arc::new(std::sync::Mutex::new(ExecutionLog::new())),
        })
    }

    /// Set lifecycle hooks for this sandbox.
    ///
    /// Hooks receive notifications for lifecycle events (creation, run
    /// start/complete/error, termination). Call this after `create()`.
    pub fn set_hooks(&mut self, hooks: Arc<dyn SandboxHooks>) {
        self.hooks = hooks;
    }

    /// Get the sandbox ID.
    pub fn id(&self) -> SandboxId {
        self.id
    }

    /// Get the current state.
    pub fn state(&self) -> SandboxState {
        self.state
    }

    /// Get the module hash.
    pub fn module_hash(&self) -> &ModuleHash {
        self.compiled.hash()
    }

    /// Get the configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Get user-defined metadata for this sandbox.
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.config.metadata
    }

    /// Get a specific metadata value by key.
    pub fn label(&self, key: &str) -> Option<&str> {
        self.config.metadata.get(key).map(String::as_str)
    }

    /// Get the capability enforcer.
    pub fn enforcer(&self) -> &CapabilityEnforcer {
        &self.enforcer
    }

    /// Get the resource meter.
    pub fn meter(&self) -> &ResourceMeter {
        &self.meter
    }

    /// Get the metrics.
    pub fn metrics(&self) -> &SandboxMetrics {
        &self.metrics
    }

    /// Get how long the sandbox has existed.
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get the execution lineage log.
    ///
    /// Contains provenance records for every completed `run()` or
    /// `run_streaming()` call, including input/output hashes, timing,
    /// and resource usage.
    pub fn execution_traces(&self) -> Vec<ExecutionTrace> {
        self.execution_log.lock().unwrap_or_else(|e| e.into_inner()).traces().to_vec()
    }

    /// Run the sandbox with optional stdin input.
    ///
    /// Executes the WASM module's `_start` function (WASI entry point) with the
    /// configured capabilities and resource limits. The sandbox transitions to the
    /// `Terminated` state after execution completes, regardless of success or failure.
    ///
    /// # Arguments
    ///
    /// * `input` - Bytes to provide on the sandbox's stdin. Pass `&[]` for no input.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The sandbox is not in the [`SandboxState::Ready`] state
    /// - Rate limiting rejects the execution attempt
    /// - WASM execution fails (trap, out of fuel, epoch timeout, etc.)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
    /// # async fn example() -> isolate_core::Result<()> {
    /// let wasm = std::fs::read("module.wasm")?;
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .fuel(1_000_000)
    ///     .capability(Capability::stdout())
    ///     .build()?;
    ///
    /// let mut sandbox = Sandbox::create(config).await?;
    /// let output = sandbox.run(b"hello").await?;
    /// assert_eq!(output.exit_code, 0);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Runnable example with an inline minimal WASM module:
    ///
    /// ```rust
    /// use isolate_core::{Sandbox, SandboxConfig};
    ///
    /// // Minimal WASI module: imports proc_exit, exports _start which calls proc_exit(0)
    /// const WASM: &[u8] = &[
    ///     0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02,
    ///     0x60, 0x01, 0x7f, 0x00, 0x60, 0x00, 0x00, 0x02, 0x24, 0x01, 0x16,
    ///     0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61, 0x70, 0x73, 0x68,
    ///     0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65, 0x77, 0x31,
    ///     0x09, 0x70, 0x72, 0x6f, 0x63, 0x5f, 0x65, 0x78, 0x69, 0x74, 0x00,
    ///     0x00, 0x03, 0x02, 0x01, 0x01, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07,
    ///     0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
    ///     0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01, 0x0a, 0x08,
    ///     0x01, 0x06, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
    /// ];
    ///
    /// # fn main() -> isolate_core::Result<()> {
    /// let rt = tokio::runtime::Runtime::new().unwrap();
    /// rt.block_on(async {
    ///     let config = SandboxConfig::builder()
    ///         .module(WASM)?
    ///         .fuel(1_000_000)
    ///         .build()?;
    ///     let mut sandbox = Sandbox::create(config).await?;
    ///     let output = sandbox.run(&[]).await?;
    ///     assert_eq!(output.exit_code, 0);
    ///     assert!(output.success());
    ///     Ok(())
    /// })
    /// # }
    /// ```
    pub async fn run(&mut self, input: &[u8]) -> Result<Output> {
        self.ensure_state(SandboxState::Ready)?;

        // Enforce rate limit before execution
        if let Some(ref limiter) = self.rate_limiter {
            limiter.try_acquire()?;
        }

        self.state = SandboxState::Running;
        self.hooks.on_event(&SandboxEvent::RunStarted { sandbox_id: self.id });

        let start = Instant::now();
        let started_at = SystemTime::now();
        self.metrics.record_run_start();

        tracing::debug!(sandbox_id = %self.id, "Starting sandbox execution");

        // Create a new instance with input if provided
        let input_data = if input.is_empty() { None } else { Some(input.to_vec()) };

        let mut instance = self.engine.instantiate_with_input(
            &self.compiled,
            &self.config,
            self.enforcer.clone(),
            self.meter.clone(),
            input_data,
        )?;

        // Set up epoch-based timeout if wall time limit is configured.
        // The global epoch ticker increments every 10ms; we just set the deadline.
        const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(10);
        if let Some(timeout) = self.config.resources.time.wall_time {
            let epochs_until_timeout =
                (timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()).max(1) as u64;
            instance.set_epoch_deadline(epochs_until_timeout);
            self.engine.ensure_epoch_ticker();
        }

        // Run the WASM instance
        let result = tokio::task::spawn_blocking(move || instance.run())
            .await
            .map_err(|e| Error::Execution(e.to_string()))?;

        let duration = start.elapsed();
        self.state = SandboxState::Terminated;

        match result {
            Ok(exec_result) => {
                self.metrics.record_run_complete(duration, true);

                // Record fuel consumption in the meter
                if let Some(fuel) = exec_result.fuel_consumed {
                    let _ = self.meter.record_fuel(fuel);
                }

                // Record execution and bandwidth for rate limiting
                if let Some(ref limiter) = self.rate_limiter {
                    limiter.record_execution();
                    let total_bytes =
                        exec_result.stdout.len() as u64 + exec_result.stderr.len() as u64;
                    let _ = limiter.record_bandwidth(total_bytes);
                }

                tracing::info!(
                    sandbox_id = %self.id,
                    exit_code = exec_result.exit_code,
                    duration_ms = duration.as_secs_f64() * 1000.0,
                    fuel_consumed = ?exec_result.fuel_consumed,
                    "Sandbox execution completed"
                );

                let output = Output {
                    exit_code: exec_result.exit_code,
                    stdout: exec_result.stdout,
                    stderr: exec_result.stderr,
                    duration,
                    resource_usage: self.meter.usage(),
                };

                self.hooks.on_event(&SandboxEvent::RunCompleted {
                    sandbox_id: self.id,
                    output: output.clone(),
                });

                // Record execution lineage
                if let Ok(mut log) = self.execution_log.lock() {
                    log.record(ExecutionTrace::new(
                        self.id,
                        self.compiled.hash().clone(),
                        input,
                        &output.stdout,
                        &output.stderr,
                        output.exit_code,
                        started_at,
                        duration,
                        output.resource_usage.clone(),
                    ));
                } else {
                    tracing::warn!(
                        sandbox_id = %self.id,
                        "Failed to record execution lineage: lock poisoned"
                    );
                }

                Ok(output)
            }
            Err(e) => {
                self.metrics.record_run_complete(duration, false);
                self.hooks.on_event(&SandboxEvent::RunFailed {
                    sandbox_id: self.id,
                    error: e.to_string(),
                });

                tracing::warn!(
                    sandbox_id = %self.id,
                    error = %e,
                    duration_ms = duration.as_secs_f64() * 1000.0,
                    "Sandbox execution failed"
                );

                Err(e)
            }
        }
    }

    /// Run a specific exported function by name.
    ///
    /// Calls a named export from the WASM module with the given arguments. Unlike
    /// [`run()`](Self::run), this method does **not** terminate the sandbox—it
    /// transitions back to [`SandboxState::Ready`] after the call completes,
    /// allowing multiple sequential calls.
    ///
    /// # Arguments
    ///
    /// * `function` - Name of the exported WASM function to call.
    /// * `args` - Arguments to pass to the function as [`wasmtime::Val`] values.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The sandbox is not in the [`SandboxState::Ready`] state
    /// - The named function does not exist in the module's exports
    /// - The function traps or runs out of fuel during execution
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::{Sandbox, SandboxConfig};
    /// # async fn example() -> isolate_core::Result<()> {
    /// let wasm = std::fs::read("module.wasm")?;
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .fuel(1_000_000)
    ///     .build()?;
    ///
    /// let mut sandbox = Sandbox::create(config).await?;
    /// let results = sandbox.call("add", &[
    ///     wasmtime::Val::I32(2),
    ///     wasmtime::Val::I32(3),
    /// ]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call(
        &mut self,
        function: &str,
        args: &[wasmtime::Val],
    ) -> Result<Vec<wasmtime::Val>> {
        self.ensure_state(SandboxState::Ready)?;

        // Enforce rate limit before execution
        if let Some(ref limiter) = self.rate_limiter {
            limiter.try_acquire()?;
        }

        self.state = SandboxState::Running;

        let start = Instant::now();

        // Create instance if needed
        let mut instance_guard = self.instance.lock().await;
        if instance_guard.is_none() {
            *instance_guard = Some(self.engine.instantiate(
                &self.compiled,
                &self.config,
                self.enforcer.clone(),
                self.meter.clone(),
            )?);
        }

        let instance = instance_guard.as_mut().ok_or_else(|| Error::InvalidState {
            expected: "instance initialized".to_string(),
            actual: "instance is None after initialization".to_string(),
        })?;

        // Set up epoch-based timeout for wall time limit
        const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(10);
        if let Some(timeout) = self.config.resources.time.wall_time {
            let epochs_until_timeout =
                (timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()).max(1) as u64;
            instance.set_epoch_deadline(epochs_until_timeout);
            self.engine.ensure_epoch_ticker();
        }

        let result = instance.call(function, args);

        let duration = start.elapsed();
        self.state = SandboxState::Ready;

        // Record execution for rate limiting
        if let Some(ref limiter) = self.rate_limiter {
            limiter.record_execution();
        }

        tracing::debug!(
            sandbox_id = %self.id,
            function = function,
            duration_ms = duration.as_secs_f64() * 1000.0,
            success = result.is_ok(),
            "Function call completed"
        );

        result
    }

    /// Terminate the sandbox and release its resources.
    ///
    /// Moves the sandbox to [`SandboxState::Terminated`] and drops the internal
    /// WASM instance. Returns the collected [`SandboxMetrics`] for the sandbox's
    /// lifetime. This method is idempotent—calling it on an already-terminated
    /// sandbox is safe.
    ///
    /// # Errors
    ///
    /// This method currently does not return errors, but returns `Result` for
    /// forward compatibility with async cleanup that may fail.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::{Sandbox, SandboxConfig};
    /// # async fn example() -> isolate_core::Result<()> {
    /// let wasm = std::fs::read("module.wasm")?;
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .build()?;
    ///
    /// let mut sandbox = Sandbox::create(config).await?;
    /// let metrics = sandbox.terminate().await?;
    /// // Sandbox resources are now released
    /// # Ok(())
    /// # }
    /// ```
    pub async fn terminate(&mut self) -> Result<SandboxMetrics> {
        if self.state == SandboxState::Running {
            return Err(Error::InvalidState {
                expected: "ready or terminated".to_string(),
                actual: "running (wait for execution to finish before terminating)".to_string(),
            });
        }

        tracing::info!(sandbox_id = %self.id, "Terminating sandbox");

        self.state = SandboxState::Terminated;

        // Drop the instance
        *self.instance.lock().await = None;

        self.hooks.on_event(&SandboxEvent::Terminated {
            sandbox_id: self.id,
            metrics: self.metrics.clone(),
        });

        Ok(self.metrics.clone())
    }

    /// Run the sandbox with real-time streaming output.
    ///
    /// Returns a receiver that yields [`OutputChunk`](crate::engine::OutputChunk)s as they are produced,
    /// plus a join handle that resolves to the final [`Output`].
    ///
    /// Unlike [`run()`](Self::run), which buffers all output until completion,
    /// this method streams stdout/stderr chunks as they are written by the
    /// WASM module. The sandbox transitions to `Terminated` immediately after
    /// the background task is spawned.
    ///
    /// # Arguments
    ///
    /// * `input` - Bytes to provide on the sandbox's stdin. Pass `&[]` for no input.
    /// * `buffer_size` - Channel capacity controlling back-pressure. A larger
    ///   buffer reduces the chance of the producer blocking, but uses more
    ///   memory. Clamped to a minimum of 1.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The sandbox is not in the [`SandboxState::Ready`] state
    /// - Rate limiting rejects the execution attempt
    /// - The streaming WASM instance cannot be created
    ///
    /// The returned `JoinHandle` may also resolve to an error if WASM execution
    /// fails (trap, out of fuel, epoch timeout, etc.).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
    /// # async fn example() -> isolate_core::Result<()> {
    /// let wasm = std::fs::read("module.wasm")?;
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .fuel(1_000_000)
    ///     .capability(Capability::stdout())
    ///     .build()?;
    ///
    /// let mut sandbox = Sandbox::create(config).await?;
    /// let (mut rx, handle) = sandbox.run_streaming(&[], 32).await?;
    ///
    /// // Consume chunks as they arrive
    /// while let Some(chunk) = rx.recv().await {
    ///     println!("chunk: {:?}", chunk);
    /// }
    ///
    /// // Wait for final result
    /// let output = handle.await.expect("task panicked")?;
    /// assert_eq!(output.exit_code, 0);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run_streaming(
        &mut self,
        input: &[u8],
        buffer_size: usize,
    ) -> Result<(
        tokio::sync::mpsc::Receiver<crate::engine::OutputChunk>,
        tokio::task::JoinHandle<Result<Output>>,
    )> {
        use crate::engine::OutputChunk;

        self.ensure_state(SandboxState::Ready)?;

        if let Some(ref limiter) = self.rate_limiter {
            limiter.try_acquire()?;
        }

        self.state = SandboxState::Running;
        self.hooks.on_event(&SandboxEvent::RunStarted { sandbox_id: self.id });

        let (tx, rx) = tokio::sync::mpsc::channel::<OutputChunk>(buffer_size.max(1));
        let sender = std::sync::Arc::new(tx);

        let input_data = if input.is_empty() { None } else { Some(input.to_vec()) };

        let mut instance = self.engine.instantiate_streaming(
            &self.compiled,
            &self.config,
            self.enforcer.clone(),
            self.meter.clone(),
            input_data,
            sender,
        )?;

        const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(10);
        if let Some(timeout) = self.config.resources.time.wall_time {
            let epochs_until_timeout =
                (timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()).max(1) as u64;
            instance.set_epoch_deadline(epochs_until_timeout);
            self.engine.ensure_epoch_ticker();
        }

        let meter = self.meter.clone();
        let mut metrics = self.metrics.clone();
        let id = self.id;
        let rate_limiter = self.rate_limiter.clone();
        let hooks = self.hooks.clone();
        let module_hash = self.compiled.hash().clone();
        let input_bytes = input.to_vec();
        let execution_log = self.execution_log.clone();

        let join = tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            let started_at = SystemTime::now();
            metrics.record_run_start();

            let result = instance.run();

            let duration = start.elapsed();

            match result {
                Ok(exec_result) => {
                    metrics.record_run_complete(duration, true);
                    if let Some(fuel) = exec_result.fuel_consumed {
                        let _ = meter.record_fuel(fuel);
                    }
                    if let Some(ref limiter) = rate_limiter {
                        limiter.record_execution();
                        let total_bytes =
                            exec_result.stdout.len() as u64 + exec_result.stderr.len() as u64;
                        let _ = limiter.record_bandwidth(total_bytes);
                    }
                    tracing::info!(sandbox_id = %id, exit_code = exec_result.exit_code, "Streaming execution completed");
                    let output = Output {
                        exit_code: exec_result.exit_code,
                        stdout: exec_result.stdout,
                        stderr: exec_result.stderr,
                        duration,
                        resource_usage: meter.usage(),
                    };

                    hooks.on_event(&SandboxEvent::RunCompleted {
                        sandbox_id: id,
                        output: output.clone(),
                    });

                    // Record execution lineage
                    if let Ok(mut log) = execution_log.lock() {
                        log.record(ExecutionTrace::new(
                            id,
                            module_hash,
                            &input_bytes,
                            &output.stdout,
                            &output.stderr,
                            output.exit_code,
                            started_at,
                            duration,
                            output.resource_usage.clone(),
                        ));
                    } else {
                        tracing::warn!(
                            sandbox_id = %id,
                            "Failed to record streaming execution lineage: lock poisoned"
                        );
                    }

                    Ok(output)
                }
                Err(e) => {
                    metrics.record_run_complete(duration, false);
                    hooks.on_event(&SandboxEvent::RunFailed {
                        sandbox_id: id,
                        error: e.to_string(),
                    });
                    Err(e)
                }
            }
        });

        // Mark as terminated once the task is spawned (caller owns the handle)
        self.state = SandboxState::Terminated;

        Ok((rx, join))
    }

    /// Ensure the sandbox is in the expected state.
    fn ensure_state(&self, expected: SandboxState) -> Result<()> {
        if self.state != expected {
            return Err(Error::InvalidState {
                expected: expected.to_string(),
                actual: self.state.to_string(),
            });
        }
        Ok(())
    }

    /// Reset a terminated sandbox back to Ready state without recompilation.
    ///
    /// This reuses the cached compiled module and resets the WASI state,
    /// fuel counters, and resource meters. Much faster than creating a
    /// new sandbox from scratch.
    ///
    /// # Errors
    ///
    /// Returns an error if the sandbox is not in the `Terminated` state.
    pub async fn reset(&mut self) -> Result<()> {
        self.ensure_state(SandboxState::Terminated)?;

        // Drop existing instance
        *self.instance.lock().await = None;

        // Reset resource meter
        self.meter = ResourceMeter::new(self.config.resources.clone());

        // Reset rate limiter: rebuild if enabled, drop if disabled
        self.rate_limiter = if self.config.rate_limit.is_enabled() {
            Some(SharedRateLimiter::new(self.config.rate_limit.clone()))
        } else {
            None
        };

        self.state = SandboxState::Ready;

        tracing::debug!(sandbox_id = %self.id, "Sandbox reset to Ready");
        Ok(())
    }

    /// Run the sandbox with multiple inputs in parallel.
    ///
    /// Executes the same WASM module with each input concurrently using
    /// separate sandbox instances that share the same compiled module.
    /// Results are returned in the same order as the inputs.
    ///
    /// Each execution gets its own resource metering and capability
    /// enforcement. The original sandbox is terminated after batch
    /// completion.
    ///
    /// # Errors
    ///
    /// Returns a Vec of Results — individual executions may fail
    /// independently. The method itself returns an error only if the
    /// sandbox is not in the Ready state.
    pub async fn run_batch(&mut self, inputs: Vec<Vec<u8>>) -> Result<Vec<Result<Output>>> {
        self.ensure_state(SandboxState::Ready)?;

        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        self.state = SandboxState::Running;

        let mut handles = Vec::with_capacity(inputs.len());

        for input in inputs {
            let engine = self.engine.clone();
            let compiled = self.compiled.clone();
            let config = self.config.clone();
            let enforcer = CapabilityEnforcer::new(config.capabilities.clone(), self.id.0);
            let meter = ResourceMeter::new(config.resources.clone());

            let handle = tokio::spawn(async move {
                let start = Instant::now();
                let input_data = if input.is_empty() { None } else { Some(input) };

                let mut instance = engine.instantiate_with_input(
                    &compiled,
                    &config,
                    enforcer,
                    meter.clone(),
                    input_data,
                )?;

                let result = tokio::task::spawn_blocking(move || instance.run())
                    .await
                    .map_err(|e| Error::Execution(e.to_string()))?;

                let duration = start.elapsed();

                match result {
                    Ok(exec_result) => Ok(Output {
                        exit_code: exec_result.exit_code,
                        stdout: exec_result.stdout,
                        stderr: exec_result.stderr,
                        duration,
                        resource_usage: meter.usage(),
                    }),
                    Err(e) => Err(e),
                }
            });

            handles.push(handle);
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) if e.is_panic() => {
                    results.push(Err(Error::Execution(format!("sandbox task panicked: {}", e))));
                }
                Err(e) => {
                    results.push(Err(Error::Execution(format!("sandbox task failed: {}", e))));
                }
            }
        }

        self.state = SandboxState::Terminated;
        Ok(results)
    }
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("module_hash", &self.compiled.hash())
            .field("age", &self.age())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    // Minimal WASM module with _start function
    // This is a valid WASM module that just returns
    const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");

    // Use a minimal valid WASM for basic tests
    #[allow(dead_code)] // Reserved for future unit tests requiring raw WASM bytes
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ];

    #[test]
    fn test_sandbox_id() {
        let id1 = SandboxId::new();
        let id2 = SandboxId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_sandbox_state_display() {
        assert_eq!(SandboxState::Ready.to_string(), "ready");
        assert_eq!(SandboxState::Running.to_string(), "running");
        assert_eq!(SandboxState::Terminated.to_string(), "terminated");
    }

    #[test]
    fn test_output_helpers() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello".to_vec(),
            stderr: b"error".to_vec(),
            duration: Duration::from_millis(100),
            resource_usage: ResourceUsage::default(),
        };

        assert!(output.success());
        assert_eq!(output.stdout_str(), "hello");
        assert_eq!(output.stderr_str(), "error");
    }

    #[tokio::test]
    async fn test_sandbox_create() {
        // Skip if fixture doesn't exist
        if HELLO_WASM.len() < 8 {
            return;
        }

        let config = SandboxConfig::builder()
            .module(HELLO_WASM)
            .unwrap()
            .capability(Capability::stdout())
            .build()
            .unwrap();

        let sandbox = Sandbox::create(config).await.unwrap();

        assert_eq!(sandbox.state(), SandboxState::Ready);
    }

    #[test]
    fn test_structured_stdout_json_lines() {
        let output = Output {
            exit_code: 0,
            stdout: b"{\"level\":\"info\",\"msg\":\"hello\"}\nplain text\n{\"level\":\"error\",\"msg\":\"fail\"}\n".to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            resource_usage: ResourceUsage::default(),
        };

        let structured = output.structured_stdout();
        assert_eq!(structured.len(), 2);
        assert_eq!(structured[0]["level"], "info");
        assert_eq!(structured[1]["level"], "error");
    }

    #[test]
    fn test_structured_stdout_empty() {
        let output = Output {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            resource_usage: ResourceUsage::default(),
        };
        assert!(output.structured_stdout().is_empty());
    }

    #[test]
    fn test_structured_stdout_no_json() {
        let output = Output {
            exit_code: 0,
            stdout: b"just plain text\nanother line\n".to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            resource_usage: ResourceUsage::default(),
        };
        assert!(output.structured_stdout().is_empty());
    }

    #[test]
    fn test_stdout_lines() {
        let output = Output {
            exit_code: 0,
            stdout: b"line1\nline2\nline3".to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.stdout_lines(), vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn test_output_failure() {
        let output = Output {
            exit_code: 42,
            stdout: Vec::new(),
            stderr: b"something went wrong".to_vec(),
            duration: Duration::from_millis(1),
            resource_usage: ResourceUsage::default(),
        };
        assert!(!output.success());
        assert_eq!(output.exit_code, 42);
    }

    #[test]
    fn test_sandbox_event_variants() {
        let id = SandboxId::new();

        // Verify all event variants can be constructed
        let _created =
            SandboxEvent::Created { sandbox_id: id, cold_start: Duration::from_millis(5) };
        let _started = SandboxEvent::RunStarted { sandbox_id: id };
        let _completed = SandboxEvent::RunCompleted {
            sandbox_id: id,
            output: Output {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                duration: Duration::from_millis(10),
                resource_usage: ResourceUsage::default(),
            },
        };
        let _failed = SandboxEvent::RunFailed { sandbox_id: id, error: "test error".to_string() };
        let _terminated =
            SandboxEvent::Terminated { sandbox_id: id, metrics: SandboxMetrics::new(id) };
    }

    #[test]
    fn test_hooks_trait_noop() {
        // NoOpHooks should not panic on any event
        struct TestHooks;
        impl SandboxHooks for TestHooks {
            fn on_event(&self, _event: &SandboxEvent) {}
        }

        let hooks = TestHooks;
        hooks.on_event(&SandboxEvent::RunStarted { sandbox_id: SandboxId::new() });
    }

    use std::sync::atomic::AtomicU32;

    #[test]
    fn test_hooks_collect_events() {
        let counter = Arc::new(AtomicU32::new(0));

        struct CountingHooks(Arc<AtomicU32>);
        impl SandboxHooks for CountingHooks {
            fn on_event(&self, _event: &SandboxEvent) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let hooks: Arc<dyn SandboxHooks> = Arc::new(CountingHooks(counter.clone()));
        hooks.on_event(&SandboxEvent::RunStarted { sandbox_id: SandboxId::new() });
        hooks.on_event(&SandboxEvent::RunStarted { sandbox_id: SandboxId::new() });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn test_parse_stdout_as() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Res {
            value: i32,
        }

        let output = Output {
            exit_code: 0,
            stdout: br#"{"value": 42}"#.to_vec(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };

        let parsed: Res = output.parse_stdout_as().unwrap();
        assert_eq!(parsed.value, 42);
    }

    #[test]
    fn test_parse_stdout_as_invalid() {
        let output = Output {
            exit_code: 0,
            stdout: b"not json".to_vec(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };

        let result: std::result::Result<serde_json::Value, _> = output.parse_stdout_as();
        assert!(result.is_err());
    }

    #[test]
    fn test_combined_output() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello ".to_vec(),
            stderr: b"world".to_vec(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.combined_output(), b"hello world");
        assert_eq!(output.combined_output_str(), "hello world");
    }

    #[test]
    fn test_combined_output_empty() {
        let output = Output {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert!(output.combined_output().is_empty());
        assert_eq!(output.combined_output_str(), "");
    }

    #[test]
    fn test_stdout_stderr_size() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello".to_vec(),
            stderr: b"err".to_vec(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.stdout_size(), 5);
        assert_eq!(output.stderr_size(), 3);
    }

    #[test]
    fn test_truncated_stdout_within_limit() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello".to_vec(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.truncated_stdout(100), "hello");
    }

    #[test]
    fn test_truncated_stdout_truncates() {
        let output = Output {
            exit_code: 0,
            stdout: b"Hello, World!".to_vec(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.truncated_stdout(5), "Hello...");
    }

    #[test]
    fn test_truncated_stderr_truncates() {
        let output = Output {
            exit_code: 1,
            stdout: Vec::new(),
            stderr: b"Error: something went wrong here".to_vec(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.truncated_stderr(5), "Error...");
    }

    #[test]
    fn test_truncated_empty() {
        let output = Output {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::ZERO,
            resource_usage: ResourceUsage::default(),
        };
        assert_eq!(output.truncated_stdout(10), "");
    }

    #[test]
    fn test_summary() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello".to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(42),
            resource_usage: ResourceUsage::default(),
        };
        let summary = output.summary();
        assert!(summary.contains("exit=0"));
        assert!(summary.contains("stdout=5 B"));
        assert!(summary.contains("stderr=0 B"));
    }

    #[test]
    fn test_summary_with_failure() {
        let output = Output {
            exit_code: 1,
            stdout: Vec::new(),
            stderr: b"error msg".to_vec(),
            duration: Duration::from_secs(2),
            resource_usage: ResourceUsage {
                fuel_consumed: 100_000,
                peak_memory: 1024 * 1024,
                ..Default::default()
            },
        };
        let summary = output.summary();
        assert!(summary.contains("exit=1"));
        assert!(summary.contains("fuel=100000"));
    }

    #[test]
    fn test_execution_summary_success() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello".to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(42),
            resource_usage: ResourceUsage {
                fuel_consumed: 5000,
                peak_memory: 1024 * 1024,
                ..Default::default()
            },
        };

        let summary = output.execution_summary();
        assert!(summary.success);
        assert_eq!(summary.exit_code, 0);
        assert_eq!(summary.duration, Duration::from_millis(42));
        assert_eq!(summary.fuel_consumed, 5000);
        assert_eq!(summary.peak_memory, 1024 * 1024);
        assert_eq!(summary.stdout_bytes, 5);
        assert_eq!(summary.stderr_bytes, 0);
    }

    #[test]
    fn test_execution_summary_failure() {
        let output = Output {
            exit_code: 1,
            stdout: Vec::new(),
            stderr: b"error".to_vec(),
            duration: Duration::from_secs(1),
            resource_usage: ResourceUsage::default(),
        };

        let summary = output.execution_summary();
        assert!(!summary.success);
        assert_eq!(summary.exit_code, 1);
        assert_eq!(summary.stderr_bytes, 5);
    }

    #[test]
    fn test_execution_summary_display() {
        let output = Output {
            exit_code: 0,
            stdout: b"hi".to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(100),
            resource_usage: ResourceUsage {
                fuel_consumed: 10_000,
                peak_memory: 2 * 1024 * 1024,
                ..Default::default()
            },
        };

        let summary = output.execution_summary();
        let display = format!("{}", summary);
        assert!(display.contains("[OK]"));
        assert!(display.contains("exit=0"));
        assert!(display.contains("fuel=10000"));
        assert!(display.contains("2.0 MB"));
    }

    #[test]
    fn test_execution_summary_display_fail() {
        let output = Output {
            exit_code: 42,
            stdout: Vec::new(),
            stderr: b"crashed".to_vec(),
            duration: Duration::from_secs(5),
            resource_usage: ResourceUsage::default(),
        };

        let summary = output.execution_summary();
        let display = format!("{}", summary);
        assert!(display.contains("[FAIL]"));
        assert!(display.contains("exit=42"));
    }
}
