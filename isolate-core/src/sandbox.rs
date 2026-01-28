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
use crate::metrics::SandboxMetrics;
use crate::ratelimit::SharedRateLimiter;
use crate::resource::{ResourceMeter, ResourceUsage};

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

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
        })
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

        let start = Instant::now();
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

        // Set up epoch-based timeout if wall time limit is configured
        // We tick epochs every EPOCH_TICK_INTERVAL and calculate the deadline accordingly
        const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(10);
        let epoch_ticker_handle = if let Some(timeout) = self.config.resources.time.wall_time {
            // Calculate how many epochs until timeout
            let epochs_until_timeout =
                (timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()).max(1) as u64;
            instance.set_epoch_deadline(epochs_until_timeout);

            // Spawn a background task to increment epochs
            let engine = self.engine.clone();
            let cancel_token = tokio_util::sync::CancellationToken::new();
            let token_clone = cancel_token.clone();

            let handle = tokio::spawn(async move {
                let mut interval = tokio::time::interval(EPOCH_TICK_INTERVAL);
                loop {
                    tokio::select! {
                        _ = token_clone.cancelled() => {
                            break;
                        }
                        _ = interval.tick() => {
                            engine.increment_epoch();
                        }
                    }
                }
            });

            Some((handle, cancel_token))
        } else {
            None
        };

        // Run the WASM instance
        let result = tokio::task::spawn_blocking(move || instance.run())
            .await
            .map_err(|e| Error::Execution(e.to_string()))?;

        // Stop the epoch ticker if it was started
        if let Some((handle, cancel_token)) = epoch_ticker_handle {
            cancel_token.cancel();
            let _ = handle.await;
        }

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
                    let total_bytes = exec_result.stdout.len() as u64
                        + exec_result.stderr.len() as u64;
                    let _ = limiter.record_bandwidth(total_bytes);
                }

                tracing::info!(
                    sandbox_id = %self.id,
                    exit_code = exec_result.exit_code,
                    duration_ms = duration.as_secs_f64() * 1000.0,
                    fuel_consumed = ?exec_result.fuel_consumed,
                    "Sandbox execution completed"
                );

                Ok(Output {
                    exit_code: exec_result.exit_code,
                    stdout: exec_result.stdout,
                    stderr: exec_result.stderr,
                    duration,
                    resource_usage: self.meter.usage(),
                })
            }
            Err(e) => {
                self.metrics.record_run_complete(duration, false);

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

        let instance = instance_guard.as_mut().unwrap();
        let result = instance.call(function, args);

        let duration = start.elapsed();
        self.state = SandboxState::Ready;

        tracing::debug!(
            sandbox_id = %self.id,
            function = function,
            duration_ms = duration.as_secs_f64() * 1000.0,
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
        tracing::info!(sandbox_id = %self.id, "Terminating sandbox");

        self.state = SandboxState::Terminated;

        // Drop the instance
        *self.instance.lock().await = None;

        Ok(self.metrics.clone())
    }

    /// Run the sandbox with real-time streaming output.
    ///
    /// Returns a receiver that yields [`OutputChunk`]s as they are produced,
    /// plus a join handle that resolves to the final [`Output`].
    ///
    /// `buffer_size` controls the channel capacity (back-pressure threshold).
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
        let epoch_ticker_handle = if let Some(timeout) = self.config.resources.time.wall_time {
            let epochs_until_timeout =
                (timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()).max(1) as u64;
            instance.set_epoch_deadline(epochs_until_timeout);

            let engine = self.engine.clone();
            let cancel_token = tokio_util::sync::CancellationToken::new();
            let token_clone = cancel_token.clone();
            let handle = tokio::spawn(async move {
                let mut interval = tokio::time::interval(EPOCH_TICK_INTERVAL);
                loop {
                    tokio::select! {
                        _ = token_clone.cancelled() => break,
                        _ = interval.tick() => engine.increment_epoch(),
                    }
                }
            });
            Some((handle, cancel_token))
        } else {
            None
        };

        let meter = self.meter.clone();
        let mut metrics = self.metrics.clone();
        let id = self.id;
        let rate_limiter = self.rate_limiter.clone();

        let join = tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            metrics.record_run_start();

            let result = instance.run();

            if let Some((handle, cancel_token)) = epoch_ticker_handle {
                cancel_token.cancel();
                // Best-effort wait; the handle will be dropped anyway
                drop(handle);
            }

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
                    Ok(Output {
                        exit_code: exec_result.exit_code,
                        stdout: exec_result.stdout,
                        stderr: exec_result.stderr,
                        duration,
                        resource_usage: meter.usage(),
                    })
                }
                Err(e) => {
                    metrics.record_run_complete(duration, false);
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
}
