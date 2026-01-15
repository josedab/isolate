//! Capability Guide: understanding the Isolate permission system.
//!
//! This example demonstrates every capability type and how to configure them.
//! Each section shows what happens without the capability and with it.
//!
//! Run with:
//!   cargo run -p isolate-core --example capability_guide

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::Duration;

const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

#[tokio::main]
async fn main() {
    println!("=== Isolate Capability Guide ===\n");
    println!("Sandboxes have ZERO capabilities by default.");
    println!("Every permission must be explicitly granted.\n");

    demo_stdio_capabilities().await;
    demo_resource_limits().await;
    demo_capability_combinations().await;
    demo_preset_profiles().await;

    println!("=== Guide complete ===");
}

async fn demo_stdio_capabilities() {
    println!("--- Standard I/O Capabilities ---\n");

    // 1. No capabilities — minimal sandbox
    println!("  1. No capabilities (compute-only):");
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid")
        .fuel(1_000_000)
        .build()
        .expect("valid config");
    let mut sb = Sandbox::create(config).await.expect("created");
    let out = sb.run(&[]).await.expect("runs");
    println!("     Exit: {} (no I/O allowed)", out.exit_code);

    // 2. Stdout only
    println!("  2. Stdout capability:");
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .expect("valid")
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .build()
        .expect("valid config");
    let mut sb = Sandbox::create(config).await.expect("created");
    let out = sb.run(&[]).await.expect("runs");
    println!("     Exit: {}, Output: {}", out.exit_code, out.stdout_str().trim());

    // 3. All stdio
    println!("  3. All stdio (stdout + stderr + stdin):");
    println!("     .capability(Capability::stdout())");
    println!("     .capability(Capability::stderr())");
    println!("     .capability(Capability::stdin())");
    println!();
}

async fn demo_resource_limits() {
    println!("--- Resource Limits ---\n");

    println!("  Resource limits protect against runaway modules:\n");
    println!("  | Limit              | Method                          | Default   |");
    println!("  |--------------------|---------------------------------|-----------|");
    println!("  | Heap memory        | .memory_limit(bytes)            | 256 MB    |");
    println!("  | Stack size         | .stack_size(bytes)              | 1 MB      |");
    println!("  | CPU fuel           | .fuel(units)                    | Unlimited |");
    println!("  | CPU time           | .cpu_time_limit(duration)       | 30s       |");
    println!("  | Wall-clock time    | .wall_time_limit(duration)      | 60s       |");
    println!("  | I/O read           | .io_read_limit(bytes)           | Unlimited |");
    println!("  | I/O write          | .io_write_limit(bytes)          | Unlimited |");
    println!();

    // Show a restrictive config
    println!("  Restrictive configuration example:");
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid")
        .memory_limit(16 * 1024 * 1024) // 16MB
        .fuel(500_000) // 500K instructions
        .wall_time_limit(Duration::from_secs(5))
        .build()
        .expect("valid config");
    let mut sb = Sandbox::create(config).await.expect("created");
    let out = sb.run(&[]).await.expect("runs");
    println!("     Completed with exit code: {}", out.exit_code);
    println!();
}

async fn demo_capability_combinations() {
    println!("--- Capability Combinations ---\n");

    println!("  Common capability profiles:\n");

    println!("  Compute-only (safest):");
    println!("    No capabilities needed\n");

    println!("  CLI tool:");
    println!("    .capability(Capability::stdout())");
    println!("    .capability(Capability::stderr())\n");

    println!("  File processor:");
    println!("    .capability(Capability::stdout())");
    println!("    .capability(Capability::stderr())");
    println!("    .capability(Capability::filesystem_read(\"/input\"))");
    println!("    .capability(Capability::filesystem_write(\"/output\"))\n");

    println!("  Web service client:");
    println!("    .capability(Capability::stdout())");
    println!("    .capability(Capability::http_client(vec![\"api.example.com\"]))");
    println!("    .capability(Capability::dns_resolve())\n");

    println!("  AI agent:");
    println!("    .capability(Capability::stdout())");
    println!("    .capability(Capability::stderr())");
    println!("    .capability(Capability::http_client(vec![\"*\"]))");
    println!("    .capability(Capability::filesystem_read(\"/data\"))");
    println!("    .capability(Capability::system_clock())");
    println!("    .capability(Capability::secure_random())\n");
}

async fn demo_preset_profiles() {
    println!("--- Using Preset Resource Profiles ---\n");

    println!("  Isolate provides preset resource limit profiles:\n");
    println!("  ResourceLimits::restrictive():");
    println!("    64MB heap, 1M fuel, 5s CPU, 10MB read, 1MB write\n");
    println!("  ResourceLimits::permissive():");
    println!("    4GB heap, unlimited fuel, 8MB stack\n");
    println!("  Or build custom limits with the builder API.\n");
}
