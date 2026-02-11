//! Basic sandbox execution example.
//!
//! Demonstrates creating a sandbox from a WASM module, running it,
//! and reading its output.
//!
//! # Usage
//! ```sh
//! cargo run --example hello_sandbox --package isolate-core
//! ```

use isolate_core::{capability::Capability, config::SandboxConfig, Sandbox};

const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build a sandbox configuration with stdout capability and fuel limit
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)?
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .build()?;

    // Create and run the sandbox
    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit code: {}", output.exit_code);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("Duration: {:?}", output.duration);

    Ok(())
}
