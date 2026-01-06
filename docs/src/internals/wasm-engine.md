# WASM Engine

Internal details of the Wasmtime-based execution engine.

## Overview

Isolate uses [Wasmtime](https://wasmtime.dev/) as its WebAssembly runtime. The engine layer provides:

- Module compilation and caching
- Instance creation with WASI
- Resource limit enforcement
- Epoch-based interruption

## WasmEngine

### Creation

```rust
pub struct WasmEngine {
    engine: wasmtime::Engine,
    module_cache: DashMap<ModuleHash, CompiledModule>,
}

impl WasmEngine {
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();

        // Enable epoch-based interruption
        config.epoch_interruption(true);

        // Enable fuel consumption
        config.consume_fuel(true);

        // Optimization settings
        config.cranelift_opt_level(OptLevel::Speed);

        let engine = wasmtime::Engine::new(&config)?;

        Ok(Self {
            engine,
            module_cache: DashMap::new(),
        })
    }
}
```

### Module Compilation

```rust
pub fn compile(&self, wasm: &WasmModule) -> Result<CompiledModule> {
    let hash = wasm.hash().clone();

    // Check cache first
    if let Some(compiled) = self.module_cache.get(&hash) {
        return Ok(compiled.clone());
    }

    // Compile
    let module = wasmtime::Module::new(&self.engine, wasm.bytes())?;

    let compiled = CompiledModule {
        module,
        hash: hash.clone(),
    };

    // Cache
    self.module_cache.insert(hash, compiled.clone());

    Ok(compiled)
}
```

## Instance Creation

### WASI Context Setup

```rust
fn create_wasi_context(
    config: &SandboxConfig,
    enforcer: &CapabilityEnforcer,
    input: Option<Vec<u8>>,
) -> Result<WasiCtx> {
    let mut builder = WasiCtxBuilder::new();

    // Stdin
    if let Some(data) = input {
        builder = builder.stdin(BufferedStdin::new(data));
    } else {
        builder = builder.stdin(EmptyStdin);
    }

    // Stdout/Stderr (if capabilities granted)
    if enforcer.check_stdout().is_ok() {
        builder = builder.stdout(CaptureStream::new());
    }

    // Filesystem preopens
    for cap in config.capabilities.iter() {
        if let Capability::FilesystemRead(path) = cap {
            builder = builder.preopened_dir(path, path, DirPerms::READ, FilePerms::READ)?;
        }
    }

    // Environment variables
    for (key, value) in &config.env {
        if enforcer.check_env_var(key).is_ok() {
            builder = builder.env(key, value)?;
        }
    }

    // Arguments
    builder = builder.args(&config.args)?;

    Ok(builder.build())
}
```

### Store Configuration

```rust
fn create_store(
    engine: &wasmtime::Engine,
    config: &SandboxConfig,
    wasi_ctx: WasiCtx,
) -> Store<StoreData> {
    let mut store = Store::new(engine, StoreData { wasi: wasi_ctx });

    // Set fuel limit
    if let Some(fuel) = config.resources.cpu.fuel {
        store.set_fuel(fuel).unwrap();
    }

    // Set memory limits via StoreLimiter
    store.limiter(|data| &mut data.limiter);

    store
}
```

## Resource Enforcement

### Fuel Metering

```rust
// Before execution
store.set_fuel(fuel_limit)?;

// After execution
let remaining = store.get_fuel()?;
let consumed = fuel_limit - remaining;
```

### Memory Limits

```rust
struct StoreLimiter {
    heap_max: usize,
    current: usize,
}

impl ResourceLimiter for StoreLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        if desired > self.heap_max {
            return Ok(false);  // Deny growth
        }
        self.current = desired;
        Ok(true)
    }
}
```

### Epoch Interruption

Used for wall-clock timeouts:

```rust
// Set deadline (epochs until interrupt)
store.epoch_deadline_async_yield_and_update(epochs_until_timeout);

// Background task increments epochs
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_millis(10));
    loop {
        interval.tick().await;
        engine.increment_epoch();
    }
});
```

## Execution

### Running _start

```rust
pub fn run(&mut self) -> Result<ExecutionResult> {
    // Get _start function
    let start = self.instance
        .get_func(&mut self.store, "_start")
        .ok_or(Error::FunctionNotFound("_start".into()))?;

    // Execute
    let result = start.call(&mut self.store, &[], &mut []);

    match result {
        Ok(()) => Ok(ExecutionResult {
            exit_code: 0,
            stdout: self.capture_stdout(),
            stderr: self.capture_stderr(),
            fuel_consumed: self.get_fuel_consumed(),
        }),
        Err(trap) => {
            // Check for timeout
            if trap.trap_code() == Some(TrapCode::Interrupt) {
                return Err(Error::Timeout(self.config.wall_time_limit.unwrap()));
            }

            // Check for fuel exhaustion
            if trap.trap_code() == Some(TrapCode::OutOfFuel) {
                return Err(Error::FuelExhausted { limit: self.fuel_limit });
            }

            Err(Error::Execution(trap.to_string()))
        }
    }
}
```

### Calling Arbitrary Functions

```rust
pub fn call(
    &mut self,
    name: &str,
    args: &[Val],
) -> Result<Vec<Val>> {
    let func = self.instance
        .get_func(&mut self.store, name)
        .ok_or(Error::FunctionNotFound(name.into()))?;

    let func_type = func.ty(&self.store);
    let results_len = func_type.results().len();
    let mut results = vec![Val::I32(0); results_len];

    func.call(&mut self.store, args, &mut results)?;

    Ok(results)
}
```

## Capture Streams

### Implementation

```rust
pub struct CaptureStream {
    buffer: Arc<Mutex<Vec<u8>>>,
    meter: Arc<ResourceMeter>,
}

impl WasiOutputStream for CaptureStream {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        // Check I/O limit
        self.meter.record_write(data.len() as u64)?;

        // Capture output
        self.buffer.lock().unwrap().extend_from_slice(data);

        Ok(data.len())
    }
}
```

## Performance Considerations

### Module Caching

- Compilation is expensive (~10-100ms for large modules)
- Cache by content hash (SHA-256)
- Shared across sandboxes

### Instance Reuse

- Currently: new instance per execution
- Future: instance pooling for warm starts

### JIT vs AOT

- Default: JIT compilation on first use
- Consider: AOT compilation for known modules

## See Also

- [Wasmtime Documentation](https://docs.wasmtime.dev/)
- [Architecture](./architecture.md)
