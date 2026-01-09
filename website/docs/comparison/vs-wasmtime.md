---
sidebar_position: 2
---

# Isolate vs Wasmtime

Isolate is built on [Wasmtime](https://wasmtime.dev/), but adds significant functionality. This page explains the differences and when to use each.

## Overview

| Aspect | Wasmtime | Isolate |
|--------|----------|---------|
| Purpose | WASM runtime | Secure sandbox runtime |
| Abstraction | Low-level | High-level |
| Security model | Minimal | Capability-based |
| Resource control | Manual setup | Built-in |
| Timeout handling | Manual | Automatic |
| I/O capture | Manual | Built-in |
| Module caching | Manual | Automatic |
| Metrics | None | Prometheus |

## Wasmtime: The Foundation

Wasmtime provides:
- WASM compilation and execution
- WASI implementation
- Basic resource limits (via `StoreLimits`)
- Epoch-based interruption (manual setup)

```rust
// Using Wasmtime directly
use wasmtime::*;
use wasmtime_wasi::*;

fn run_with_wasmtime(wasm_bytes: &[u8]) -> Result<()> {
    // 1. Create engine with configuration
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    let engine = Engine::new(&config)?;

    // 2. Compile module (no caching)
    let module = Module::new(&engine, wasm_bytes)?;

    // 3. Create WASI context manually
    let wasi = WasiCtxBuilder::new()
        .inherit_stdout()
        .inherit_stderr()
        .build();

    // 4. Create store with data
    let mut store = Store::new(&engine, wasi);
    store.add_fuel(1_000_000)?;

    // 5. Set up timeout manually
    store.epoch_deadline_trap();
    store.set_epoch_deadline(100);

    // 6. Spawn background task for epochs
    let engine_clone = engine.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(10));
            engine_clone.increment_epoch();
        }
    });

    // 7. Create linker and instantiate
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;
    let instance = linker.instantiate(&mut store, &module)?;

    // 8. Call function
    let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
    start.call(&mut store, ())?;

    // 9. Manually track fuel usage
    let fuel_consumed = store.fuel_consumed();

    Ok(())
}
```

**Problems with direct Wasmtime usage:**
- Lots of boilerplate
- Manual timeout management
- No built-in capability system
- No I/O capture
- No module caching
- No metrics

## Isolate: The Solution

Isolate wraps Wasmtime and provides:

```rust
// Using Isolate
use isolate_core::*;

async fn run_with_isolate(wasm_bytes: &[u8]) -> Result<Output> {
    let config = SandboxConfig::builder()
        .module(wasm_bytes)?
        .fuel(1_000_000)
        .wall_time_limit(Duration::from_secs(5))
        .capability(Capability::stdout())
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    sandbox.run(&[]).await
}
```

**What Isolate handles automatically:**
- Module compilation and caching
- Timeout with epoch ticker
- I/O capture (stdout, stderr)
- Capability enforcement
- Resource metering
- Prometheus metrics
- Clean error handling

## Feature Comparison

### Security Model

**Wasmtime:**
```rust
// WASI context allows everything by default
let wasi = WasiCtxBuilder::new()
    .inherit_stdio()      // Full stdio access
    .preopened_dir(dir)?  // Manual directory setup
    .build();
```

**Isolate:**
```rust
// Default deny, explicit grants
let config = SandboxConfig::builder()
    .module(&wasm)?
    .capability(Capability::stdout())  // Only stdout
    // filesystem_read not granted = no file access
    .build()?;
```

### Resource Control

**Wasmtime:**
```rust
// Manual fuel setup
store.add_fuel(1_000_000)?;

// Manual memory limits via StoreLimits
struct MyLimiter { /* ... */ }
impl ResourceLimiter for MyLimiter {
    fn memory_growing(&mut self, current: usize, desired: usize, max: Option<usize>) -> bool {
        desired <= self.max_memory
    }
}
```

**Isolate:**
```rust
// Declarative limits
let config = SandboxConfig::builder()
    .module(&wasm)?
    .fuel(1_000_000)
    .memory_limit(64 * 1024 * 1024)
    .stdout_limit(1024 * 1024)
    .build()?;
```

### Timeout Handling

**Wasmtime:**
```rust
// Manual epoch management
store.epoch_deadline_trap();
store.set_epoch_deadline(100);

// Must spawn and manage ticker thread
let handle = std::thread::spawn(move || {
    loop {
        std::thread::sleep(Duration::from_millis(10));
        engine.increment_epoch();
    }
});

// Must cancel ticker after execution
handle.abort();
```

**Isolate:**
```rust
// Automatic timeout
let config = SandboxConfig::builder()
    .module(&wasm)?
    .wall_time_limit(Duration::from_secs(5))
    .build()?;

// Ticker managed automatically during execution
let output = sandbox.run(&[]).await?;
```

### Module Caching

**Wasmtime:**
```rust
// No built-in caching, must implement yourself
let module = Module::new(&engine, &wasm_bytes)?;

// For caching, you'd need:
use std::collections::HashMap;
struct ModuleCache {
    cache: HashMap<Vec<u8>, Module>,
}
```

**Isolate:**
```rust
// Automatic caching by module hash
let sandbox1 = Sandbox::create(config1).await?;  // Compiles
let sandbox2 = Sandbox::create(config2).await?;  // Cache hit!
```

### I/O Capture

**Wasmtime:**
```rust
// Custom stream implementation required
struct CaptureStream {
    buffer: Vec<u8>,
}

impl HostOutputStream for CaptureStream {
    fn write(&mut self, bytes: Bytes) -> Result<()> {
        self.buffer.extend_from_slice(&bytes);
        Ok(())
    }
    // ... more methods
}
```

**Isolate:**
```rust
// Automatic capture
let output = sandbox.run(&[]).await?;
println!("stdout: {}", output.stdout_str());
println!("stderr: {}", output.stderr_str());
```

### Metrics

**Wasmtime:**
- No built-in metrics
- Must implement custom tracking

**Isolate:**
```rust
// Built-in Prometheus metrics
// sandbox_executions_total
// sandbox_execution_duration_seconds
// sandbox_fuel_consumed
// sandbox_memory_bytes
// capability_grants_total
// capability_denials_total
```

## When to Use Wasmtime Directly

Use Wasmtime directly when you need:

1. **Maximum control** - Custom WASI implementations
2. **Non-standard embeddings** - Browser, embedded systems
3. **Component model** - WASI preview2 (not yet in Isolate)
4. **Custom linking** - Host functions beyond WASI

```rust
// Example: Custom host function
let mut linker = Linker::new(&engine);
linker.func_wrap("my_module", "custom_log", |caller: Caller<'_, _>, ptr: i32, len: i32| {
    // Custom implementation
})?;
```

## When to Use Isolate

Use Isolate when you need:

1. **Quick integration** - High-level API
2. **Security by default** - Capability-based model
3. **Production features** - Metrics, logging, resource control
4. **Timeout safety** - Automatic epoch management
5. **Multiple sandboxes** - Shared engine with caching

## Migration from Wasmtime

If you're already using Wasmtime, migration is straightforward:

```rust
// Before: Wasmtime
let engine = Engine::new(&config)?;
let module = Module::new(&engine, &wasm_bytes)?;
let mut store = Store::new(&engine, wasi_ctx);
let instance = linker.instantiate(&mut store, &module)?;
let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
start.call(&mut store, ())?;

// After: Isolate
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::stdout())
    .build()?;
let mut sandbox = Sandbox::create(config).await?;
let output = sandbox.run(&[]).await?;
```

## Summary

| Use Case | Recommendation |
|----------|----------------|
| Running untrusted code | **Isolate** |
| Custom WASI implementation | Wasmtime |
| Quick sandbox setup | **Isolate** |
| Component model (preview2) | Wasmtime |
| Production deployment | **Isolate** |
| Maximum flexibility | Wasmtime |
| Security-focused | **Isolate** |

Isolate is the right choice for most sandbox use cases. Use Wasmtime directly only when you need features Isolate doesn't expose.
