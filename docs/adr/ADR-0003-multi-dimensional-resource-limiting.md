# ADR-0003: Multi-Dimensional Resource Limiting

## Status

Accepted

## Context

Untrusted code can abuse system resources in multiple ways: consuming excessive memory, spinning in infinite loops, flooding I/O channels, or simply running too long. A single resource limit (e.g., just memory) is insufficient because:

- CPU-intensive code can starve other sandboxes
- I/O-heavy code can saturate disk or network
- Long-running code can hold resources indefinitely
- Memory limits alone don't prevent computational DoS

We needed a comprehensive resource limiting strategy that addresses all dimensions of resource consumption while remaining practical to configure and enforce.

## Decision

We implemented **multi-dimensional resource limiting** with independent limits for memory, CPU, I/O, and time.

### Resource Dimensions

```rust
pub struct ResourceLimits {
    pub memory: MemoryLimits,  // heap, stack, instances, tables
    pub cpu: CpuLimits,        // fuel, cpu_time
    pub io: IoLimits,          // read_bytes, write_bytes, ops_per_second
    pub time: TimeLimits,      // wall_time, cpu_time (redundant for clarity)
}
```

### Memory Limits
- **max_heap**: Maximum linear memory (enforced by Wasmtime's StoreLimits)
- **max_stack**: Maximum stack size for call depth limiting
- **max_instances**: Maximum WASM instances (for component model)
- **max_tables**: Maximum table elements

### CPU Limits
- **fuel**: Instruction count limit (1 fuel ≈ 1 instruction)
- **cpu_time**: Actual CPU time consumed (measured post-execution)

### I/O Limits
- **max_read_bytes**: Total bytes readable from stdin/files
- **max_write_bytes**: Total bytes writable to stdout/stderr/files
- **max_io_ops_per_second**: Rate limiting for I/O operations

### Time Limits
- **wall_time**: Maximum elapsed time (enforced via epoch interruption)
- **cpu_time**: Maximum CPU time (measured, not enforced in real-time)

### ResourceMeter

Thread-safe metering with atomic counters for hot paths:

```rust
pub struct ResourceMeter {
    limits: ResourceLimits,
    fuel_consumed: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    io_ops: AtomicU64,
    state: Mutex<MeterState>,  // peak_memory, cpu_time, etc.
}
```

### Preset Profiles

```rust
// For untrusted user code
ResourceLimits::restrictive()  // 64MB heap, 1M fuel, 30s wall time

// For trusted internal code
ResourceLimits::permissive()   // 4GB heap, unlimited fuel, 1hr wall time
```

## Consequences

### Positive

- **Comprehensive protection**: No single resource abuse vector can compromise the system
- **Independent enforcement**: Each dimension fails independently with clear errors
- **Atomic metering**: Lock-free counters minimize overhead on hot paths
- **Observable**: ResourceUsage returned after execution shows actual consumption
- **Composable presets**: Users can start with presets and customize specific limits

### Negative

- **Configuration surface**: Many parameters to tune, though presets help
- **Imperfect CPU metering**: Fuel approximates instructions but isn't perfectly correlated with wall time
- **I/O metering overhead**: Every read/write operation increments atomic counters

### Implications

- StoreLimits integration with Wasmtime for memory enforcement
- CaptureStream and BufferedStdin integrate metering into I/O paths
- Epoch ticker background task checks wall_time limits
- ResourceUsage summary returned to callers for billing/accounting
