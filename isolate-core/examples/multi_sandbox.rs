//! Multi-Sandbox Concurrent Execution Example
//!
//! This example demonstrates:
//! - Running multiple sandboxes concurrently using tokio
//! - Sharing a WASM module across sandboxes
//! - Collecting and comparing results from multiple executions

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

// A minimal WASI WASM module that exits immediately
// This is a valid WASM module that calls proc_exit(0)
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // WASM magic
    0x01, 0x00, 0x00, 0x00, // Version 1
    // Type section
    0x01, 0x08, 0x02, 0x60, 0x01, 0x7f, 0x00, 0x60, 0x00, 0x00,
    // Import section: wasi_snapshot_preview1.proc_exit
    0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61, 0x70, 0x73, 0x68, 0x6f,
    0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65, 0x77, 0x31, 0x09, 0x70, 0x72, 0x6f, 0x63, 0x5f,
    0x65, 0x78, 0x69, 0x74, 0x00, 0x00, // Function section
    0x03, 0x02, 0x01, 0x01, // Memory section
    0x05, 0x03, 0x01, 0x00, 0x01, // Export section: memory and _start
    0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x5f, 0x73, 0x74,
    0x61, 0x72, 0x74, 0x00, 0x01, // Code section: call proc_exit(0)
    0x0a, 0x08, 0x01, 0x06, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
];

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    println!("Isolate Multi-Sandbox Concurrent Execution Example");
    println!("====================================================\n");

    let num_sandboxes = 10;

    // Example 1: Sequential execution (baseline)
    println!("1. Sequential Execution ({} sandboxes):", num_sandboxes);
    let start = Instant::now();

    for i in 0..num_sandboxes {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)?
            .memory_limit(64 * 1024 * 1024)
            .fuel(1_000_000)
            .wall_time_limit(Duration::from_secs(10))
            .capability(Capability::stdout())
            .build()?;

        let mut sandbox = Sandbox::create(config).await?;
        let output = sandbox.run(&[]).await?;

        if i == 0 {
            println!(
                "   First sandbox: exit_code={}, duration={:?}",
                output.exit_code, output.duration
            );
        }
    }

    let sequential_duration = start.elapsed();
    println!("   Total sequential time: {:?}", sequential_duration);
    println!();

    // Example 2: Concurrent execution using JoinSet
    println!("2. Concurrent Execution ({} sandboxes):", num_sandboxes);
    let start = Instant::now();

    let mut tasks: JoinSet<isolate_core::Result<(String, u32, Duration)>> = JoinSet::new();

    for i in 0..num_sandboxes {
        // Clone the WASM bytes for each task
        let wasm = MINIMAL_WASM.to_vec();

        tasks.spawn(async move {
            let config = SandboxConfig::builder()
                .module(&wasm)?
                .memory_limit(64 * 1024 * 1024)
                .fuel(1_000_000)
                .wall_time_limit(Duration::from_secs(10))
                .capability(Capability::stdout())
                .build()?;

            let mut sandbox = Sandbox::create(config).await?;
            let sandbox_id = sandbox.id().to_string();
            let output = sandbox.run(&[]).await?;

            Ok((sandbox_id, output.exit_code as u32, output.duration))
        });

        // Small delay between spawns to avoid overwhelming the system
        if i < num_sandboxes - 1 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    // Collect results
    let mut successful = 0;
    let mut total_execution_time = Duration::ZERO;

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok((id, exit_code, duration))) => {
                successful += 1;
                total_execution_time += duration;
                if successful == 1 {
                    println!(
                        "   First completed: {} exit_code={}, duration={:?}",
                        &id[..8],
                        exit_code,
                        duration
                    );
                }
            }
            Ok(Err(e)) => {
                println!("   Sandbox error: {}", e);
            }
            Err(e) => {
                println!("   Task join error: {}", e);
            }
        }
    }

    let concurrent_duration = start.elapsed();
    println!("   Successful: {}/{}", successful, num_sandboxes);
    println!("   Total concurrent wall time: {:?}", concurrent_duration);
    println!("   Sum of execution times: {:?}", total_execution_time);
    println!();

    // Example 3: Performance comparison
    println!("3. Performance Comparison:");
    let speedup = sequential_duration.as_secs_f64() / concurrent_duration.as_secs_f64();
    println!("   Sequential: {:?}", sequential_duration);
    println!("   Concurrent: {:?}", concurrent_duration);
    println!("   Speedup: {:.2}x", speedup);
    println!();

    // Example 4: Different configurations per sandbox
    println!("4. Mixed Configurations (varying resource limits):");
    let configs = vec![
        ("small", 32 * 1024 * 1024, 500_000),    // 32MB, 500K fuel
        ("medium", 64 * 1024 * 1024, 1_000_000), // 64MB, 1M fuel
        ("large", 128 * 1024 * 1024, 2_000_000), // 128MB, 2M fuel
    ];

    let mut tasks: JoinSet<isolate_core::Result<(&str, Duration)>> = JoinSet::new();

    for (name, memory, fuel) in configs.clone() {
        let wasm = MINIMAL_WASM.to_vec();

        tasks.spawn(async move {
            let config = SandboxConfig::builder()
                .module(&wasm)?
                .memory_limit(memory)
                .fuel(fuel)
                .wall_time_limit(Duration::from_secs(10))
                .capability(Capability::stdout())
                .build()?;

            let creation_start = Instant::now();
            let mut sandbox = Sandbox::create(config).await?;
            let _ = sandbox.run(&[]).await?;

            Ok((name, creation_start.elapsed()))
        });
    }

    while let Some(result) = tasks.join_next().await {
        if let Ok(Ok((name, duration))) = result {
            println!("   {}: {:?}", name, duration);
        }
    }
    println!();

    println!("Example complete!");
    println!("\nNote: Concurrent execution leverages Rust's async runtime");
    println!("to efficiently multiplex multiple sandboxes across threads.");

    Ok(())
}
