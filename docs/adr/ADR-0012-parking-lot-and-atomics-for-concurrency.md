# ADR-0012: Parking-lot and Atomics for Concurrency

## Status

Accepted

## Context

A high-performance sandbox runtime handles many concurrent operations:

- Multiple sandboxes executing simultaneously
- Shared module cache accessed by many threads
- Resource meters tracking I/O across async tasks
- Metrics being updated from various contexts

The standard library's synchronization primitives (`std::sync::Mutex`, `RwLock`) have known limitations:

- Mutex poisoning requires `unwrap()` everywhere
- RwLock can have writer starvation
- Not optimized for short critical sections
- No async-aware variants in std

We needed a concurrency strategy that:

- Minimizes lock contention on hot paths
- Avoids mutex poisoning boilerplate
- Provides both sync and async-compatible primitives
- Scales to many concurrent sandboxes

## Decision

We adopted a **hybrid concurrency approach** using `parking_lot` for synchronous locks, `DashMap` for concurrent hashmaps, and `std::sync::atomic` for hot-path counters.

### Parking-lot for Synchronous Locks

```rust
use parking_lot::RwLock;

pub struct TimingStats {
    inner: Arc<RwLock<TimingStatsInner>>,
}

impl TimingStats {
    pub fn record(&self, duration: Duration) {
        let mut inner = self.inner.write();  // No unwrap needed!
        inner.count += 1;
        inner.sum += duration;
        // ...
    }

    pub fn count(&self) -> u64 {
        self.inner.read().count  // Reader doesn't block other readers
    }
}
```

Benefits over std:
- No poisoning (no panics to propagate)
- Smaller (1 byte vs 40+ bytes for std Mutex)
- Faster for uncontended cases
- Fair RwLock prevents writer starvation

### DashMap for Concurrent HashMaps

```rust
use dashmap::DashMap;

pub struct WasmEngine {
    engine: Engine,
    module_cache: Arc<DashMap<ModuleHash, Module>>,
}

impl WasmEngine {
    pub fn compile(&self, module: &WasmModule) -> Result<CompiledModule> {
        let hash = module.hash().clone();

        // Lock-free read
        if let Some(cached) = self.module_cache.get(&hash) {
            return Ok(CompiledModule { module: cached.clone(), hash });
        }

        // Compile and insert (sharded locking)
        let compiled = Module::new(&self.engine, module.bytes())?;
        self.module_cache.insert(hash.clone(), compiled.clone());
        Ok(CompiledModule { module: compiled, hash })
    }
}
```

DashMap provides:
- Sharded internal structure (16 shards by default)
- Per-shard locking minimizes contention
- Familiar HashMap API
- Entry API for atomic operations

### Atomics for Hot Paths

Resource metering uses atomics to avoid locks entirely:

```rust
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub struct ResourceMeter {
    limits: ResourceLimits,
    fuel_consumed: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    io_ops: AtomicU64,
}

impl ResourceMeter {
    pub fn record_read(&self, bytes: usize) -> Result<()> {
        let new_total = self.bytes_read.fetch_add(bytes as u64, Ordering::Relaxed) + bytes as u64;

        if let Some(limit) = self.limits.io.read_bytes {
            if new_total > limit {
                return Err(Error::IoLimitExceeded { limit, actual: new_total });
            }
        }
        Ok(())
    }

    pub fn usage(&self) -> ResourceUsage {
        ResourceUsage {
            fuel_consumed: self.fuel_consumed.load(Ordering::Relaxed),
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            // ...
        }
    }
}
```

Ordering choices:
- `Relaxed`: Sufficient for counters where exact ordering doesn't matter
- `SeqCst`: Used for version numbers where ordering is critical
- `Acquire/Release`: Used for publish-subscribe patterns

### CoW Store with Atomics

```rust
pub struct CowMemoryStore {
    pages: DashMap<PageHash, Arc<PageData>>,
    total_pages: AtomicUsize,
    bytes_saved: AtomicU64,
}

struct PageData {
    data: Vec<u8>,
    ref_count: AtomicUsize,  // Manual reference counting
}

impl PageData {
    fn increment(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
    }

    fn decrement(&self) -> usize {
        self.ref_count.fetch_sub(1, Ordering::Relaxed)
    }
}
```

## Consequences

### Positive

- **No poisoning**: parking_lot locks don't poison, cleaner code
- **Lower contention**: DashMap sharding and atomics reduce lock conflicts
- **Smaller footprint**: parking_lot primitives are more compact
- **Consistent performance**: Fair RwLock prevents latency spikes
- **Zero-cost counters**: Atomic operations are essentially free

### Negative

- **Additional dependencies**: parking_lot and dashmap add to dependency tree
- **Memory ordering complexity**: Choosing correct `Ordering` requires care
- **No async support**: parking_lot blocks; need tokio::sync for async contexts
- **Debugging difficulty**: Lock-free code is harder to reason about

### Implications

- Use parking_lot for short critical sections in sync code
- Use tokio::sync::Mutex/RwLock for async code that holds locks across await points
- Use DashMap when concurrent HashMap access is needed
- Use atomics for simple counters and flags on hot paths
- Default to `Ordering::Relaxed` for counters, escalate only when needed
- Document memory ordering requirements in comments
