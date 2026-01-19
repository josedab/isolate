# Add WASI Call Tracing

## Task Description

Add optional tracing/logging for WASI calls made by sandboxed modules.
This aids debugging and helps users understand module behavior.

## Background Context

When debugging issues with sandboxed modules, it's helpful to see what WASI
calls are being made. This feature would log calls like:
- fd_write to stdout/stderr
- path_open for file access
- clock_time_get for time queries

The tracing should be:
- Optional (disabled by default)
- Configurable via SandboxConfig
- Integrated with the `tracing` crate

## Files to Modify

- `isolate-core/src/config.rs` - Add tracing option to config
- `isolate-core/src/engine/wasm.rs` - Add trace points
- `isolate-core/src/engine/host.rs` - Add trace points if needed

## Acceptance Criteria

- [ ] New config option: `trace_wasi_calls(bool)`
- [ ] Tracing uses the `tracing` crate at DEBUG level
- [ ] Traces include: call name, arguments (redacted if sensitive), result
- [ ] No performance impact when disabled
- [ ] Documentation added for the new option
- [ ] Example in docs showing how to enable tracing

## Example Output

```
DEBUG isolate::wasi: fd_write fd=1 data_len=14
DEBUG isolate::wasi: fd_write -> Ok(14)
DEBUG isolate::wasi: clock_time_get clock_id=realtime
DEBUG isolate::wasi: clock_time_get -> Ok(1642000000000000000)
DEBUG isolate::wasi: path_open dir_fd=3 path="/data/input.txt"
DEBUG isolate::wasi: path_open -> Ok(4)
```

## Example API

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .trace_wasi_calls(true)  // Enable WASI tracing
    .build()?;
```

## Helpful Resources

- tracing crate: https://docs.rs/tracing/
- WASI API documentation: https://github.com/WebAssembly/WASI
- Existing tracing usage in the codebase

## Estimated Difficulty

Medium (1-4 hours)
