//! Resource limits example: demonstrate timeout and fuel exhaustion.
//!
//! This example shows how resource limits protect against runaway WASM modules.
//!
//! Run with:
//!   cargo run -p isolate-core --example resource_limits

use isolate_core::{Sandbox, SandboxConfig};
use std::time::Duration;

const INFINITE_LOOP_WASM: &[u8] = include_bytes!("../tests/fixtures/infinite_loop.wasm");
const CPU_INTENSIVE_WASM: &[u8] = include_bytes!("../tests/fixtures/cpu_intensive.wasm");

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Example 1: Wall-time timeout stops infinite loops
    println!("=== Timeout Example ===");
    let config = SandboxConfig::builder()
        .module(INFINITE_LOOP_WASM)?
        .wall_time_limit(Duration::from_millis(100))
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    match sandbox.run(&[]).await {
        Ok(output) => println!("Exited with code {}", output.exit_code),
        Err(e) => {
            println!("Caught expected error: {e}");
            if let Some(suggestion) = e.suggestion() {
                println!("Suggestion: {suggestion}");
            }
        }
    }

    // Example 2: Fuel limits stop CPU-intensive modules
    println!("\n=== Fuel Limit Example ===");
    let config = SandboxConfig::builder()
        .module(CPU_INTENSIVE_WASM)?
        .fuel(1_000) // Very low fuel
        .wall_time_limit(Duration::from_secs(5))
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    match sandbox.run(&[]).await {
        Ok(output) => println!("Exited with code {}", output.exit_code),
        Err(e) => {
            println!("Caught expected error: {e}");
            if let Some(suggestion) = e.suggestion() {
                println!("Suggestion: {suggestion}");
            }
        }
    }

    println!("\nResource limits working correctly!");
    Ok(())
}
