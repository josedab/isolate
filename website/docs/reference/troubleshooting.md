---
sidebar_position: 4
---

# Troubleshooting

Common issues and their solutions when working with Isolate.

## Module Issues

### "Invalid WASM magic number"

**Problem:** The file doesn't start with the WASM magic number (`\0asm`).

**Solutions:**
- Verify you're loading a compiled `.wasm` file, not source code
- Check the file wasn't corrupted during transfer
- Ensure you're reading the file as binary, not text

```rust
// Correct: binary read
let wasm = std::fs::read("module.wasm")?;

// Wrong: text read (may corrupt binary)
let wasm = std::fs::read_to_string("module.wasm")?;
```

### "WASM module too small"

**Problem:** The module is less than 8 bytes.

**Solutions:**
- Check the file exists and is not empty
- Verify the path is correct
- Ensure the WASM was compiled successfully

### "Module validation failed"

**Problem:** The WASM bytecode is invalid.

**Solutions:**
- Recompile the module
- Check for compiler bugs or unsupported features
- Use `wasmtime validate module.wasm` to get detailed errors

## Capability Issues

### "Capability not granted: stdout"

**Problem:** Module tried to write to stdout without permission.

**Solution:** Grant the stdout capability:

```rust
.capability(Capability::stdout())
```

### "Capability not granted: filesystem_read"

**Problem:** Module tried to read a file without permission.

**Solution:** Grant filesystem read access to the specific path:

```rust
.capability(Capability::filesystem_read("/path/to/file"))
```

:::tip
Paths must be absolute and exact. `/data` does not grant access to `/data-backup`.
:::

### "Capability not granted: clock"

**Problem:** Module tried to access system time.

**Solution:** Grant clock capability:

```rust
.capability(Capability::system_clock())
```

## Resource Limit Issues

### "Fuel exhausted"

**Problem:** Module consumed all allocated CPU fuel.

**Solutions:**
- Increase fuel limit: `.fuel(10_000_000)`
- Optimize the WASM module
- Check for infinite loops

```rust
// Generous fuel allocation
.fuel(100_000_000)  // 100M instructions
```

### "Memory limit exceeded"

**Problem:** Module tried to allocate more memory than allowed.

**Solutions:**
- Increase memory limit: `.memory_limit(256 * 1024 * 1024)`
- Check for memory leaks in the module
- Profile memory usage

### "Timeout"

**Problem:** Execution exceeded wall clock limit.

**Solutions:**
- Increase timeout: `.wall_time_limit(Duration::from_secs(60))`
- Optimize the module
- Check for I/O blocking

## State Issues

### "Invalid state: expected Ready, got Terminated"

**Problem:** Trying to run a sandbox that has already terminated.

**Solution:** Create a new sandbox for each execution:

```rust
// Each run needs a new sandbox
let mut sandbox = Sandbox::create(config.clone()).await?;
let output = sandbox.run(&[]).await?;
```

### "Invalid state: expected Ready, got Running"

**Problem:** Trying to run while already running.

**Solution:** Wait for current execution to complete:

```rust
let output = sandbox.run(&[]).await?;  // Wait for completion
```

## Performance Issues

### Slow cold starts

**Problem:** Sandbox creation is slow.

**Solutions:**
1. Share the WasmEngine across sandboxes:

```rust
let engine = Arc::new(WasmEngine::new()?);

// Reuse engine for multiple sandboxes
let sandbox1 = Sandbox::create_with_engine(config1, engine.clone()).await?;
let sandbox2 = Sandbox::create_with_engine(config2, engine.clone()).await?;
```

2. Module is already cached after first creation
3. Consider pre-compiling modules

### High memory usage

**Problem:** Process using too much memory.

**Solutions:**
- Reduce `memory_limit` per sandbox
- Limit concurrent sandboxes
- Clear engine cache periodically

```rust
engine.clear_cache();
```

### Epoch ticker overhead

**Problem:** Timeout monitoring consuming CPU.

**Solution:** The epoch ticker only runs during execution. If you don't need wall clock timeouts, don't set `wall_time_limit`.

## Debugging

### Enable debug logging

```rust
tracing_subscriber::fmt()
    .with_env_filter("isolate=debug")
    .init();
```

### Trace capability checks

```bash
RUST_LOG=isolate::capability::audit=trace cargo run
```

### Inspect module

```bash
# Using wasmtime CLI
wasmtime explore module.wasm

# Using wasm-tools
wasm-tools print module.wasm
wasm-tools validate module.wasm
```

### Check resource usage

```rust
let output = sandbox.run(&[]).await?;
println!("Fuel: {:?}", output.resource_usage.fuel_consumed);
println!("Memory: {} bytes", output.resource_usage.memory_peak);
println!("Duration: {:?}", output.duration);
```

## Common Patterns

### Testing if capability is needed

```rust
// Start with no capabilities, add as errors occur
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // .capability(...)  // Add based on errors
    .build()?;

match sandbox.run(&[]).await {
    Err(Error::CapabilityDenied(cap)) => {
        println!("Module needs capability: {}", cap);
    }
    // ...
}
```

### Finding appropriate limits

```rust
// Start with generous limits, then tighten
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .memory_limit(1024 * 1024 * 1024)  // 1GB
    .fuel(u64::MAX)
    .capability(Capability::stdout())
    .build()?;

let output = sandbox.run(&[]).await?;
let usage = &output.resource_usage;

println!("Actual memory: {} bytes", usage.memory_peak);
println!("Actual fuel: {:?}", usage.fuel_consumed);

// Now set limits based on actual usage + margin
```

## Getting Help

If you're still stuck:

1. Check [GitHub Issues](https://github.com/josedab/isolate/issues) for similar problems
2. Search [GitHub Discussions](https://github.com/josedab/isolate/discussions)
3. Open a new issue with:
   - Isolate version
   - Rust version
   - Minimal reproduction
   - Full error message

## Frequently Asked Questions

### General

#### What languages can I run in Isolate?

Any language that compiles to WebAssembly:
- **Rust** - First-class support via `wasm32-wasi` target
- **C/C++** - Via Emscripten or wasi-sdk
- **Go** - Via TinyGo (recommended) or standard Go
- **AssemblyScript** - TypeScript-like syntax
- **Python** - Via MicroPython or Pyodide
- **JavaScript** - Via QuickJS or other engines compiled to WASM

#### Is Isolate production-ready?

Isolate is at version 0.1.x and suitable for production use with caveats:
- The core API is stable
- Some features are experimental (GPU, mesh, enclaves)
- Performance characteristics are well-tested
- Used in production by early adopters

#### How does Isolate compare to containers?

| Aspect | Isolate | Containers |
|--------|---------|------------|
| Startup time | &lt;5ms | ~500ms |
| Memory overhead | ~1MB | ~50MB |
| Isolation | WASM sandbox | OS namespaces |
| Language support | WASM-compiled | Any |
| Ecosystem | Growing | Mature |

Isolate is better for running untrusted code snippets; containers are better for running full applications.

#### Can I use Isolate for serverless functions?

Yes, Isolate is designed for serverless use cases:
- Sub-5ms cold starts enable responsive scaling
- Capability-based security prevents lateral movement
- Resource limits prevent noisy neighbors
- Multi-tenant by design

### Security

#### How secure is WASM sandboxing?

WebAssembly provides strong isolation guarantees:
- **Memory safety**: Linear memory model prevents buffer overflows
- **Control flow**: Only valid function calls allowed
- **No syscalls**: WASM cannot directly call the OS
- **Type safety**: Validated bytecode

Isolate adds capability-based security on top for defense-in-depth.

#### Can a WASM module escape the sandbox?

Under normal circumstances, no. WASM modules:
- Cannot access memory outside their linear memory
- Cannot make system calls directly
- Cannot access denied capabilities
- Are subject to resource limits

However, vulnerabilities in Wasmtime or Isolate could theoretically allow escape. We recommend:
- Keeping Isolate updated
- Running with minimal OS privileges
- Using Linux security modules (seccomp, Landlock) for additional isolation

#### Should I trust user-provided WASM modules?

With Isolate, you can safely run untrusted WASM modules if you:
1. Set appropriate resource limits (memory, fuel, time)
2. Grant only necessary capabilities
3. Validate output before using it
4. Monitor for suspicious activity

#### How do I handle sensitive data?

```rust
// Don't expose sensitive data through environment variables
.env("API_KEY", "secret")  // Risky if module is untrusted

// Instead, have the module request data through a controlled channel
.capability(Capability::host_function("get_secret"))  // You control access
```

### Performance

#### Why is my first sandbox slow?

The first sandbox creation includes WASM compilation. Subsequent creations reuse the cached compiled module:

```rust
// First call: ~4ms (includes compilation)
let sandbox1 = Sandbox::create(config.clone()).await?;

// Subsequent calls: <1ms (uses cache)
let sandbox2 = Sandbox::create(config.clone()).await?;
```

#### How do I improve throughput?

1. **Share the engine**: Use `create_with_engine` with a shared `WasmEngine`
2. **Pre-warm cache**: Create a dummy sandbox on startup
3. **Use concurrency**: Sandboxes are independent and can run in parallel
4. **Tune limits**: Higher epoch tick intervals reduce overhead

See [Benchmarks](./benchmarks) for detailed performance data.

#### How much memory does each sandbox use?

- **Base overhead**: ~1.2MB (runtime structures, WASI context)
- **WASM memory**: Up to your configured `memory_limit`
- **Shared**: Engine and compiled modules are shared across sandboxes

With 100 sandboxes sharing an engine, expect ~150MB total (vs. ~500MB without sharing).

### Capabilities

#### What happens if I don't grant any capabilities?

The module runs but cannot:
- Write to stdout/stderr (output is discarded)
- Read from stdin
- Access the filesystem
- Access the network
- Read environment variables
- Get the current time
- Generate random numbers

This is useful for pure computation that only returns via exit code.

#### Can I grant capabilities dynamically?

No, capabilities are fixed at sandbox creation. Design your capability set to cover all operations the module might need:

```rust
// Grant capabilities for all possible operations
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::stdout())
    .capability(Capability::filesystem_read("/data"))
    .capability(Capability::filesystem_read("/config"))
    .build()?;
```

#### How do I know what capabilities a module needs?

1. **Start with none**: Run without capabilities, note errors
2. **Add incrementally**: Grant capabilities as denied errors occur
3. **Document**: Record required capabilities for each module

```rust
// Discovery mode
match sandbox.run(&[]).await {
    Err(Error::CapabilityDenied(cap)) => {
        println!("Module needs: {}", cap);
    }
    // ...
}
```

### Integration

#### Can I call Rust functions from WASM?

Yes, via host functions:

```rust
.capability(Capability::host_function("my_function"))
```

Host function implementation is done at the Wasmtime level. See [Wasmtime documentation](https://docs.wasmtime.dev/) for details.

#### Can I use async in WASM modules?

WASM execution is synchronous. For async patterns:
- Use the `timers` capability for delays
- Design modules to be called repeatedly
- Consider state snapshots for long-running workflows

#### How do I pass complex data to/from modules?

Common patterns:
1. **JSON via stdin/stdout**: Simple, universal
2. **MessagePack**: Efficient binary format
3. **Shared memory**: Advanced, requires coordination

```rust
// JSON pattern
let input = serde_json::to_vec(&data)?;
let output = sandbox.run(&input).await?;
let result: MyOutput = serde_json::from_slice(&output.stdout)?;
```

### Debugging

#### How do I debug a WASM module?

1. **Enable verbose logging**:
   ```bash
   RUST_LOG=isolate=debug cargo run
   ```

2. **Use generous limits during development**:
   ```rust
   .fuel(u64::MAX)
   .memory_limit(1024 * 1024 * 1024)
   ```

3. **Capture stderr for debugging output**:
   ```rust
   .capability(Capability::stderr())
   ```

4. **Inspect WASM with tools**:
   ```bash
   wasm-tools print module.wasm
   wasmtime explore module.wasm
   ```

#### Why does my module run forever?

Check for infinite loops. WASM doesn't have preemption, so:
1. Set a `fuel` limit to stop runaway computation
2. Set a `wall_time_limit` for absolute timeout
3. Audit the module code for unbounded loops

## See Also

- [Error Handling](./errors) - Understanding error types
- [Monitoring](../guides/monitoring) - Debugging with logs and metrics
- [Configuration](./configuration) - Configuration options
- [Benchmarks](./benchmarks) - Performance data
