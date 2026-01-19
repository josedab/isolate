# Troubleshooting

Common issues and their solutions.

## Module Errors

### "WASM module too small"

**Cause:** The input is less than 8 bytes.

**Solution:** Ensure you're loading a valid compiled WASM file, not source code.

```rust
let wasm_bytes = std::fs::read("module.wasm")?;  // Correct
// Not: std::fs::read("module.wat")  // Wrong - this is text format
```

### "Invalid WASM magic number"

**Cause:** The file doesn't start with `\0asm`.

**Solutions:**
1. Verify the file is a compiled `.wasm` file
2. Check for file corruption
3. Ensure the file wasn't truncated during transfer

```bash
# Check the magic number
xxd module.wasm | head -1
# Should show: 00000000: 0061 736d 0100 0000 ...
```

### "Unknown import: wasi_snapshot_preview1::..."

**Cause:** The module requires WASI functions not supported by Isolate.

**Solution:** Isolate supports WASI preview1. Check if your module needs preview2:

```bash
# List imports
wasm-tools print module.wasm | grep import
```

## Capability Errors

### "Capability not granted: stdout"

**Cause:** The module tried to write to stdout without the capability.

**Solution:**
```rust
.capability(Capability::stdout())
```

### "Filesystem access denied"

**Cause:** Trying to access a path not granted in capabilities.

**Solutions:**
1. Grant the specific path:
   ```rust
   .capability(Capability::filesystem_read("/data/file.txt"))
   ```
2. Grant the parent directory:
   ```rust
   .capability(Capability::filesystem_read("/data"))
   ```

## Resource Limit Errors

### "CPU fuel exhausted"

**Cause:** Module exceeded the instruction limit.

**Solutions:**
1. Increase fuel limit:
   ```rust
   .fuel(10_000_000)  // 10 million instructions
   ```
2. Optimize the WASM module
3. Check for infinite loops

### "Memory limit exceeded"

**Cause:** Module tried to allocate more memory than allowed.

**Solutions:**
1. Increase memory limit:
   ```rust
   .memory_limit(256 * 1024 * 1024)  // 256MB
   ```
2. Check for memory leaks in the module
3. Use a smaller dataset

### "Execution timed out"

**Cause:** Module didn't complete within the wall clock limit.

**Solutions:**
1. Increase timeout:
   ```rust
   .wall_time_limit(Duration::from_secs(60))
   ```
2. Check for blocking operations
3. Optimize the module

## Runtime Errors

### "unreachable executed"

**Cause:** The module hit an `unreachable` instruction (often from a panic).

**Solutions:**
1. Check the module's stderr for panic messages
2. Debug the module with more input validation
3. Ensure all required data is provided

### "out of bounds memory access"

**Cause:** The module accessed invalid memory.

**Solutions:**
1. This is usually a bug in the WASM module
2. Check array bounds in the source code
3. Ensure pointers are valid

### "indirect call type mismatch"

**Cause:** Function pointer called with wrong signature.

**Solution:** This is a bug in the WASM module. Check function pointer usage.

## Performance Issues

### Slow sandbox creation

**Causes:**
- Large WASM modules
- No module caching
- Cold JIT compilation

**Solutions:**
1. Use a shared engine:
   ```rust
   let engine = Arc::new(WasmEngine::new()?);
   Sandbox::create_with_engine(config, engine).await?;
   ```
2. Pre-compile modules
3. Use a warm pool

### High memory usage

**Causes:**
- Many concurrent sandboxes
- Large memory limits
- Memory leaks

**Solutions:**
1. Reduce concurrent sandboxes
2. Lower memory limits
3. Terminate sandboxes promptly

## Debugging

### Enable Verbose Logging

```rust
tracing_subscriber::fmt()
    .with_env_filter("isolate=debug")
    .init();
```

### Check Module Imports/Exports

```bash
wasm-tools print module.wasm | grep -E "(import|export)"
```

### Validate Module

```bash
wasm-tools validate module.wasm
```

### Inspect Resource Usage

```rust
let output = sandbox.run(&[]).await?;
println!("Fuel: {:?}", output.resource_usage.fuel_consumed);
println!("Memory: {} bytes", output.resource_usage.memory_peak);
println!("Duration: {:?}", output.duration);
```

## Common Patterns

### Checking Module Validity First

```rust
// Validate before creating sandbox
let module = WasmModule::from_bytes(wasm_bytes)?;
println!("Module hash: {}", module.hash());

// Then create sandbox
let config = SandboxConfig::builder()
    .wasm_module(module)
    .build()?;
```

### Graceful Degradation

```rust
let result = sandbox.run(input).await;

let output = match result {
    Ok(o) => o,
    Err(Error::Timeout(_)) => {
        // Return partial results if available
        return Ok(partial_output());
    }
    Err(e) => return Err(e),
};
```

## Getting Help

If you're still stuck:

1. Check [existing issues](https://github.com/josedab/isolate/issues)
2. Open a [new issue](https://github.com/josedab/isolate/issues/new) with:
   - Isolate version
   - Rust version
   - Minimal reproduction case
   - Full error message
   - Relevant configuration
