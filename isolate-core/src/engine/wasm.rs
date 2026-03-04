//! WASM engine implementation using Wasmtime.

use crate::capability::CapabilityEnforcer;
use crate::config::{ModuleHash, SandboxConfig, WasmModule};
use crate::error::{Error, Result};
use crate::resource::ResourceMeter;

use super::capture::{
    new_capture_buffer, BufferedStdin, CaptureBuffer, CaptureStream, EmptyStdin, NullStream,
    OutputSource, StreamingCaptureStream,
};
use super::host::HostState;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Epoch tick interval in milliseconds (default 10ms).
    /// Lower values give more precise timeouts but increase overhead.
    pub epoch_tick_ms: u64,
    /// Maximum WASM module size in bytes (0 = unlimited).
    pub max_module_bytes: usize,
}

impl Default for WasmEngineConfig {
    fn default() -> Self {
        Self {
            enable_fuel: true,
            enable_epoch_interruption: true,
            max_cached_modules: 100,
            epoch_tick_ms: 10,
            max_module_bytes: 50 * 1024 * 1024, // 50MB
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

    /// Check if the module exports a function with the given name.
    pub fn has_export(&self, name: &str) -> bool {
        self.module.exports().any(|e| e.name() == name)
    }

    /// Get a reference to the underlying Wasmtime module.
    pub(crate) fn module_ref(&self) -> &Module {
        &self.module
    }

    /// List all imports the module requires.
    ///
    /// Each import has a module name, field name, and type. Use this to
    /// determine what capabilities or WASI functions the module needs.
    pub fn required_imports(&self) -> Vec<ImportDescriptor> {
        self.module
            .imports()
            .map(|import| ImportDescriptor {
                module: import.module().to_string(),
                name: import.name().to_string(),
                kind: match import.ty() {
                    wasmtime::ExternType::Func(_) => ImportKind::Function,
                    wasmtime::ExternType::Global(_) => ImportKind::Global,
                    wasmtime::ExternType::Table(_) => ImportKind::Table,
                    wasmtime::ExternType::Memory(_) => ImportKind::Memory,
                },
            })
            .collect()
    }

    /// List all exports the module provides.
    pub fn exported_functions(&self) -> Vec<ExportDescriptor> {
        self.module
            .exports()
            .map(|export| ExportDescriptor {
                name: export.name().to_string(),
                kind: match export.ty() {
                    wasmtime::ExternType::Func(_) => ExportKind::Function,
                    wasmtime::ExternType::Global(_) => ExportKind::Global,
                    wasmtime::ExternType::Table(_) => ExportKind::Table,
                    wasmtime::ExternType::Memory(_) => ExportKind::Memory,
                },
            })
            .collect()
    }

    /// Get the module's memory requirements.
    ///
    /// Returns the initial and maximum memory pages declared in the module.
    /// Each page is 64 KiB.
    pub fn memory_requirements(&self) -> Option<MemoryRequirements> {
        for export in self.module.exports() {
            if let wasmtime::ExternType::Memory(mem_ty) = export.ty() {
                return Some(MemoryRequirements {
                    initial_pages: mem_ty.minimum(),
                    maximum_pages: mem_ty.maximum(),
                    initial_bytes: mem_ty.minimum().saturating_mul(65536),
                    maximum_bytes: mem_ty.maximum().map(|m| m.saturating_mul(65536)),
                });
            }
        }
        None
    }

    /// Check compatibility between this module and a sandbox configuration.
    ///
    /// Returns a report detailing whether the module is likely to run
    /// successfully with the given config. Catches mismatches that would
    /// otherwise surface as runtime errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::{SandboxConfig, engine::WasmEngine, capability::Capability, config::WasmModule};
    /// # async fn example() -> isolate_core::Result<()> {
    /// let engine = WasmEngine::new()?;
    /// let wasm = std::fs::read("module.wasm")?;
    /// let wasm_module = WasmModule::from_bytes(wasm.clone())?;
    /// let module = engine.compile(&wasm_module)?;
    ///
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .capability(Capability::stdout())
    ///     .build()?;
    ///
    /// let report = module.check_compatibility(&config);
    /// if !report.is_compatible() {
    ///     for issue in &report.issues {
    ///         eprintln!("⚠ {}", issue);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn check_compatibility(
        &self,
        config: &crate::config::SandboxConfig,
    ) -> CompatibilityReport {
        let mut issues = Vec::new();

        // Check entry point exists
        if !self.has_export(&config.entry_point) {
            let available: Vec<_> = self
                .exported_functions()
                .iter()
                .filter(|e| e.kind == ExportKind::Function)
                .map(|e| e.name.clone())
                .collect();
            issues.push(CompatibilityIssue {
                severity: IssueSeverity::Error,
                category: "entry_point".to_string(),
                message: format!(
                    "Entry point '{}' not found in module exports",
                    config.entry_point
                ),
                suggestion: if available.is_empty() {
                    "Module exports no functions".to_string()
                } else {
                    format!("Available functions: {}", available.join(", "))
                },
            });
        }

        // Check memory compatibility
        if let Some(mem_req) = self.memory_requirements() {
            let configured = config.resources.memory.heap_max as u64;
            if mem_req.initial_bytes > configured {
                issues.push(CompatibilityIssue {
                    severity: IssueSeverity::Error,
                    category: "memory".to_string(),
                    message: format!(
                        "Module requires {} bytes initial memory, but config allows only {} bytes",
                        mem_req.initial_bytes, configured
                    ),
                    suggestion: format!(
                        "Increase memory_limit to at least {}",
                        mem_req.initial_bytes
                    ),
                });
            } else if mem_req.initial_bytes as f64 / configured as f64 > 0.8 {
                issues.push(CompatibilityIssue {
                    severity: IssueSeverity::Warning,
                    category: "memory".to_string(),
                    message: format!(
                        "Module's initial memory ({} bytes) uses >80% of configured limit ({} bytes)",
                        mem_req.initial_bytes, configured
                    ),
                    suggestion: "Consider increasing memory_limit to leave room for runtime allocations".to_string(),
                });
            }
        }

        // Check WASI import requirements
        let imports = self.required_imports();
        let non_wasi: Vec<_> =
            imports.iter().filter(|i| i.module != "wasi_snapshot_preview1").collect();
        if !non_wasi.is_empty() {
            for imp in &non_wasi {
                issues.push(CompatibilityIssue {
                    severity: IssueSeverity::Warning,
                    category: "imports".to_string(),
                    message: format!(
                        "Module imports '{}::{}' which is not a standard WASI function",
                        imp.module, imp.name
                    ),
                    suggestion:
                        "Register a host function for this import or use a WASI-only module"
                            .to_string(),
                });
            }
        }

        CompatibilityReport {
            compatible: !issues.iter().any(|i| i.severity == IssueSeverity::Error),
            issues,
        }
    }
}

/// Describes a WASM module import.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportDescriptor {
    /// Module the import comes from (e.g., "wasi_snapshot_preview1").
    pub module: String,
    /// Name of the imported item.
    pub name: String,
    /// Kind of import.
    pub kind: ImportKind,
}

/// Kind of WASM import or export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ImportKind {
    /// A function import.
    Function,
    /// A global variable import.
    Global,
    /// A table import.
    Table,
    /// A memory import.
    Memory,
}

/// Describes a WASM module export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportDescriptor {
    /// Name of the exported item.
    pub name: String,
    /// Kind of export.
    pub kind: ExportKind,
}

/// Kind of WASM export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ExportKind {
    /// A function export.
    Function,
    /// A global variable export.
    Global,
    /// A table export.
    Table,
    /// A memory export.
    Memory,
}

/// Memory requirements of a WASM module.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct MemoryRequirements {
    /// Initial memory pages (each page = 64 KiB).
    pub initial_pages: u64,
    /// Maximum memory pages, if declared.
    pub maximum_pages: Option<u64>,
    /// Initial memory in bytes.
    pub initial_bytes: u64,
    /// Maximum memory in bytes, if declared.
    pub maximum_bytes: Option<u64>,
}

/// Report from a module compatibility check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompatibilityReport {
    /// Whether the module is expected to run successfully.
    pub compatible: bool,
    /// Individual issues found.
    pub issues: Vec<CompatibilityIssue>,
}

impl CompatibilityReport {
    /// Whether the module is compatible with the configuration.
    pub fn is_compatible(&self) -> bool {
        self.compatible
    }

    /// Get only error-severity issues.
    pub fn errors(&self) -> Vec<&CompatibilityIssue> {
        self.issues.iter().filter(|i| i.severity == IssueSeverity::Error).collect()
    }

    /// Get only warning-severity issues.
    pub fn warnings(&self) -> Vec<&CompatibilityIssue> {
        self.issues.iter().filter(|i| i.severity == IssueSeverity::Warning).collect()
    }
}

impl std::fmt::Display for CompatibilityReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.compatible {
            write!(f, "Compatible")?;
        } else {
            write!(f, "Incompatible")?;
        }
        if !self.issues.is_empty() {
            write!(f, " ({} issues)", self.issues.len())?;
        }
        for issue in &self.issues {
            write!(f, "\n  {}", issue)?;
        }
        Ok(())
    }
}

/// A single compatibility issue.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompatibilityIssue {
    /// Severity of the issue.
    pub severity: IssueSeverity,
    /// Category (e.g., "entry_point", "memory", "imports").
    pub category: String,
    /// Description of the issue.
    pub message: String,
    /// Suggested fix.
    pub suggestion: String,
}

impl std::fmt::Display for CompatibilityIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let icon = match self.severity {
            IssueSeverity::Error => "✗",
            IssueSeverity::Warning => "⚠",
        };
        write!(f, "{} [{}] {}", icon, self.category, self.message)
    }
}

/// Severity of a compatibility issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum IssueSeverity {
    /// Will prevent execution.
    Error,
    /// May cause unexpected behavior.
    Warning,
}

/// A cached module entry with a sequence counter for LRU eviction.
#[derive(Clone)]
struct CachedEntry {
    module: Module,
    access_seq: u64,
}

/// The WASM execution engine.
#[derive(Clone)]
pub struct WasmEngine {
    engine: Engine,
    module_cache: Arc<DashMap<ModuleHash, CachedEntry>>,
    config: WasmEngineConfig,
    /// Whether the global epoch ticker has been started.
    epoch_ticker_started: Arc<AtomicBool>,
    /// Signal to stop the epoch ticker task.
    epoch_ticker_shutdown: Arc<AtomicBool>,
    /// Monotonic counter for LRU eviction ordering.
    access_counter: Arc<std::sync::atomic::AtomicU64>,
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
            epoch_ticker_started: Arc::new(AtomicBool::new(false)),
            epoch_ticker_shutdown: Arc::new(AtomicBool::new(false)),
            access_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        })
    }

    /// Compile a WASM module.
    pub fn compile(&self, wasm_module: &WasmModule) -> Result<CompiledModule> {
        // Enforce module size limit
        if self.config.max_module_bytes > 0
            && wasm_module.bytes().len() > self.config.max_module_bytes
        {
            return Err(Error::ModuleValidation(format!(
                "Module size {} bytes exceeds limit of {} bytes",
                wasm_module.bytes().len(),
                self.config.max_module_bytes
            )));
        }

        let hash = wasm_module.hash().clone();

        // Check cache first
        if let Some(mut cached) = self.module_cache.get_mut(&hash) {
            cached.access_seq = self.access_counter.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(module_hash = %hash, "module cache hit");
            return Ok(CompiledModule { module: cached.module.clone(), hash });
        }

        tracing::debug!(module_hash = %hash, "module cache miss — compiling");

        // Compile the module
        let module = Module::new(&self.engine, wasm_module.bytes()).map_err(|e| {
            let detail = e.to_string();
            let phase =
                if detail.contains("failed to parse") || detail.contains("unexpected content") {
                    "parsing"
                } else if detail.contains("validation") || detail.contains("type mismatch") {
                    "validation"
                } else {
                    "compilation"
                };
            Error::Compilation(format!("{} error: {}", phase, detail))
        })?;

        let seq = self.access_counter.fetch_add(1, Ordering::Relaxed);
        self.module_cache
            .insert(hash.clone(), CachedEntry { module: module.clone(), access_seq: seq });

        // Evict LRU entries until within capacity. Using a while loop instead
        // of a single check handles the case where multiple concurrent compilers
        // insert simultaneously, pushing the cache over the limit.
        while self.module_cache.len() > self.config.max_cached_modules {
            if let Some(oldest_key) = self
                .module_cache
                .iter()
                .filter(|e| e.key() != &hash) // don't evict the entry we just inserted
                .min_by_key(|entry| entry.value().access_seq)
                .map(|entry| entry.key().clone())
            {
                tracing::debug!(evicted_hash = %oldest_key, "module cache over capacity — evicting LRU entry");
                self.module_cache.remove(&oldest_key);
            } else {
                break;
            }
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

    /// Create a new instance wired for streaming output via a channel.
    pub fn instantiate_streaming(
        &self,
        module: &CompiledModule,
        config: &SandboxConfig,
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
        input: Option<Vec<u8>>,
        sender: Arc<tokio::sync::mpsc::Sender<super::capture::OutputChunk>>,
    ) -> Result<WasmInstance> {
        WasmInstance::new_streaming(self, module, config, enforcer, meter, input, sender)
    }

    /// Get the underlying Wasmtime engine.
    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the engine configuration.
    pub(crate) fn config(&self) -> &WasmEngineConfig {
        &self.config
    }

    /// Increment the epoch (for interruption).
    pub fn increment_epoch(&self) {
        self.engine.increment_epoch();
    }

    /// Ensure the global epoch ticker is running.
    ///
    /// Starts a single background tokio task that increments the engine epoch
    /// every 10ms. This replaces per-sandbox ticker tasks and scales to thousands
    /// of concurrent sandboxes with constant overhead.
    pub fn ensure_epoch_ticker(&self) {
        if !self.config.enable_epoch_interruption {
            return;
        }
        if self.epoch_ticker_started.swap(true, Ordering::SeqCst) {
            return; // Already started
        }
        let engine = self.engine.clone();
        let tick_ms = self.config.epoch_tick_ms;
        let shutdown = self.epoch_ticker_shutdown.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
            loop {
                interval.tick().await;
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                engine.increment_epoch();
            }
        });
    }

    /// Signal the epoch ticker to stop.
    ///
    /// The ticker task will exit on its next tick after this is called.
    pub fn shutdown_epoch_ticker(&self) {
        self.epoch_ticker_shutdown.store(true, Ordering::Relaxed);
    }

    /// Clear the module cache.
    pub fn clear_cache(&self) {
        self.module_cache.clear();
    }

    /// Get the number of cached modules.
    pub fn cached_module_count(&self) -> usize {
        self.module_cache.len()
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            cached_modules: self.module_cache.len(),
            max_modules: self.config.max_cached_modules,
        }
    }
}

/// Module cache statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    /// Number of currently cached modules.
    pub cached_modules: usize,
    /// Maximum cache capacity.
    pub max_modules: usize,
}

// WasmEngine intentionally does not implement Default because engine
// creation is fallible (allocates resources, configures Wasmtime).
// Use `WasmEngine::new()` or `WasmEngine::with_config()` instead.

/// State held by the WASM store.
pub struct SandboxWasiState {
    /// WASI preview1 context.
    wasi: WasiP1Ctx,
    /// Host state for sandbox operations.
    #[allow(dead_code)] // Retained for future host-function access from the store
    host: HostState,
    /// Initial fuel amount (for calculating consumed fuel).
    initial_fuel: Option<u64>,
    /// Resource limits for memory, tables, etc.
    limits: StoreLimits,
}

impl SandboxWasiState {
    /// Create a new sandbox WASI state.
    pub(crate) fn new(
        wasi: WasiP1Ctx,
        host: HostState,
        initial_fuel: Option<u64>,
        limits: StoreLimits,
    ) -> Self {
        Self { wasi, host, initial_fuel, limits }
    }

    /// Get mutable reference to WASI context.
    pub(crate) fn wasi_ctx(&mut self) -> &mut WasiP1Ctx {
        &mut self.wasi
    }

    /// Get mutable reference to store limits (for limiter callback).
    pub(crate) fn store_limits(&mut self) -> &mut StoreLimits {
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
                wasi_builder
                    .stdout(CaptureStream::with_meter(stdout_buffer.clone(), meter.clone()));
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
                wasi_builder
                    .stderr(CaptureStream::with_meter(stderr_buffer.clone(), meter.clone()));
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
                let read_only = enforcer.check_fs_write(&host_path).is_err();

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
        let state = SandboxWasiState { wasi, host, initial_fuel, limits };
        let mut store = Store::new(engine.engine(), state);

        // Configure memory limiter
        store.limiter(|state| state.store_limits());

        // Configure fuel if enabled
        if let Some(fuel) = config.resources.cpu.fuel {
            store.set_fuel(fuel).map_err(|e| Error::Engine(e.to_string()))?;
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

    /// Create a WASM instance that streams output chunks via a channel.
    fn new_streaming(
        engine: &WasmEngine,
        module: &CompiledModule,
        config: &SandboxConfig,
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
        input: Option<Vec<u8>>,
        sender: Arc<tokio::sync::mpsc::Sender<super::capture::OutputChunk>>,
    ) -> Result<Self> {
        let stdout_buffer = new_capture_buffer();
        let stderr_buffer = new_capture_buffer();

        let mut wasi_builder = WasiCtxBuilder::new();

        // Stdin setup (identical to non-streaming)
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

        // Stdout — streaming capture
        if enforcer.check_stdout().is_ok() {
            let m = if config.resources.io.is_limited() { Some(meter.clone()) } else { None };
            wasi_builder.stdout(StreamingCaptureStream::new(
                sender.clone(),
                OutputSource::Stdout,
                stdout_buffer.clone(),
                m,
            ));
        } else {
            wasi_builder.stdout(NullStream);
        }

        // Stderr — streaming capture
        if enforcer.check_stderr().is_ok() {
            let m = if config.resources.io.is_limited() { Some(meter.clone()) } else { None };
            wasi_builder.stderr(StreamingCaptureStream::new(
                sender.clone(),
                OutputSource::Stderr,
                stderr_buffer.clone(),
                m,
            ));
        } else {
            wasi_builder.stderr(NullStream);
        }

        // Environment, args, preopens (same as non-streaming)
        for (key, value) in &config.env {
            if enforcer.check_env_var(key).is_ok() {
                wasi_builder.env(key, value);
            }
        }
        if enforcer.check_args().is_ok() {
            let args: Vec<&str> = config.args.iter().map(|s| s.as_str()).collect();
            wasi_builder.args(&args);
        }
        for (host_path, guest_path) in enforcer.filesystem_preopens() {
            if host_path.exists() {
                let read_only = enforcer.check_fs_write(&host_path).is_err();
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
                let _ = wasi_builder.preopened_dir(&host_path, &guest_path, dir_perms, file_perms);
            }
        }

        let wasi = wasi_builder.build_p1();
        let host = HostState::new(enforcer, meter);
        let initial_fuel = config.resources.cpu.fuel;
        let limits = StoreLimitsBuilder::new()
            .memory_size(config.resources.memory.heap_max)
            .trap_on_grow_failure(true)
            .build();

        let state = SandboxWasiState { wasi, host, initial_fuel, limits };
        let mut store = Store::new(engine.engine(), state);
        store.limiter(|state| state.store_limits());

        if let Some(fuel) = config.resources.cpu.fuel {
            store.set_fuel(fuel).map_err(|e| Error::Engine(e.to_string()))?;
        }
        if engine.config.enable_epoch_interruption {
            store.epoch_deadline_trap();
            store.set_epoch_deadline(u64::MAX);
        }

        let mut linker: Linker<SandboxWasiState> = Linker::new(engine.engine());
        preview1::add_to_linker_sync(&mut linker, SandboxWasiState::wasi_ctx)
            .map_err(|e| Error::Instantiation(e.to_string()))?;

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

    /// Construct from pre-assembled parts (used by PreInitializedPool).
    pub(crate) fn from_parts(
        store: Store<SandboxWasiState>,
        instance: wasmtime::Instance,
        entry_point: String,
        stdout_buffer: CaptureBuffer,
        stderr_buffer: CaptureBuffer,
    ) -> Self {
        Self { store, instance, entry_point, stdout_buffer, stderr_buffer }
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
                    "WASM execution error"
                );

                if error_msg.contains("out of fuel") {
                    let remaining = self.store.get_fuel().unwrap_or(0);
                    let consumed = self.fuel_consumed().unwrap_or(0);
                    let limit = consumed.saturating_add(remaining);
                    return Err(Error::FuelExhausted { limit, consumed });
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
            let needed_pages = data.len().div_ceil(65536);
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
        let globals: Vec<_> =
            self.instance.exports(&mut self.store).filter_map(|e| e.into_global()).collect();

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
        let globals: Vec<_> =
            self.instance.exports(&mut self.store).filter_map(|e| e.into_global()).collect();

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

        let mut instance = engine.instantiate(&compiled, &config, enforcer, meter).unwrap();
        let result = instance.run();
        let exec_result = result.unwrap_or_else(|e| unreachable!("Expected Ok, got Err: {:?}", e));
        assert_eq!(exec_result.exit_code, 0, "Expected exit code 0");
    }

    #[test]
    fn test_engine_with_custom_config() {
        let config = WasmEngineConfig {
            enable_fuel: false,
            enable_epoch_interruption: false,
            max_cached_modules: 5,
            ..Default::default()
        };
        let engine = WasmEngine::with_config(config).unwrap();
        assert_eq!(engine.cached_module_count(), 0);
    }

    #[test]
    fn test_engine_compile_invalid_wasm() {
        let result = WasmModule::from_bytes(vec![0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(result.is_err());
    }

    #[test]
    fn test_engine_cache_eviction() {
        let config = WasmEngineConfig { max_cached_modules: 2, ..Default::default() };
        let engine = WasmEngine::with_config(config).unwrap();

        // Create 3 different valid WASM modules by varying a custom section
        let make_wasm = |byte: u8| -> Vec<u8> {
            let mut wasm = MINIMAL_WASM.to_vec();
            // Append a custom section (id=0) with a unique byte
            wasm.extend_from_slice(&[0x00, 0x02, 0x01, byte]);
            wasm
        };

        let m1 = WasmModule::from_bytes(make_wasm(0x01)).unwrap();
        let m2 = WasmModule::from_bytes(make_wasm(0x02)).unwrap();
        let m3 = WasmModule::from_bytes(make_wasm(0x03)).unwrap();

        engine.compile(&m1).unwrap();
        engine.compile(&m2).unwrap();
        assert_eq!(engine.cached_module_count(), 2);

        // This should evict the oldest entry
        engine.compile(&m3).unwrap();
        assert!(engine.cached_module_count() <= 2);
    }

    #[test]
    fn test_compiled_module_has_export() {
        let engine = WasmEngine::new().unwrap();
        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&wasm_module).unwrap();

        assert!(compiled.has_export("_start"));
        assert!(!compiled.has_export("nonexistent_function"));
    }

    #[test]
    fn test_required_imports() {
        let engine = WasmEngine::new().unwrap();
        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&wasm_module).unwrap();

        let imports = compiled.required_imports();
        // WASI module should import from wasi_snapshot_preview1
        assert!(!imports.is_empty());
        assert!(imports.iter().any(|i| i.module.contains("wasi")));
        // All WASI imports should be functions
        assert!(imports.iter().all(|i| i.kind == ImportKind::Function));
    }

    #[test]
    fn test_exported_functions() {
        let engine = WasmEngine::new().unwrap();
        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&wasm_module).unwrap();

        let exports = compiled.exported_functions();
        assert!(!exports.is_empty());
        // Should have _start and memory exports
        assert!(exports.iter().any(|e| e.name == "_start" && e.kind == ExportKind::Function));
        assert!(exports.iter().any(|e| e.name == "memory" && e.kind == ExportKind::Memory));
    }

    #[test]
    fn test_memory_requirements() {
        let engine = WasmEngine::new().unwrap();
        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&wasm_module).unwrap();

        let mem = compiled.memory_requirements();
        assert!(mem.is_some());
        let mem = mem.unwrap();
        assert!(mem.initial_pages >= 1);
        assert_eq!(mem.initial_bytes, mem.initial_pages * 65536);
    }

    #[test]
    fn test_cache_stats() {
        let config = WasmEngineConfig { max_cached_modules: 10, ..Default::default() };
        let engine = WasmEngine::with_config(config).unwrap();

        let stats = engine.cache_stats();
        assert_eq!(stats.cached_modules, 0);
        assert_eq!(stats.max_modules, 10);

        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        engine.compile(&wasm_module).unwrap();

        let stats = engine.cache_stats();
        assert_eq!(stats.cached_modules, 1);
    }

    #[test]
    fn test_module_size_limit() {
        let config = WasmEngineConfig {
            max_module_bytes: 10, // very small limit
            ..Default::default()
        };
        let engine = WasmEngine::with_config(config).unwrap();
        let wasm_module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let result = engine.compile(&wasm_module);
        assert!(result.is_err());
    }

    #[test]
    fn test_compatibility_report_minimal_module() {
        let engine = WasmEngine::new().unwrap();
        let module = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&module).unwrap();

        // Minimal WASM has no _start — entry point check should fail
        let config = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();

        let report = compiled.check_compatibility(&config);
        assert!(!report.is_compatible());
        assert!(report.errors().iter().any(|i| i.category == "entry_point"));
    }

    #[test]
    fn test_compatibility_report_runnable_module() {
        let engine = WasmEngine::new().unwrap();
        let module = WasmModule::from_bytes(RUNNABLE_WASM.to_vec()).unwrap();
        let compiled = engine.compile(&module).unwrap();

        let config = SandboxConfig::builder()
            .module(RUNNABLE_WASM)
            .unwrap()
            .memory_limit(64 * 1024 * 1024) // 64MB — plenty
            .build()
            .unwrap();

        let report = compiled.check_compatibility(&config);
        // Should be compatible (has _start, sufficient memory)
        assert!(report.is_compatible(), "Expected compatible, got: {}", report);
    }

    #[test]
    fn test_compatibility_report_display() {
        let report = CompatibilityReport {
            compatible: false,
            issues: vec![CompatibilityIssue {
                severity: IssueSeverity::Error,
                category: "entry_point".to_string(),
                message: "Missing _start".to_string(),
                suggestion: "Add _start export".to_string(),
            }],
        };
        let display = format!("{}", report);
        assert!(display.contains("Incompatible"));
        assert!(display.contains("Missing _start"));
    }

    #[test]
    fn test_compatibility_issue_display() {
        let issue = CompatibilityIssue {
            severity: IssueSeverity::Warning,
            category: "memory".to_string(),
            message: "High memory usage".to_string(),
            suggestion: "Increase limit".to_string(),
        };
        let display = format!("{}", issue);
        assert!(display.contains("⚠"));
        assert!(display.contains("memory"));
    }

    #[test]
    fn test_compatibility_report_accessors() {
        let report = CompatibilityReport {
            compatible: false,
            issues: vec![
                CompatibilityIssue {
                    severity: IssueSeverity::Error,
                    category: "entry_point".to_string(),
                    message: "missing".to_string(),
                    suggestion: "fix".to_string(),
                },
                CompatibilityIssue {
                    severity: IssueSeverity::Warning,
                    category: "memory".to_string(),
                    message: "tight".to_string(),
                    suggestion: "increase".to_string(),
                },
            ],
        };
        assert_eq!(report.errors().len(), 1);
        assert_eq!(report.warnings().len(), 1);
    }
}
