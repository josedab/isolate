//! Basic example of using Isolate to run WASM code.
//!
//! This example demonstrates:
//! - Creating a sandbox with capabilities and resource limits
//! - Running a WASM module
//! - Accessing the output and metrics

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::Duration;

// A minimal WASM module for demonstration
// In practice, you'd load a compiled WASM file
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x01, 0x00, 0x00, 0x00, // version: 1
];

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("Isolate Basic Example");
    println!("======================\n");

    // Create a sandbox configuration
    let config = SandboxConfig::builder()
        // Load the WASM module
        .module(MINIMAL_WASM)?
        // Set resource limits
        .memory_limit(128 * 1024 * 1024) // 128MB
        .fuel(10_000_000) // 10M instructions
        .wall_time_limit(Duration::from_secs(30))
        // Grant capabilities
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        // Environment and arguments
        .env("APP_NAME", "isolate-example")
        .arg("--verbose".to_string())
        .build()?;

    println!("Configuration:");
    println!("  Memory limit: {} bytes", config.resources.memory.heap_max);
    println!("  Fuel limit: {:?}", config.resources.cpu.fuel);
    println!("  Wall time limit: {:?}", config.resources.time.wall_time);
    println!();

    // Create the sandbox
    println!("Creating sandbox...");
    let sandbox = Sandbox::create(config).await?;

    println!("Sandbox created:");
    println!("  ID: {}", sandbox.id());
    println!("  State: {}", sandbox.state());
    println!("  Module hash: {}", sandbox.module_hash());
    println!();

    // In a real scenario, you'd run the sandbox:
    // let output = sandbox.run(&[]).await?;
    // println!("Output: {}", output.stdout_str());

    println!("Example complete!");

    Ok(())
}
