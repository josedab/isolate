//! Resource Limits Example
//!
//! This example demonstrates Isolate's resource limiting capabilities:
//! - Memory limits (heap and stack)
//! - CPU limits (fuel-based instruction counting)
//! - Time limits (wall time and CPU time)
//! - I/O limits (bytes read/written, operation count)

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::Duration;

// A minimal WASM module for demonstration
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x01, 0x00, 0x00, 0x00, // version: 1
];

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    println!("Isolate Resource Limits Example");
    println!("=================================\n");

    // Example 1: Memory Limits
    println!("1. Memory Limits:");
    let config_memory = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        // Heap memory limit
        .memory_limit(64 * 1024 * 1024) // 64MB max heap
        // Stack size
        .stack_size(512 * 1024) // 512KB stack
        .build()?;

    println!(
        "   Heap limit: {} bytes (64MB)",
        config_memory.resources.memory.heap_max
    );
    println!(
        "   Stack size: {} bytes (512KB)",
        config_memory.resources.memory.stack_max
    );
    println!("   Memory violations will cause sandbox termination.");
    println!();

    // Example 2: CPU/Instruction Limits (Fuel)
    println!("2. CPU Limits (Fuel-based):");
    let config_cpu = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        // Fuel is consumed for each WASM instruction
        .fuel(1_000_000) // 1 million instructions max
        .build()?;

    println!(
        "   Fuel limit: {:?} instructions",
        config_cpu.resources.cpu.fuel
    );
    println!("   When fuel exhausts, execution stops immediately.");
    println!("   Useful for preventing infinite loops and CPU abuse.");
    println!();

    // Example 3: Time Limits
    println!("3. Time Limits:");
    let config_time = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        // Wall clock time (real time)
        .wall_time_limit(Duration::from_secs(30))
        // CPU time (execution time only)
        .cpu_time_limit(Duration::from_secs(10))
        .build()?;

    println!(
        "   Wall time limit: {:?}",
        config_time.resources.time.wall_time
    );
    println!(
        "   CPU time limit: {:?}",
        config_time.resources.time.cpu_time
    );
    println!("   Wall time includes I/O waits; CPU time is pure execution.");
    println!("   Epoch-based interruption allows sub-10ms timeout precision.");
    println!();

    // Example 4: I/O Limits
    println!("4. I/O Limits:");
    let config_io = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        // Limit total bytes read
        .io_read_limit(10 * 1024 * 1024) // 10MB max read
        // Limit total bytes written
        .io_write_limit(5 * 1024 * 1024) // 5MB max write
        .build()?;

    println!(
        "   Read limit: {:?} (10MB)",
        config_io.resources.io.read_bytes
    );
    println!(
        "   Write limit: {:?} (5MB)",
        config_io.resources.io.write_bytes
    );
    println!("   IOPS limit: {:?}", config_io.resources.io.iops);
    println!("   Prevents I/O-based resource exhaustion attacks.");
    println!();

    // Example 5: Combined Production Configuration
    println!("5. Production Configuration (All Limits):");
    let config_production = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        // Memory: 128MB heap, 1MB stack
        .memory_limit(128 * 1024 * 1024)
        .stack_size(1024 * 1024)
        // CPU: 50M instructions
        .fuel(50_000_000)
        // Time: 60s wall, 30s CPU
        .wall_time_limit(Duration::from_secs(60))
        .cpu_time_limit(Duration::from_secs(30))
        // I/O: 50MB read, 10MB write
        .io_read_limit(50 * 1024 * 1024)
        .io_write_limit(10 * 1024 * 1024)
        .build()?;

    println!(
        "   Memory: {} heap, {} stack",
        format_bytes(config_production.resources.memory.heap_max as u64),
        format_bytes(config_production.resources.memory.stack_max as u64)
    );
    println!("   CPU: {:?} fuel", config_production.resources.cpu.fuel);
    println!(
        "   Time: {:?} wall, {:?} CPU",
        config_production.resources.time.wall_time, config_production.resources.time.cpu_time
    );
    if let (Some(read), Some(write)) = (
        config_production.resources.io.read_bytes,
        config_production.resources.io.write_bytes,
    ) {
        println!(
            "   I/O: {} read, {} write",
            format_bytes(read),
            format_bytes(write)
        );
    }
    println!();

    // Example 6: Checking Resource Usage
    println!("6. Accessing Resource Usage After Execution:");
    let sandbox = Sandbox::create(config_production).await?;

    println!("   After creating sandbox:");
    println!("   - Sandbox ID: {}", sandbox.id());
    println!("   - State: {}", sandbox.state());
    println!();
    println!("   After running (sandbox.run(&[]).await?):");
    println!("   - output.resource_usage.memory_peak: peak memory used");
    println!("   - output.resource_usage.fuel_consumed: instructions executed");
    println!("   - output.resource_usage.cpu_time: CPU time used");
    println!("   - output.resource_usage.bytes_read: total bytes read");
    println!("   - output.resource_usage.bytes_written: total bytes written");
    println!();

    println!("Example complete!");
    println!("\nNote: Resource limit violations result in specific error types:");
    println!("  - Error::FuelExhausted - CPU instruction limit reached");
    println!("  - Error::MemoryLimitExceeded - Memory allocation failed");
    println!("  - Error::Timeout - Time limit exceeded");
    println!("  - Error::IoLimitExceeded - I/O limit exceeded");

    // Prevent unused warnings
    let _ = config_memory;
    let _ = config_cpu;
    let _ = config_time;
    let _ = config_io;

    Ok(())
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{}GB", bytes / (1024 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 {
        format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}
