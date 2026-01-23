//! WASM engine implementation using Wasmtime.

use crate::capability::CapabilityEnforcer;
use crate::config::{ModuleHash, SandboxConfig, WasmModule};
use crate::error::{Error, Result};
use crate::resource::ResourceMeter;

use super::capture::{
    new_capture_buffer, BufferedStdin, CaptureBuffer, CaptureStream, EmptyStdin, NullStream,
};
use super::host::HostState;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use wasmtime::{Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Val};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

/// Configuration for the WASM engine.
#[derive(Debug, Clone)]
pub struct WasmEngineConfig {
    /// Enable fuel-based CPU metering.
    pub enable_fuel: bool,
    /// Enable epoch-based interruption.
    pub enable_epoch_interruption: bool,
    /// Maximum number of cached modules.
    pub max_cached_modules: usize,
}

impl Default for WasmEngineConfig {
    fn default() -> Self {
        Self {
            enable_fuel: true,
            enable_epoch_interruption: true,
            max_cached_modules: 100,
        }
    }
}

/// A compiled WASM module ready for instantiation.
#[derive(Clone)]
pub struct CompiledModule {
    module: Module,
    hash: ModuleHash,
}

impl CompiledModule {
    /// Get the module hash.
    pub fn hash(&self) -> &ModuleHash {
        &self.hash
    }
}

/// The WASM execution engine.
#[derive(Clone)]
pub struct WasmEngine {
    engine: Engine,
    module_cache: Arc<DashMap<ModuleHash, Module>>,
    config: WasmEngineConfig,
}

impl WasmEngine {
    /// Create a new WASM engine with default configuration.
    pub fn new() -> Result<Self> {
        Self::with_config(WasmEngineConfig::default())
    }

    /// Create a new WASM engine with the given configuration.
    pub fn with_config(config: WasmEngineConfig) -> Result<Self> {
        let mut engine_config = wasmtime::Config::new();

        // Enable WASM features
        engine_config.wasm_simd(true);
        engine_config.wasm_bulk_memory(true);

        // Security features
        if config.enable_epoch_interruption {
            engine_config.epoch_interruption(true);
        }
        if config.enable_fuel {
            engine_config.consume_fuel(true);
        }

        // Performance settings
        engine_config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        engine_config.parallel_compilation(true);

        let engine = Engine::new(&engine_config).map_err(|e| Error::Engine(e.to_string()))?;

        Ok(Self {
            engine,
            module_cache: Arc::new(DashMap::new()),
            config,
        })
    }

    /// Compile a WASM module.
    pub fn compile(&self, wasm_module: &WasmModule) -> Result<CompiledModule> {
        let hash = wasm_module.hash().clone();

        // Check cache first
        if let Some(cached) = self.module_cache.get(&hash) {
            return Ok(CompiledModule {
                module: cached.clone(),
                hash,
            });
        }

        // Compile the module
        let module = Module::new(&self.engine, wasm_module.bytes())
            .map_err(|e| Error::Compilation(e.to_string()))?;

        // Cache if under limit
        if self.module_cache.len() < self.config.max_cached_modules {
            self.module_cache.insert(hash.clone(), module.clone());
        }

        Ok(CompiledModule { module, hash })
    }

    /// Create a new instance of a compiled module.
    pub fn instantiate(
        &self,
        module: &CompiledModule,
        config: &SandboxConfig,
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
    ) -> Result<WasmInstance> {
        self.instantiate_with_input(module, config, enforcer, meter, None)
    }

    /// Create a new instance of a compiled module with input data.
    pub fn instantiate_with_input(
        &self,
        module: &CompiledModule,
        config: &SandboxConfig,
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
        input: Option<Vec<u8>>,
    ) -> Result<WasmInstance> {
        WasmInstance::new(self, module, config, enforcer, meter, input)
    }

    /// Get the underlying Wasmtime engine.
    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Increment the epoch (for interruption).
    pub fn increment_epoch(&self) {
        self.engine.increment_epoch();
    }

    /// Clear the module cache.
    pub fn clear_cache(&self) {
        self.module_cache.clear();
    }

    /// Get the number of cached modules.
    pub fn cached_module_count(&self) -> usize {
        self.module_cache.len()
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASM engine")
    }
}

/// State held by the WASM store.
pub struct SandboxWasiState {
    /// WASI preview1 context.
    wasi: WasiP1Ctx,
    /// Host state for sandbox operations.
    #[allow(dead_code)]
    host: HostState,
    /// Initial fuel amount (for calculating consumed fuel).
    initial_fuel: Option<u64>,
    /// Resource limits for memory, tables, etc.
    limits: StoreLimits,
}

impl SandboxWasiState {
    /// Get mutable reference to WASI context.
    fn wasi_ctx(&mut self) -> &mut WasiP1Ctx {
        &mut self.wasi
    }

    /// Get mutable reference to store limits (for limiter callback).
    fn store_limits(&mut self) -> &mut StoreLimits {
        &mut self.limits
    }
}

/// A WASM instance ready for execution.
pub struct WasmInstance {
    store: Store<SandboxWasiState>,
    instance: wasmtime::Instance,
    entry_point: String,
    stdout_buffer: CaptureBuffer,
    stderr_buffer: CaptureBuffer,
}

impl WasmInstance {
    /// Create a new WASM instance.
    fn new(
        engine: &WasmEngine,
        module: &CompiledModule,
        config: &SandboxConfig,
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
        input: Option<Vec<u8>>,
    ) -> Result<Self> {
        // Create capture buffers for stdout/stderr
        let stdout_buffer = new_capture_buffer();
        let stderr_buffer = new_capture_buffer();

        // Build WASI context using preview1 API
        let mut wasi_builder = WasiCtxBuilder::new();

        // Configure stdin - use buffered input if provided and allowed, empty otherwise
        // Include metering if I/O limits are configured
        if enforcer.check_stdin().is_ok() {
            if let Some(data) = input {
                if config.resources.io.is_limited() {
                    wasi_builder.stdin(BufferedStdin::with_meter(data, meter.clone()));
                } else {
                    wasi_builder.stdin(BufferedStdin::new(data));
                }
            } else {
                wasi_builder.stdin(EmptyStdin);
            }
        } else {
            wasi_builder.stdin(EmptyStdin);
        }

        // Configure stdout - capture if allowed, null otherwise
        // Include metering if I/O limits are configured
        if enforcer.check_stdout().is_ok() {
            if config.resources.io.is_limited() {
                wasi_builder.stdout(CaptureStream::with_meter(
                    stdout_buffer.clone(),
                    meter.clone(),
                ));
            } else {
                wasi_builder.stdout(CaptureStream::new(stdout_buffer.clone()));
            }
        } else {
            wasi_builder.stdout(NullStream);
        }

        // Configure stderr - capture if allowed, null otherwise
        // Include metering if I/O limits are configured
        if enforcer.check_stderr().is_ok() {
            if config.resources.io.is_limited() {
                wasi_builder.stderr(CaptureStream::with_meter(
                    stderr_buffer.clone(),
                    meter.clone(),
                ));
            } else {
                wasi_builder.stderr(CaptureStream::new(stderr_buffer.clone()));
            }
        } else {
            wasi_builder.stderr(NullStream);
        }

        // Configure environment variables
        for (key, value) in &config.env {
            if enforcer.check_env_var(key).is_ok() {
                wasi_builder.env(key, value);
            }
        }

        // Configure arguments
        if enforcer.check_args().is_ok() {
            let args: Vec<&str> = config.args.iter().map(|s| s.as_str()).collect();
            wasi_builder.args(&args);
        }

        // Configure preopened directories for filesystem access
        for (host_path, guest_path) in enforcer.filesystem_preopens() {
            if host_path.exists() {
                // Determine permissions based on capability
                let read_only = !enforcer.check_fs_write(&host_path).is_ok();

                let dir_perms = if read_only {
                    wasmtime_wasi::DirPerms::READ
                } else {
                    wasmtime_wasi::DirPerms::all()
                };

                let file_perms = if read_only {
                    wasmtime_wasi::FilePerms::READ
                } else {
                    wasmtime_wasi::FilePerms::all()
                };

                // Use WasiCtxBuilder::preopened_dir which takes (host_path, guest_path, dir_perms, file_perms)
                if wasi_builder
                    .preopened_dir(&host_path, &guest_path, dir_perms, file_perms)
                    .is_ok()
                {
                    tracing::debug!(
                        host_path = %host_path.display(),
                        guest_path = %guest_path,
                        read_only = read_only,
                        "Preopened directory for WASI"
                    );
                }
            }
        }

        // Build preview1 WASI context
        let wasi = wasi_builder.build_p1();

        // Create host state
        let host = HostState::new(enforcer, meter);

        // Store initial fuel for tracking consumption
        let initial_fuel = config.resources.cpu.fuel;

        // Create store limits for memory enforcement
        let limits = StoreLimitsBuilder::new()
            .memory_size(config.resources.memory.heap_max)
            .trap_on_grow_failure(true)
            .build();

        // Create store
        let state = SandboxWasiState {
            wasi,
            host,
            initial_fuel,
            limits,
        };
        let mut store = Store::new(engine.engine(), state);

        // Configure memory limiter
        store.limiter(|state| state.store_limits());

        // Configure fuel if enabled
        if let Some(fuel) = config.resources.cpu.fuel {
            store
                .set_fuel(fuel)
                .map_err(|e| Error::Engine(e.to_string()))?;
        }

        // Configure epoch deadline if enabled
        // The epoch-based interruption system works by comparing the engine's current epoch
        // against the store's deadline. External code will call set_epoch_deadline() before
        // running and spawn a task to increment epochs periodically.
        if engine.config.enable_epoch_interruption {
            store.epoch_deadline_trap();
            // Initially set to 1 - external code will call set_epoch_deadline() with the
            // appropriate value before running. Without a deadline set, execution would
            // immediately trap, so we start with a permissive value.
            store.set_epoch_deadline(u64::MAX);
        }

        // Create linker and add WASI preview1 functions
        let mut linker: Linker<SandboxWasiState> = Linker::new(engine.engine());
        preview1::add_to_linker_sync(&mut linker, SandboxWasiState::wasi_ctx)
            .map_err(|e| Error::Instantiation(e.to_string()))?;

        // Instantiate the module
        let instance = linker
            .instantiate(&mut store, &module.module)
            .map_err(|e| Error::Instantiation(e.to_string()))?;

        Ok(Self {
            store,
            instance,
            entry_point: config.entry_point.clone(),
            stdout_buffer,
            stderr_buffer,
        })
    }

    /// Run the WASM instance.
    pub fn run(&mut self) -> Result<ExecutionResult> {
        let start = Instant::now();

        // Get the entry point function
        let func = self
            .instance
            .get_func(&mut self.store, &self.entry_point)
            .ok_or_else(|| Error::FunctionNotFound(self.entry_point.clone()))?;

        // Call the function
        let result = func.call(&mut self.store, &[], &mut []);

        let elapsed = start.elapsed();

        match result {
            Ok(()) => Ok(ExecutionResult {
                exit_code: 0,
                stdout: self.stdout_buffer.read().clone(),
                stderr: self.stderr_buffer.read().clone(),
                elapsed,
                fuel_consumed: self.fuel_consumed(),
            }),
            Err(e) => {
                // Check for specific error types
                let error_msg = e.to_string();

                tracing::debug!(
                    error = %error_msg,
                    error_type = std::any::type_name_of_val(&e),
                    "WASM execution error"
                );

                if error_msg.contains("out of fuel") {
                    let limit = self.store.get_fuel().unwrap_or(0);
                    return Err(Error::FuelExhausted { limit });
                }

                if error_msg.contains("epoch") {
                    return Err(Error::Timeout(elapsed));
                }

                // Check for WASI exit code - try downcast first
                if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                    tracing::debug!(exit_code = exit.0, "Caught I32Exit via downcast_ref");
                    return Ok(ExecutionResult {
                        exit_code: exit.0,
                        stdout: self.stdout_buffer.read().clone(),
                        stderr: self.stderr_buffer.read().clone(),
                        elapsed,
                        fuel_consumed: self.fuel_consumed(),
                    });
                }

                // Also check root cause for I32Exit (in case it's wrapped)
                if let Some(exit) = e.root_cause().downcast_ref::<wasmtime_wasi::I32Exit>() {
                    tracing::debug!(exit_code = exit.0, "Caught I32Exit via root_cause");
                    return Ok(ExecutionResult {
                        exit_code: exit.0,
                        stdout: self.stdout_buffer.read().clone(),
                        stderr: self.stderr_buffer.read().clone(),
                        elapsed,
                        fuel_consumed: self.fuel_consumed(),
                    });
                }

                // Fallback: check error message for WASI exit pattern
                let root_cause_msg = e.root_cause().to_string();
                if root_cause_msg.contains("Exited with i32 exit status") {
                    if let Some(code_str) = root_cause_msg
                        .rsplit("exit status ")
                        .next()
                        .and_then(|s| s.split_whitespace().next())
                    {
                        if let Ok(code) = code_str.parse::<i32>() {
                            tracing::debug!(
                                exit_code = code,
                                "Caught exit via error message parsing"
                            );
                            return Ok(ExecutionResult {
                                exit_code: code,
                                stdout: self.stdout_buffer.read().clone(),
                                stderr: self.stderr_buffer.read().clone(),
                                elapsed,
                                fuel_consumed: self.fuel_consumed(),
                            });
                        }
                    }
                }

                Err(Error::Execution(error_msg))
            }
        }
    }

    /// Call a specific exported function.
    pub fn call(&mut self, name: &str, args: &[Val]) -> Result<Vec<Val>> {
        let func = self
            .instance
            .get_func(&mut self.store, name)
            .ok_or_else(|| Error::FunctionNotFound(name.to_string()))?;

        let ty = func.ty(&self.store);
        let mut results = vec![Val::I32(0); ty.results().len()];

        func.call(&mut self.store, args, &mut results)
            .map_err(|e| Error::Execution(e.to_string()))?;

        Ok(results)
    }

    /// Get remaining fuel.
    pub fn remaining_fuel(&self) -> Option<u64> {
        self.store.get_fuel().ok()
    }

    /// Get fuel consumed.
    pub fn fuel_consumed(&self) -> Option<u64> {
        let initial = self.store.data().initial_fuel?;
        let remaining = self.store.get_fuel().ok()?;
        Some(initial.saturating_sub(remaining))
    }

    /// Get the stdout buffer contents.
    pub fn stdout(&self) -> Vec<u8> {
        self.stdout_buffer.read().clone()
    }

    /// Get the stderr buffer contents.
    pub fn stderr(&self) -> Vec<u8> {
        self.stderr_buffer.read().clone()
    }

    /// Set the epoch deadline for timeout interruption.
    /// The deadline is relative to the current engine epoch.
    pub fn set_epoch_deadline(&mut self, deadline_epochs: u64) {
        self.store.set_epoch_deadline(deadline_epochs);
    }

    /// Get the current memory contents.
    /// Returns None if the module doesn't export memory.
    pub fn get_memory(&mut self) -> Option<Vec<u8>> {
        let memory = self.instance.get_memory(&mut self.store, "memory")?;
        let data = memory.data(&self.store);
        Some(data.to_vec())
    }

    /// Get the current memory size in bytes.
    pub fn memory_size(&mut self) -> Option<usize> {
        let memory = self.instance.get_memory(&mut self.store, "memory")?;
        Some(memory.data_size(&self.store))
    }

    /// Write memory contents (for snapshot restore).
    /// Returns an error if the memory sizes don't match.
    pub fn set_memory(&mut self, data: &[u8]) -> Result<()> {
        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .ok_or_else(|| Error::Engine("Module has no exported memory".into()))?;

        let current_size = memory.data_size(&self.store);
        if data.len() != current_size {
            // Try to grow memory if needed
            let current_pages = memory.size(&self.store);
            let needed_pages = (data.len() + 65535) / 65536;
            if needed_pages > current_pages as usize {
                let grow_by = needed_pages - current_pages as usize;
                memory
                    .grow(&mut self.store, grow_by as u64)
                    .map_err(|e| Error::Engine(format!("Failed to grow memory: {}", e)))?;
            }
        }

        // Write the data
        let mem_data = memory.data_mut(&mut self.store);
        let copy_len = data.len().min(mem_data.len());
        mem_data[..copy_len].copy_from_slice(&data[..copy_len]);

        Ok(())
    }

    /// Get the values of all global variables.
    #[cfg(feature = "snapshots")]
    pub fn get_globals(&mut self) -> Vec<crate::snapshot::GlobalValue> {
        use crate::snapshot::GlobalValue;

        // Collect globals first since we can't iterate and read at the same time
        let globals: Vec<_> = self
            .instance
            .exports(&mut self.store)
            .filter_map(|e| e.into_global())
            .collect();

        let mut result = Vec::new();
        for (idx, global) in globals.iter().enumerate() {
            let val = global.get(&mut self.store);
            // Note: Wasmtime's Val::F32/F64 already store the bit representation as u32/u64
            let snapshot_val = match val {
                Val::I32(v) => GlobalValue::I32(v),
                Val::I64(v) => GlobalValue::I64(v),
                Val::F32(v) => GlobalValue::F32(v), // Already u32 bits
                Val::F64(v) => GlobalValue::F64(v), // Already u64 bits
                Val::V128(v) => GlobalValue::V128(v.as_u128().to_le_bytes()),
                Val::FuncRef(f) => GlobalValue::FuncRef(f.map(|_| idx as u32)),
                Val::ExternRef(e) => GlobalValue::ExternRef(e.map(|_| idx as u32)),
                Val::AnyRef(_) => continue, // Skip AnyRef for now
            };
            result.push(snapshot_val);
        }

        result
    }

    /// Set global variable values (for snapshot restore).
    #[cfg(feature = "snapshots")]
    pub fn set_globals(&mut self, values: &[crate::snapshot::GlobalValue]) -> Result<()> {
        use crate::snapshot::GlobalValue;

        // Collect globals first since we can't iterate and mutate at the same time
        let globals: Vec<_> = self
            .instance
            .exports(&mut self.store)
            .filter_map(|e| e.into_global())
            .collect();

        for (idx, global) in globals.into_iter().enumerate() {
            if idx >= values.len() {
                break;
            }

            // Only set mutable globals
            if global.ty(&self.store).mutability() == wasmtime::Mutability::Var {
                // Note: Wasmtime's Val::F32/F64 take u32/u64 bit representations directly
                let val = match &values[idx] {
                    GlobalValue::I32(v) => Val::I32(*v),
                    GlobalValue::I64(v) => Val::I64(*v),
                    GlobalValue::F32(v) => Val::F32(*v), // Already u32 bits
                    GlobalValue::F64(v) => Val::F64(*v), // Already u64 bits
                    GlobalValue::V128(v) => Val::V128(u128::from_le_bytes(*v).into()),
                    GlobalValue::FuncRef(_) | GlobalValue::ExternRef(_) => {
                        // Skip reference types for now
                        continue;
                    }
                };

                if let Err(e) = global.set(&mut self.store, val) {
                    tracing::warn!(idx = idx, error = %e, "Failed to set global");
                }
            }
        }

        Ok(())
    }
}

/// Result of WASM execution.
#[derive(Debug)]
pub struct ExecutionResult {
    /// Exit code (0 for success).
    pub exit_code: i32,
    /// Captured stdout.
    pub stdout: Vec<u8>,
    /// Captured stderr.
    pub stderr: Vec<u8>,
    /// Execution time.
    pub elapsed: std::time::Duration,
    /// Fuel consumed (if metering enabled).
    pub fuel_consumed: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Capability, CapabilityEnforcer};
    use crate::config::SandboxConfig;
    use crate::resource::ResourceMeter;

    // Minimal valid WASM module
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ];

    // WASM module with _start that calls proc_exit(0)
    const RUNNABLE_WASM: &[u8] = include_bytes!("../../tests/fixtures/minimal.wasm");

    #[test]
    fn test_wasm_engine_creation() {
        let engine = WasmEngine::new().unwrap();
        assert_eq!(engine.cached_module_count(), 0);
    }

    #[test]
    fn test_wasm_engine_compile() {
        let engine = WasmEngine::new().unwrap();
        let module = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();

        let compiled = engine.compile(&module).unwrap();
        assert_eq!(compiled.hash(), module.hash());

        // Second compile should use cache
        let compiled2 = engine.compile(&module).unwrap();
        assert_eq!(compiled2.hash(), compiled.hash());
        assert_eq!(engine.cached_module_count(), 1);
    }

    #[test]
    fn test_wasm_engine_clear_cache() {
        let engine = WasmEngine::new().unwrap();
        let module = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();

        engine.compile(&module).unwrap();
        assert_eq!(engine.cached_module_count(), 1);

        engine.clear_cache();
        assert_eq!(engine.cached_module_count(), 0);
    }

    #[test]
    fn test_wasm_instance_i32exit() {
        let engine = WasmEngine::new().unwrap();
        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&wasm_module).unwrap();

        let config = SandboxConfig::builder()
            .module(RUNNABLE_WASM)
            .expect("valid module")
            .fuel(1_000_000)
            .capability(Capability::stdout())
            .build()
            .expect("valid config");

        let enforcer = CapabilityEnforcer::new(config.capabilities.clone(), uuid::Uuid::new_v4());
        let meter = ResourceMeter::new(config.resources.clone());

        let mut instance = engine
            .instantiate(&compiled, &config, enforcer, meter)
            .unwrap();
        let result = instance.run();

        match result {
            Ok(exec_result) => {
                assert_eq!(exec_result.exit_code, 0, "Expected exit code 0");
            }
            Err(e) => {
                panic!("Expected Ok, got Err: {:?}", e);
            }
        }
    }
}
