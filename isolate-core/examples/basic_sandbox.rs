//! Basic example: run a WASM module in a sandbox.
//!
//! This example loads the "hello.wasm" test fixture and runs it in a sandbox
//! with stdout capability enabled.
//!
//! Run with:
//!   cargo run -p isolate-core --example basic_sandbox

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::Duration;

// hello.wasm writes "Hello from WASM!\n" to stdout and exits with 0
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)?
        .memory_limit(64 * 1024 * 1024) // 64 MB
        .fuel(1_000_000)
        .wall_time_limit(Duration::from_secs(5))
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit code: {}", output.exit_code);
    println!("Stdout: {}", output.stdout_str());

    if !output.success() {
        eprintln!("Stderr: {}", output.stderr_str());
    }

    Ok(())
}
