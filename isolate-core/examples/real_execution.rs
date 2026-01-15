//! Real WASM Execution Example
//!
//! This example demonstrates running actual WASM modules from the test fixtures.
//! It shows:
//! - Loading and executing real WASM modules
//! - Capturing stdout/stderr output
//! - Handling different exit codes
//! - Measuring resource consumption

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::Duration;

// Include WASM test fixtures at compile time
// These are valid WASI modules from the test fixtures directory

/// Minimal WASM that just exits with code 0
const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");

/// WASM that prints "Hello from WASM!" to stdout
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

/// WASM that exits with code 42
const EXIT_42_WASM: &[u8] = include_bytes!("../tests/fixtures/exit_42.wasm");

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Initialize logging for better visibility
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    println!("Isolate Real WASM Execution Example");
    println!("=====================================\n");

    // Example 1: Minimal module execution
    println!("1. Minimal Module (exit 0):");
    run_module("minimal", MINIMAL_WASM, false).await?;
    println!();

    // Example 2: Hello world with stdout capture
    println!("2. Hello World Module (stdout capture):");
    run_module("hello", HELLO_WASM, true).await?;
    println!();

    // Example 3: Non-zero exit code
    println!("3. Exit Code 42 Module:");
    run_module("exit_42", EXIT_42_WASM, false).await?;
    println!();

    // Example 4: Multiple executions with resource tracking
    println!("4. Resource Tracking Across Executions:");
    track_resources().await?;
    println!();

    println!("Example complete!");
    println!("\nNote: WASM modules from isolate-core/tests/fixtures/ were used.");
    println!("See generate_fixtures.py for how to create more test modules.");

    Ok(())
}

async fn run_module(name: &str, wasm: &[u8], needs_stdout: bool) -> isolate_core::Result<()> {
    let mut builder = SandboxConfig::builder()
        .module(wasm)?
        .memory_limit(64 * 1024 * 1024) // 64MB
        .fuel(10_000_000) // 10M instructions
        .wall_time_limit(Duration::from_secs(30));

    if needs_stdout {
        builder = builder.capability(Capability::stdout());
    }

    let config = builder.build()?;

    println!("   Module: {} ({} bytes)", name, wasm.len());

    let creation_start = std::time::Instant::now();
    let mut sandbox = Sandbox::create(config).await?;
    let creation_time = creation_start.elapsed();

    println!("   Sandbox created in {:?}", creation_time);
    println!("   ID: {}", sandbox.id());

    let output = sandbox.run(&[]).await?;

    println!("   Exit code: {}", output.exit_code);
    println!("   Duration: {:?}", output.duration);

    if !output.stdout.is_empty() {
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        println!("   Stdout: {:?}", stdout_str.trim());
    }

    if !output.stderr.is_empty() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        println!("   Stderr: {:?}", stderr_str.trim());
    }

    println!("   Peak memory: {} bytes", output.resource_usage.peak_memory);
    println!("   Fuel consumed: {}", output.resource_usage.fuel_consumed);

    Ok(())
}

async fn track_resources() -> isolate_core::Result<()> {
    let modules = [("minimal", MINIMAL_WASM), ("hello", HELLO_WASM), ("exit_42", EXIT_42_WASM)];

    let mut total_fuel = 0u64;
    let mut total_time = Duration::ZERO;
    let mut max_memory = 0usize;

    for (name, wasm) in modules {
        let config = SandboxConfig::builder()
            .module(wasm)?
            .memory_limit(64 * 1024 * 1024)
            .fuel(10_000_000)
            .wall_time_limit(Duration::from_secs(30))
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .build()?;

        let mut sandbox = Sandbox::create(config).await?;
        let output = sandbox.run(&[]).await?;

        total_fuel += output.resource_usage.fuel_consumed;
        total_time += output.duration;
        max_memory = max_memory.max(output.resource_usage.peak_memory);

        println!(
            "   {}: exit={}, fuel={}, time={:?}",
            name, output.exit_code, output.resource_usage.fuel_consumed, output.duration
        );
    }

    println!();
    println!("   Summary:");
    println!("   - Total fuel consumed: {} units", total_fuel);
    println!("   - Total execution time: {:?}", total_time);
    println!("   - Max peak memory: {} bytes", max_memory);

    Ok(())
}
