# Error Catalog

Complete reference for all Isolate error types, their causes, and how to resolve them.

Every error includes a programmatic `suggestion()` method that returns actionable advice.
Use `error.suggestion()` in your code to surface these hints to users.

## Resource Limit Errors

These errors indicate the sandbox exceeded its configured resource constraints.
Check with `error.is_resource_limit()`.

### `FuelExhausted`

**Message:** `CPU fuel exhausted (limit: {limit} units)`

**Cause:** The WASM module consumed more CPU instructions than the configured fuel limit.

**Fix:**
- Increase the fuel limit: `.fuel(10_000_000)`
- Optimize the WASM module to use fewer instructions
- Check for infinite loops or unnecessary computation

**Example:**
```rust
match sandbox.run(&[]).await {
    Err(Error::FuelExhausted { limit }) => {
        eprintln!("Module used all {} fuel units", limit);
    }
    _ => {}
}
```

### `MemoryLimitExceeded`

**Message:** `Memory limit exceeded (limit: {limit} bytes, requested: {requested} bytes)`

**Cause:** The WASM module attempted to allocate more memory than allowed.

**Fix:**
- Increase the memory limit: `.memory_limit(256 * 1024 * 1024)` (256MB)
- Check for memory leaks in the WASM module
- Use a profiler to identify high-memory operations

### `Timeout`

**Message:** `Execution timed out after {duration}`

**Cause:** The WASM module did not complete within the wall-clock time limit.

**Fix:**
- Increase the timeout: `.wall_time_limit(Duration::from_secs(60))`
- Add fuel limits to catch infinite loops earlier: `.fuel(5_000_000)`
- Optimize the WASM module

## Capability Errors

These errors indicate the sandbox tried to perform an operation it wasn't granted permission for.
Check with `error.is_capability_error()`.

### `CapabilityDenied`

**Message:** `Capability not granted: {capability}`

**Cause:** The WASM module attempted to use a WASI function (stdout, filesystem, network, etc.) that requires an explicit capability grant.

**Fix by capability type:**

| Capability Denied | Add to Config |
|-------------------|---------------|
| `Stdio(Stdout)` | `.capability(Capability::stdout())` |
| `Stdio(Stderr)` | `.capability(Capability::stderr())` |
| `Stdio(Stdin)` | `.capability(Capability::stdin())` |
| `Filesystem(ReadOnly(path))` | `.capability(Capability::filesystem_read(path))` |
| `Filesystem(ReadWrite(path))` | `.capability(Capability::filesystem_write(path))` |
| `Network(HttpClient(_))` | `.capability(Capability::http_client(vec!["host"]))` |
| `Network(DnsResolve)` | `.capability(Capability::dns_resolve())` |
| `Time(SystemClock)` | `.capability(Capability::system_clock())` |
| `Random(Secure)` | `.capability(Capability::secure_random())` |

### `FilesystemAccessDenied`

**Message:** `Filesystem access denied: {path}`

**Cause:** The module tried to access a file path that isn't covered by any filesystem capability.

**Fix:** Grant read or write access to the required directory:
```rust
.capability(Capability::filesystem_read("/data"))
.capability(Capability::filesystem_write("/tmp"))
```

### `NetworkAccessDenied`

**Message:** `Network access denied: {host}`

**Cause:** The module tried to connect to a host not covered by any network capability.

**Fix:**
```rust
.capability(Capability::http_client(vec!["api.example.com"]))
```

## Configuration Errors

### `InvalidConfig`

**Message:** `Invalid configuration: {details}`

**Cause:** The sandbox configuration has invalid or conflicting settings.

**Fix:** Review the configuration builder. Common issues:
- Memory limit too low (minimum is typically 64KB)
- Missing module (must call `.module()` before `.build()`)
- Conflicting capability grants

### `InvalidCapability`

**Message:** `Invalid capability configuration: {details}`

**Cause:** A capability was configured with invalid parameters.

**Fix:** Check capability syntax. Use `Capability::stdout()` factory methods instead of constructing variants directly.

## Module Errors

### `Compilation`

**Message:** `WASM compilation error: {details}`

**Cause:** The provided bytes are not a valid WebAssembly module.

**Fix:**
- Verify the file starts with the WASM magic bytes (`\0asm`)
- Ensure the module was compiled for WASI (not browser JavaScript)
- Check that the file isn't corrupted or truncated

### `Instantiation`

**Message:** `WASM instantiation error: {details}`

**Cause:** The compiled module could not be instantiated (missing imports, incompatible WASI version, etc.).

**Fix:**
- Ensure the module uses WASI Preview 1 imports
- Check that memory limits are sufficient for the module's initial memory pages
- Verify all required host functions are available

### `FunctionNotFound`

**Message:** `Function not found: {name}`

**Cause:** The requested entry point function doesn't exist in the module's exports.

**Fix:**
- Use `_start` as the entry point for WASI modules (this is the default)
- List available exports: `isolate info module.wasm --exports`

### `InvalidSignature`

**Message:** `Invalid function signature for '{name}': expected {expected}, got {actual}`

**Cause:** The function exists but has the wrong parameter/return types.

**Fix:** WASI `_start` should have signature `() -> ()`. Reactor modules use `(i32, i32) -> i32`.

### `ModuleValidation`

**Message:** `Module validation failed: {details}`

**Cause:** The module failed Wasmtime's validation checks.

**Fix:** Recompile the module with a supported toolchain (Rust `wasm32-wasi`, Emscripten, etc.).

## Runtime Errors

### `Execution`

**Message:** `Execution error: {details}`

**Cause:** An error occurred during WASM execution (trap, unreachable instruction, etc.).

**Fix:** Enable debug logging (`-l debug`) to see the full stack trace. Common causes:
- Division by zero, stack overflow, out-of-bounds memory access
- Explicit `unreachable` instruction (often from `panic!()` in Rust)

### `InvalidState`

**Message:** `Invalid sandbox state: expected {expected}, got {actual}`

**Cause:** An operation was called on a sandbox in the wrong lifecycle state.

**Fix:** Ensure operations follow the lifecycle: `Create → Ready → Running → Terminated`.
A terminated sandbox cannot be run again.

### `Engine`

**Message:** `Internal engine error: {details}`

**Cause:** An internal error in the Wasmtime engine or Isolate runtime.

**Fix:** This is likely a bug. Please [report it](https://github.com/josedab/isolate/issues) with the full error message.

## Infrastructure Errors

### `PoolExhausted`

**Message:** `Warm pool exhausted, no available sandboxes`

**Cause:** All pre-warmed sandboxes are currently in use.

**Fix:** Wait for running sandboxes to complete, or increase the pool size.

### `Io`

**Message:** `I/O error: {details}`

**Cause:** A system I/O operation failed.

**Fix:** Check file permissions, disk space, and that paths exist. Ensure filesystem capabilities are granted.

### `Http`

**Message:** `HTTP error: {details}`

**Cause:** An HTTP request failed.

**Fix:** Check network connectivity, URL correctness, and that `--cap-http` includes the target host.

## Using Error Methods

```rust
use isolate_core::Error;

match result {
    Err(ref e) if e.is_resource_limit() => {
        eprintln!("Resource limit hit: {}", e);
        if let Some(suggestion) = e.suggestion() {
            eprintln!("Suggestion: {}", suggestion);
        }
    }
    Err(ref e) if e.is_capability_error() => {
        eprintln!("Permission denied: {}", e);
        if let Some(suggestion) = e.suggestion() {
            eprintln!("Fix: {}", suggestion);
        }
    }
    Err(e) => eprintln!("Error: {}", e),
    Ok(output) => { /* success */ }
}
```
