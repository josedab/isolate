---
sidebar_position: 2
---

# Resource Limits

Resource limits prevent WASM modules from consuming excessive CPU, memory, or I/O resources. They're essential for running untrusted code safely.

## Overview

| Resource | Limit Type | Enforcement |
|----------|------------|-------------|
| Memory | Heap size, Stack size | Wasmtime StoreLimits |
| CPU | Fuel (instructions), Time | Fuel metering, Epoch interruption |
| I/O | Read bytes, Write bytes | Stream metering |
| Time | Wall clock, CPU time | Async timeout, Fuel |

## Memory Limits

### Heap Memory

```rust
.memory_limit(128 * 1024 * 1024)  // 128MB maximum heap
```

When exceeded:

```rust
Err(Error::MemoryLimitExceeded {
    limit: 134217728,
    requested: 268435456
})
```

### Stack Size

```rust
.stack_size(1024 * 1024)  // 1MB stack
```

Prevents stack overflow attacks and deep recursion.

### Memory Usage Tracking

```rust
let output = sandbox.run(&[]).await?;
println!("Peak memory: {} bytes", output.resource_usage.memory_peak);
```

## CPU Limits

### Fuel Metering

Fuel is a unit of computation. Each WASM instruction consumes fuel:

```rust
.fuel(1_000_000)  // 1 million fuel units
```

When exhausted:

```rust
Err(Error::FuelExhausted { limit: 1000000 })
```

**Fuel consumption rates** (approximate):
- Simple instructions (add, sub): 1 fuel
- Memory operations: 1-2 fuel
- Function calls: 2-5 fuel
- Loops: fuel per iteration

### CPU Time Limit

```rust
use std::time::Duration;

.cpu_time_limit(Duration::from_secs(30))  // 30 seconds of CPU time
```

This is **CPU time**, not wall clock time. A module sleeping doesn't consume CPU time.

## Time Limits

### Wall Clock Timeout

```rust
.wall_time_limit(Duration::from_secs(60))  // 60 seconds total
```

Includes all time: execution, I/O waits, sleeps. This is enforced via epoch-based interruption.

When exceeded:

```rust
Err(Error::Timeout(Duration::from_secs(60)))
```

## I/O Limits

### Read Limit

```rust
.io_read_limit(10 * 1024 * 1024)  // 10MB read limit
```

### Write Limit

```rust
.io_write_limit(1024 * 1024)  // 1MB write limit
```

These limits apply across all I/O operations: filesystem, network, stdin/stdout.

## Default Values

When not specified, these defaults apply:

| Resource | Default |
|----------|---------|
| Heap memory | 256MB |
| Stack size | 1MB |
| Fuel | Unlimited |
| CPU time | Unlimited |
| Wall time | Unlimited |
| I/O read | Unlimited |
| I/O write | Unlimited |

:::danger Warning
Running untrusted code without limits is dangerous! Always set appropriate limits.
:::

## Recommended Configurations

### Interactive Applications

```rust
SandboxConfig::builder()
    .memory_limit(64 * 1024 * 1024)   // 64MB
    .fuel(10_000_000)                  // 10M instructions
    .wall_time_limit(Duration::from_secs(5))
    .io_write_limit(1024 * 1024)       // 1MB output
```

### Batch Processing

```rust
SandboxConfig::builder()
    .memory_limit(512 * 1024 * 1024)  // 512MB
    .fuel(1_000_000_000)               // 1B instructions
    .wall_time_limit(Duration::from_secs(300))  // 5 minutes
    .io_read_limit(100 * 1024 * 1024)  // 100MB input
    .io_write_limit(100 * 1024 * 1024) // 100MB output
```

### Untrusted Code (Strict)

```rust
SandboxConfig::builder()
    .memory_limit(16 * 1024 * 1024)   // 16MB
    .stack_size(256 * 1024)            // 256KB stack
    .fuel(100_000)                     // 100K instructions
    .wall_time_limit(Duration::from_millis(100))
    .io_read_limit(1024)               // 1KB
    .io_write_limit(1024)              // 1KB
```

## Resource Usage Reporting

After execution, inspect resource usage:

```rust
let output = sandbox.run(&[]).await?;
let usage = &output.resource_usage;

println!("Fuel consumed: {:?}", usage.fuel_consumed);
println!("Memory peak: {} bytes", usage.memory_peak);
println!("I/O read: {} bytes", usage.io_read);
println!("I/O write: {} bytes", usage.io_write);
println!("Duration: {:?}", output.duration);
```

## Monitoring Resource Usage

Track resource usage with Prometheus metrics:

```rust
// Metrics are automatically exported:
// - isolate_sandbox_fuel_consumed
// - isolate_sandbox_memory_peak_bytes
// - isolate_sandbox_io_read_bytes
// - isolate_sandbox_io_write_bytes
// - isolate_sandbox_duration_seconds
```

## Error Handling

```rust
match sandbox.run(&[]).await {
    Ok(output) => { /* success */ }

    Err(Error::FuelExhausted { limit }) => {
        log::warn!("CPU limit hit: {} fuel", limit);
    }

    Err(Error::MemoryLimitExceeded { limit, requested }) => {
        log::warn!("Memory limit hit: {} of {} bytes", requested, limit);
    }

    Err(Error::Timeout(duration)) => {
        log::warn!("Timeout after {:?}", duration);
    }

    Err(e) => {
        log::error!("Execution failed: {}", e);
    }
}
```

## See Also

- [Capabilities](./capabilities) - Permission controls
- [Security Model](./security-model) - Defense in depth
- [Monitoring](./monitoring) - Tracking and alerting
