# Edge Compute

Deploy WASM functions at CDN edge locations with ultra-low latency.

## Architecture

```
┌────────┐    ┌────────────┐    ┌──────────────┐
│ Client │───▶│ Edge Node  │───▶│ Isolate      │
│        │    │ (PoP)      │    │ + Precompile │
└────────┘    └────────────┘    └──────────────┘
                    │
                    ▼
              ┌────────────┐
              │ Origin     │
              │ (fallback) │
              └────────────┘
```

## Key Design Decisions

### Pre-compilation at Deploy Time
Edge nodes pre-compile WASM modules during deployment, not at request time:

```rust
use isolate_core::coldstart::PrecompileCache;
use isolate_core::engine::WasmEngine;

// Deploy-time: pre-compile and distribute
let engine = WasmEngine::new()?;
let cache = PrecompileCache::new("/edge/cache".into(), 500)?;
let hash = cache.precompile(&engine, &wasm_bytes)?;
```

### Language Profiles for Edge
Use optimized profiles for edge workloads:

```rust
use isolate_core::profile::LanguageProfile;
use isolate_core::SandboxConfig;

let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .apply_profile(LanguageProfile::Rust)  // Minimal footprint
    .wall_time_limit(std::time::Duration::from_millis(50))  // 50ms edge budget
    .memory_limit(8 * 1024 * 1024)  // 8MB for edge
    .build()?;
```

## Performance Targets

| Metric | Target | Approach |
|--------|--------|----------|
| Cold start | <1ms | PrecompileCache |
| P99 latency | <10ms | Rust profile + tight limits |
| Memory per instance | <8MB | Language profile tuning |
| Concurrent functions | 1000+ | Shared WasmEngine + async |
