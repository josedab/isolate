//! Core sandbox implementation for the embedded SDK.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use wasmtime::{Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

/// Error type for the embedded sandbox.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to create engine: {0}")]
    Engine(String),
    #[error("Failed to compile module: {0}")]
    Compilation(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Execution timed out after {0:?}")]
    Timeout(Duration),
    #[error("Fuel exhausted (limit: {0})")]
    FuelExhausted(u64),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for the embedded sandbox.
pub type Result<T> = std::result::Result<T, Error>;

/// Synchronous sandbox configuration.
#[derive(Clone)]
pub struct SandboxConfig {
    wasm_bytes: Vec<u8>,
    module_hash: String,
    memory_limit: usize,
    fuel: Option<u64>,
    allow_stdout: bool,
    allow_stderr: bool,
    env: HashMap<String, String>,
    args: Vec<String>,
    entry_point: String,
}

impl SandboxConfig {
    /// Create a new sandbox config from WASM bytes.
    pub fn new(wasm_bytes: &[u8]) -> Self {
        let hash = {
            let mut h = Sha256::new();
            h.update(wasm_bytes);
            hex::encode(h.finalize())
        };
        Self {
            wasm_bytes: wasm_bytes.to_vec(),
            module_hash: hash,
            memory_limit: 64 * 1024 * 1024,
            fuel: None,
            allow_stdout: true,
            allow_stderr: true,
            env: HashMap::new(),
            args: Vec::new(),
            entry_point: "_start".to_string(),
        }
    }

    /// Set maximum memory in bytes (default: 64MB).
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = bytes;
        self
    }

    /// Set CPU fuel limit.
    pub fn fuel(mut self, fuel: u64) -> Self {
        self.fuel = Some(fuel);
        self
    }

    /// Allow stdout capture (default: true).
    pub fn allow_stdout(mut self, allow: bool) -> Self {
        self.allow_stdout = allow;
        self
    }

    /// Allow stderr capture (default: true).
    pub fn allow_stderr(mut self, allow: bool) -> Self {
        self.allow_stderr = allow;
        self
    }

    /// Set an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Add a command-line argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set the entry point function name (default: "_start").
    pub fn entry_point(mut self, name: impl Into<String>) -> Self {
        self.entry_point = name.into();
        self
    }
}

/// Output from a sandbox execution.
#[derive(Debug, Clone)]
pub struct Output {
    /// Process exit code.
    pub exit_code: i32,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
    /// Wall time duration.
    pub duration: Duration,
    /// Fuel consumed (if fuel metering was enabled).
    pub fuel_consumed: u64,
}

impl Output {
    /// Get stdout as a UTF-8 string.
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    /// Get stderr as a UTF-8 string.
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }

    /// Check if execution succeeded (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Host state for the WASM store.
struct HostState {
    wasi: WasiP1Ctx,
    limits: StoreLimits,
}

/// A synchronous WASM sandbox.
pub struct Sandbox {
    engine: Engine,
    module: Module,
    config: SandboxConfig,
}

impl Sandbox {
    /// Create a new sandbox from a configuration (synchronous).
    pub fn create(config: SandboxConfig) -> Result<Self> {
        let mut engine_config = wasmtime::Config::new();
        engine_config.async_support(false);
        engine_config.consume_fuel(config.fuel.is_some());
        engine_config.wasm_simd(true);
        engine_config.wasm_bulk_memory(true);

        let engine = Engine::new(&engine_config)
            .map_err(|e| Error::Engine(e.to_string()))?;

        let module = Module::from_binary(&engine, &config.wasm_bytes)
            .map_err(|e| Error::Compilation(e.to_string()))?;

        Ok(Self { engine, module, config })
    }

    /// Run the sandbox with the given input bytes (synchronous).
    pub fn run(&mut self, input: &[u8]) -> Result<Output> {
        let start = Instant::now();

        // Build WASI context
        let mut wasi_builder = WasiCtxBuilder::new();

        // Set stdin from input
        if !input.is_empty() {
            wasi_builder.stdin(wasmtime_wasi::pipe::MemoryInputPipe::new(input.to_vec()));
        }

        // Capture stdout/stderr
        let stdout_pipe = wasmtime_wasi::pipe::MemoryOutputPipe::new(4096);
        let stderr_pipe = wasmtime_wasi::pipe::MemoryOutputPipe::new(4096);

        if self.config.allow_stdout {
            wasi_builder.stdout(stdout_pipe.clone());
        }
        if self.config.allow_stderr {
            wasi_builder.stderr(stderr_pipe.clone());
        }

        // Set env vars
        for (key, value) in &self.config.env {
            wasi_builder.env(key, value);
        }

        // Set args
        let prog = "module.wasm";
        wasi_builder.arg(prog);
        for arg in &self.config.args {
            wasi_builder.arg(arg);
        }

        let wasi = wasi_builder.build_p1();

        // Memory limits
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.config.memory_limit)
            .build();

        let host_state = HostState { wasi, limits };
        let mut store = Store::new(&self.engine, host_state);
        store.limiter(|state| &mut state.limits);

        // Set fuel
        if let Some(fuel) = self.config.fuel {
            store.set_fuel(fuel).map_err(|e| Error::Engine(e.to_string()))?;
        }

        // Link WASI
        let mut linker = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |state: &mut HostState| &mut state.wasi)
            .map_err(|e| Error::Engine(format!("Failed to link WASI: {}", e)))?;

        // Instantiate
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| Error::Execution(format!("Instantiation failed: {}", e)))?;

        // Find entry point
        let entry = instance
            .get_typed_func::<(), ()>(&mut store, &self.config.entry_point)
            .map_err(|e| Error::Execution(format!("Entry point '{}' not found: {}", self.config.entry_point, e)))?;

        // Execute
        let exit_code = match entry.call(&mut store, ()) {
            Ok(()) => 0,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("wasi:snapshot-preview1") && msg.contains("exit") {
                    // Parse exit code from WASI proc_exit trap
                    if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                        exit.0
                    } else {
                        1
                    }
                } else if msg.contains("fuel") {
                    return Err(Error::FuelExhausted(self.config.fuel.unwrap_or(0)));
                } else {
                    return Err(Error::Execution(msg));
                }
            }
        };

        let duration = start.elapsed();
        let fuel_consumed = if self.config.fuel.is_some() {
            let remaining = store.get_fuel().unwrap_or(0);
            self.config.fuel.unwrap_or(0).saturating_sub(remaining)
        } else {
            0
        };

        // Collect output
        let stdout: Vec<u8> = stdout_pipe.try_into_inner().unwrap_or_default().into();
        let stderr: Vec<u8> = stderr_pipe.try_into_inner().unwrap_or_default().into();

        Ok(Output {
            exit_code,
            stdout,
            stderr,
            duration,
            fuel_consumed,
        })
    }

    /// Get the module hash.
    pub fn module_hash(&self) -> &str {
        &self.config.module_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_defaults() {
        let config = SandboxConfig::new(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
        assert_eq!(config.memory_limit, 64 * 1024 * 1024);
        assert!(config.fuel.is_none());
        assert!(config.allow_stdout);
        assert_eq!(config.entry_point, "_start");
    }

    #[test]
    fn test_sandbox_config_builder() {
        let config = SandboxConfig::new(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])
            .memory_limit(128 * 1024 * 1024)
            .fuel(1_000_000)
            .env("KEY", "VALUE")
            .arg("--verbose");

        assert_eq!(config.memory_limit, 128 * 1024 * 1024);
        assert_eq!(config.fuel, Some(1_000_000));
        assert_eq!(config.env.get("KEY"), Some(&"VALUE".to_string()));
        assert_eq!(config.args, vec!["--verbose"]);
    }

    #[test]
    fn test_output_helpers() {
        let output = Output {
            exit_code: 0,
            stdout: b"hello world".to_vec(),
            stderr: b"warning".to_vec(),
            duration: Duration::from_millis(42),
            fuel_consumed: 1000,
        };

        assert!(output.success());
        assert_eq!(output.stdout_str(), "hello world");
        assert_eq!(output.stderr_str(), "warning");
    }

    #[test]
    fn test_sandbox_invalid_wasm() {
        let config = SandboxConfig::new(&[0xFF, 0xFF, 0xFF, 0xFF]);
        let result = Sandbox::create(config);
        assert!(result.is_err());
    }
}
