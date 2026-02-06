//! Python bindings for Isolate sandbox runtime.
//!
//! This module provides Python bindings for the Isolate secure sandbox runtime,
//! allowing Python developers to execute untrusted WebAssembly code safely.
//!
//! # Example
//!
//! ```python
//! import isolate
//!
//! # Create a sandbox configuration
//! config = isolate.SandboxConfig.builder() \
//!     .module_from_file("module.wasm") \
//!     .memory_limit(128 * 1024 * 1024) \
//!     .fuel(1_000_000) \
//!     .capability(isolate.Capability.stdout()) \
//!     .capability(isolate.Capability.stderr()) \
//!     .build()
//!
//! # Create and run the sandbox
//! sandbox = isolate.Sandbox.create(config)
//! output = sandbox.run()  # Optionally pass input bytes
//!
//! print(f"Exit code: {output.exit_code}")
//! print(f"Stdout: {output.stdout_str()}")
//! ```

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

// =============================================================================
// Capability
// =============================================================================

/// Capability granting specific permissions to a sandbox.
#[pyclass(name = "Capability")]
#[derive(Clone)]
pub struct PyCapability {
    inner: isolate_core::capability::Capability,
}

#[pymethods]
impl PyCapability {
    /// Grant stdout access.
    #[staticmethod]
    fn stdout() -> Self {
        Self { inner: isolate_core::capability::Capability::stdout() }
    }

    /// Grant stderr access.
    #[staticmethod]
    fn stderr() -> Self {
        Self { inner: isolate_core::capability::Capability::stderr() }
    }

    /// Grant stdin access.
    #[staticmethod]
    fn stdin() -> Self {
        Self { inner: isolate_core::capability::Capability::stdin() }
    }

    /// Grant read access to a filesystem path.
    #[staticmethod]
    fn filesystem_read(path: &str) -> Self {
        Self { inner: isolate_core::capability::Capability::filesystem_read(path) }
    }

    /// Grant write access to a filesystem path.
    #[staticmethod]
    fn filesystem_write(path: &str) -> Self {
        Self { inner: isolate_core::capability::Capability::filesystem_write(path) }
    }

    /// Grant access to all environment variables.
    #[staticmethod]
    fn env_all() -> Self {
        Self { inner: isolate_core::capability::Capability::env_all() }
    }

    /// Grant access to a specific environment variable.
    #[staticmethod]
    fn env_var(name: &str) -> Self {
        Self { inner: isolate_core::capability::Capability::env_var(name) }
    }

    /// Grant system clock access.
    #[staticmethod]
    fn system_clock() -> Self {
        Self { inner: isolate_core::capability::Capability::system_clock() }
    }

    /// Grant monotonic clock access.
    #[staticmethod]
    fn monotonic_clock() -> Self {
        Self { inner: isolate_core::capability::Capability::monotonic_clock() }
    }

    /// Grant timer access (sleep, intervals).
    #[staticmethod]
    fn timers() -> Self {
        Self { inner: isolate_core::capability::Capability::timers() }
    }

    /// Grant secure random number generation access.
    #[staticmethod]
    fn secure_random() -> Self {
        Self { inner: isolate_core::capability::Capability::secure_random() }
    }

    /// Grant seeded (deterministic) random number generation.
    #[staticmethod]
    fn seeded_random(seed: u64) -> Self {
        Self { inner: isolate_core::capability::Capability::seeded_random(seed) }
    }

    /// Grant HTTP client access to specific hosts.
    #[staticmethod]
    fn http_client(hosts: Vec<String>) -> Self {
        Self { inner: isolate_core::capability::Capability::http_client(hosts) }
    }

    /// Grant temporary directory access.
    #[staticmethod]
    fn temp_dir() -> Self {
        Self { inner: isolate_core::capability::Capability::temp_dir() }
    }

    fn __repr__(&self) -> String {
        format!("Capability({:?})", self.inner)
    }
}

// =============================================================================
// SandboxConfig
// =============================================================================

/// Builder for sandbox configuration.
#[pyclass(name = "SandboxConfigBuilder")]
#[derive(Clone)]
pub struct PySandboxConfigBuilder {
    wasm_bytes: Option<Vec<u8>>,
    memory_limit: Option<usize>,
    fuel: Option<u64>,
    cpu_time_limit: Option<Duration>,
    capabilities: Vec<isolate_core::capability::Capability>,
    env_vars: HashMap<String, String>,
    args: Vec<String>,
}

#[pymethods]
impl PySandboxConfigBuilder {
    /// Set the WASM module from bytes.
    fn module(&self, wasm_bytes: Vec<u8>) -> Self {
        let mut new = self.clone();
        new.wasm_bytes = Some(wasm_bytes);
        new
    }

    /// Set the WASM module from a file path.
    fn module_from_file(&self, path: &str) -> PyResult<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to read WASM file: {}", e)))?;
        let mut new = self.clone();
        new.wasm_bytes = Some(bytes);
        Ok(new)
    }

    /// Set the memory limit in bytes.
    fn memory_limit(&self, bytes: usize) -> Self {
        let mut new = self.clone();
        new.memory_limit = Some(bytes);
        new
    }

    /// Set the fuel limit (instruction count).
    fn fuel(&self, amount: u64) -> Self {
        let mut new = self.clone();
        new.fuel = Some(amount);
        new
    }

    /// Set the CPU time limit in seconds.
    fn cpu_time_limit(&self, seconds: f64) -> Self {
        let mut new = self.clone();
        new.cpu_time_limit = Some(Duration::from_secs_f64(seconds));
        new
    }

    /// Add a capability.
    fn capability(&self, cap: PyCapability) -> Self {
        let mut new = self.clone();
        new.capabilities.push(cap.inner);
        new
    }

    /// Set an environment variable.
    fn env(&self, key: &str, value: &str) -> Self {
        let mut new = self.clone();
        new.env_vars.insert(key.to_string(), value.to_string());
        new
    }

    /// Set environment variables from a dictionary.
    fn envs(&self, vars: HashMap<String, String>) -> Self {
        let mut new = self.clone();
        new.env_vars.extend(vars);
        new
    }

    /// Add a command-line argument.
    fn arg(&self, value: &str) -> Self {
        let mut new = self.clone();
        new.args.push(value.to_string());
        new
    }

    /// Set command-line arguments.
    fn args(&self, values: Vec<String>) -> Self {
        let mut new = self.clone();
        new.args = values;
        new
    }

    /// Build the configuration.
    fn build(&self) -> PyResult<PySandboxConfig> {
        let wasm_bytes = self
            .wasm_bytes
            .as_ref()
            .ok_or_else(|| PyValueError::new_err("WASM module is required"))?;

        let mut builder = isolate_core::SandboxConfig::builder()
            .module(wasm_bytes)
            .map_err(|e| PyValueError::new_err(format!("Invalid WASM module: {}", e)))?;

        if let Some(limit) = self.memory_limit {
            builder = builder.memory_limit(limit);
        }

        if let Some(fuel) = self.fuel {
            builder = builder.fuel(fuel);
        }

        if let Some(timeout) = self.cpu_time_limit {
            builder = builder.cpu_time_limit(timeout);
        }

        for cap in &self.capabilities {
            builder = builder.capability(cap.clone());
        }

        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }

        for arg in &self.args {
            builder = builder.arg(arg);
        }

        let config = builder
            .build()
            .map_err(|e| PyValueError::new_err(format!("Invalid configuration: {}", e)))?;

        Ok(PySandboxConfig { inner: config })
    }
}

/// Sandbox configuration.
#[pyclass(name = "SandboxConfig")]
#[derive(Clone)]
pub struct PySandboxConfig {
    inner: isolate_core::SandboxConfig,
}

#[pymethods]
impl PySandboxConfig {
    /// Create a new configuration builder.
    #[staticmethod]
    fn builder() -> PySandboxConfigBuilder {
        PySandboxConfigBuilder {
            wasm_bytes: None,
            memory_limit: None,
            fuel: None,
            cpu_time_limit: None,
            capabilities: Vec::new(),
            env_vars: HashMap::new(),
            args: Vec::new(),
        }
    }

    fn __repr__(&self) -> String {
        "SandboxConfig(...)".to_string()
    }
}

// =============================================================================
// Output
// =============================================================================

/// Output from a sandbox execution.
#[pyclass(name = "Output")]
#[derive(Clone)]
pub struct PyOutput {
    /// Exit code from the sandbox.
    #[pyo3(get)]
    exit_code: i32,
    /// Standard output bytes.
    stdout_bytes: Vec<u8>,
    /// Standard error bytes.
    stderr_bytes: Vec<u8>,
    /// Execution duration in seconds.
    #[pyo3(get)]
    duration_secs: f64,
    /// Fuel consumed.
    #[pyo3(get)]
    fuel_consumed: u64,
}

#[pymethods]
impl PyOutput {
    /// Get stdout as bytes.
    #[getter]
    fn stdout(&self) -> &[u8] {
        &self.stdout_bytes
    }

    /// Get stderr as bytes.
    #[getter]
    fn stderr(&self) -> &[u8] {
        &self.stderr_bytes
    }

    /// Get stdout as a string.
    fn stdout_str(&self) -> PyResult<String> {
        String::from_utf8(self.stdout_bytes.clone())
            .map_err(|e| PyValueError::new_err(format!("Invalid UTF-8 in stdout: {}", e)))
    }

    /// Get stderr as a string.
    fn stderr_str(&self) -> PyResult<String> {
        String::from_utf8(self.stderr_bytes.clone())
            .map_err(|e| PyValueError::new_err(format!("Invalid UTF-8 in stderr: {}", e)))
    }

    /// Check if execution was successful (exit code 0).
    fn is_success(&self) -> bool {
        self.exit_code == 0
    }

    fn __repr__(&self) -> String {
        format!(
            "Output(exit_code={}, stdout_len={}, stderr_len={})",
            self.exit_code,
            self.stdout_bytes.len(),
            self.stderr_bytes.len()
        )
    }
}

// =============================================================================
// Sandbox
// =============================================================================

/// A secure WebAssembly sandbox.
#[pyclass(name = "Sandbox")]
pub struct PySandbox {
    inner: Option<isolate_core::Sandbox>,
    runtime: tokio::runtime::Runtime,
}

#[pymethods]
impl PySandbox {
    /// Create a new sandbox with the given configuration.
    #[staticmethod]
    fn create(config: PySandboxConfig) -> PyResult<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        let sandbox = runtime
            .block_on(async { isolate_core::Sandbox::create(config.inner).await })
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create sandbox: {}", e)))?;

        Ok(Self { inner: Some(sandbox), runtime })
    }

    /// Get the sandbox ID.
    #[getter]
    fn id(&self) -> PyResult<String> {
        let sandbox = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Sandbox has been consumed"))?;
        Ok(sandbox.id().to_string())
    }

    /// Get the sandbox state.
    #[getter]
    fn state(&self) -> PyResult<String> {
        let sandbox = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Sandbox has been consumed"))?;
        Ok(format!("{:?}", sandbox.state()))
    }

    /// Run the sandbox with optional input.
    #[pyo3(signature = (input=None))]
    fn run(&mut self, input: Option<Vec<u8>>) -> PyResult<PyOutput> {
        let mut sandbox = self
            .inner
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("Sandbox has already been run"))?;

        let input_data = input.unwrap_or_default();

        let output = self
            .runtime
            .block_on(async { sandbox.run(&input_data).await })
            .map_err(|e| PyRuntimeError::new_err(format!("Execution failed: {}", e)))?;

        Ok(PyOutput {
            exit_code: output.exit_code,
            stdout_bytes: output.stdout,
            stderr_bytes: output.stderr,
            duration_secs: output.resource_usage.wall_time.as_secs_f64(),
            fuel_consumed: output.resource_usage.fuel_consumed,
        })
    }

    /// Terminate the sandbox.
    fn terminate(&mut self) -> PyResult<()> {
        if let Some(mut sandbox) = self.inner.take() {
            self.runtime
                .block_on(async { sandbox.terminate().await })
                .map_err(|e| PyRuntimeError::new_err(format!("Termination failed: {}", e)))?;
        }
        Ok(())
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            Some(sandbox) => format!("Sandbox(id={}, state={:?})", sandbox.id(), sandbox.state()),
            None => "Sandbox(consumed)".to_string(),
        }
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Run a WASM module with simple configuration.
#[pyfunction]
#[pyo3(signature = (wasm_bytes, memory_limit=None, fuel=None, stdin=None, env=None))]
fn run_wasm(
    wasm_bytes: Vec<u8>,
    memory_limit: Option<usize>,
    fuel: Option<u64>,
    stdin: Option<Vec<u8>>,
    env: Option<HashMap<String, String>>,
) -> PyResult<PyOutput> {
    let builder = PySandboxConfigBuilder {
        wasm_bytes: Some(wasm_bytes),
        memory_limit,
        fuel,
        cpu_time_limit: None,
        capabilities: vec![
            isolate_core::capability::Capability::stdout(),
            isolate_core::capability::Capability::stderr(),
        ],
        env_vars: env.unwrap_or_default(),
        args: Vec::new(),
    };

    let config = builder.build()?;
    let mut sandbox = PySandbox::create(config)?;
    sandbox.run(stdin)
}

/// Run a WASM file with simple configuration.
#[pyfunction]
#[pyo3(signature = (path, memory_limit=None, fuel=None, stdin=None, env=None))]
fn run_wasm_file(
    path: &str,
    memory_limit: Option<usize>,
    fuel: Option<u64>,
    stdin: Option<Vec<u8>>,
    env: Option<HashMap<String, String>>,
) -> PyResult<PyOutput> {
    let wasm_bytes = std::fs::read(path)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to read WASM file: {}", e)))?;
    run_wasm(wasm_bytes, memory_limit, fuel, stdin, env)
}

/// Get the version of the isolate library.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// =============================================================================
// Module Definition
// =============================================================================

/// Isolate: Secure WebAssembly Sandbox Runtime
///
/// This module provides Python bindings for the Isolate sandbox runtime,
/// allowing you to execute untrusted WebAssembly code safely.
///
/// Example:
///     >>> import isolate
///     >>> config = isolate.SandboxConfig.builder() \
///     ...     .module_from_file("hello.wasm") \
///     ...     .capability(isolate.Capability.stdout()) \
///     ...     .build()
///     >>> sandbox = isolate.Sandbox.create(config)
///     >>> output = sandbox.run()
///     >>> print(output.stdout_str())
#[pymodule]
fn _isolate(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCapability>()?;
    m.add_class::<PySandboxConfig>()?;
    m.add_class::<PySandboxConfigBuilder>()?;
    m.add_class::<PyOutput>()?;
    m.add_class::<PySandbox>()?;
    m.add_function(wrap_pyfunction!(run_wasm, m)?)?;
    m.add_function(wrap_pyfunction!(run_wasm_file, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_creation() {
        let _ = PyCapability::stdout();
        let _ = PyCapability::stderr();
        let _ = PyCapability::stdin();
        let _ = PyCapability::filesystem_read("/tmp");
        let _ = PyCapability::filesystem_write("/tmp");
        let _ = PyCapability::env_all();
        let _ = PyCapability::env_var("HOME");
        let _ = PyCapability::system_clock();
        let _ = PyCapability::monotonic_clock();
        let _ = PyCapability::timers();
        let _ = PyCapability::secure_random();
        let _ = PyCapability::seeded_random(42);
        let _ = PyCapability::http_client(vec!["example.com".to_string()]);
        let _ = PyCapability::temp_dir();
    }

    #[test]
    fn test_config_builder_creation() {
        let builder = PySandboxConfig::builder();
        assert!(builder.wasm_bytes.is_none());
    }

    #[test]
    fn test_config_builder_requires_wasm() {
        let builder = PySandboxConfig::builder();
        let result = builder.build();
        assert!(result.is_err());
    }
}
