# ADR-0004: Shared Engine with Module Caching

## Status

Accepted

## Context

WASM module compilation is expensive (100-500ms for moderate modules). Creating a new Wasmtime engine for each sandbox would waste memory and CPU. However, sharing state between sandboxes raises isolation concerns. We needed an architecture that:

- Amortizes compilation cost across multiple sandbox instances
- Maintains strict isolation between sandbox executions
- Achieves <5ms cold start for previously-seen modules
- Manages memory efficiently as module count grows

## Decision

We implemented a **shared WasmEngine** with compiled module caching using DashMap for thread-safe concurrent access.

### Architecture

```rust
pub struct WasmEngine {
    engine: Arc<Engine>,  // Wasmtime engine (immutable config)
    module_cache: Arc<DashMap<ModuleHash, Module>>,  // Compiled modules
    config: EngineConfig,
}

pub struct EngineConfig {
    pub cache_size: usize,        // Default: 100 modules
    pub epoch_tick_interval: Duration,  // Default: 10ms
}
```

### Module Hash

Modules are identified by SHA256 hash of their bytes:

```rust
pub struct ModuleHash(pub String);

impl ModuleHash {
    pub fn compute(bytes: &[u8]) -> Self {
        let hash = Sha256::digest(bytes);
        Self(hex::encode(hash))
    }
}
```

### Caching Strategy

1. **On module load**: Compute hash, check cache
2. **Cache miss**: Compile module, store in cache
3. **Cache hit**: Return cloned module reference (Module is Arc-wrapped internally)
4. **Eviction**: LRU when cache exceeds `cache_size`

### Isolation Guarantees

- Engine configuration is immutable after creation
- Each sandbox gets its own `Store` (Wasmtime's per-instance state)
- Module compilation is deterministic given same bytes
- No mutable state shared between sandbox executions

### Cold Start Performance

```
First load (cache miss):  ~200ms (compilation)
Subsequent loads (hit):   ~2ms (hash + cache lookup)
Target cold start:        <5ms
```

## Consequences

### Positive

- **Fast warm starts**: Cache hits skip compilation entirely
- **Memory efficiency**: Compiled code shared across sandboxes
- **Thread-safe**: DashMap enables concurrent module loading without locks
- **Predictable hashing**: Same module bytes always produce same hash
- **Bounded memory**: Cache size limit prevents unbounded growth

### Negative

- **First-load latency**: Initial compilation still takes 100-500ms
- **Cache invalidation**: No automatic invalidation; relies on hash uniqueness
- **Memory overhead**: Compiled modules larger than source (~3-5x)
- **DashMap overhead**: Some overhead vs single-threaded HashMap

### Implications

- Engine creation should happen once at application startup
- Module bytes should be hashed before checking availability
- Precompilation support available for AOT scenarios
- Cache statistics available for monitoring hit rates
