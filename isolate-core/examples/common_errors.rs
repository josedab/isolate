//! Common errors and how to handle them.
//!
//! This example demonstrates the most frequent errors developers encounter
//! and shows idiomatic ways to handle each one.
//!
//! Run with:
//!   cargo run -p isolate-core --example common_errors

use isolate_core::{capability::Capability, error::Error, Sandbox, SandboxConfig};
use std::time::Duration;

// Minimal WASM that exits with code 0 (no I/O)
// WASM that writes to stdout
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

#[tokio::main]
async fn main() {
    println!("=== Isolate Common Errors Demo ===\n");

    demo_capability_denied().await;
    demo_fuel_exhausted().await;
    demo_invalid_wasm().await;
    demo_error_suggestions().await;
    demo_error_categories().await;

    println!("\n=== All demos complete ===");
}

/// Demonstrates what happens when a module tries to use stdout without permission.
async fn demo_capability_denied() {
    println!("--- Demo: Capability Denied ---");

    // hello.wasm writes to stdout, but we DON'T grant stdout capability
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .expect("valid module")
        .fuel(1_000_000)
        // Note: intentionally NOT granting Capability::stdout()
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox created");
    match sandbox.run(&[]).await {
        Ok(output) => {
            // The module may still succeed but stdout is silently dropped
            println!("  Exit code: {} (stdout was silently dropped)", output.exit_code);
        }
        Err(e) => {
            println!("  Error: {}", e);
            if let Some(suggestion) = e.suggestion() {
                println!("  Suggestion: {}", suggestion);
            }
        }
    }

    // Fix: grant the capability
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .expect("valid module")
        .fuel(1_000_000)
        .capability(Capability::stdout()) // <-- The fix
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox created");
    let output = sandbox.run(&[]).await.expect("should succeed now");
    println!("  Fixed! Exit: {}, Output: {}", output.exit_code, output.stdout_str().trim());
    println!();
}

/// Demonstrates running out of CPU fuel.
async fn demo_fuel_exhausted() {
    println!("--- Demo: Fuel Exhausted ---");

    // Give very little fuel — the module may exhaust it
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .expect("valid module")
        .fuel(100) // Very low fuel limit
        .capability(Capability::stdout())
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox created");
    match sandbox.run(&[]).await {
        Ok(output) => println!("  Completed with exit code: {}", output.exit_code),
        Err(ref e) if e.is_resource_limit() => {
            println!("  Resource limit hit: {}", e);
            if let Some(suggestion) = e.suggestion() {
                println!("  Suggestion: {}", suggestion);
            }
        }
        Err(e) => println!("  Other error: {}", e),
    }
    println!();
}

/// Demonstrates invalid WASM bytes.
async fn demo_invalid_wasm() {
    println!("--- Demo: Invalid WASM ---");

    let bad_bytes = b"this is not valid wasm";
    match SandboxConfig::builder().module(bad_bytes) {
        Ok(_) => println!("  Unexpectedly accepted invalid WASM"),
        Err(e) => {
            println!("  Error: {}", e);
            if let Some(suggestion) = e.suggestion() {
                println!("  Suggestion: {}", suggestion);
            }
        }
    }
    println!();
}

/// Demonstrates the suggestion() method on all error types.
async fn demo_error_suggestions() {
    println!("--- Demo: Error Suggestions ---");

    let errors: Vec<Error> = vec![
        Error::Timeout(Duration::from_secs(30)),
        Error::FuelExhausted { limit: 1_000_000, consumed: 1_000_001 },
        Error::MemoryLimitExceeded {
            limit: 64 * 1024 * 1024,
            requested: 128 * 1024 * 1024,
            current_usage: 60 * 1024 * 1024,
        },
        Error::CapabilityDenied(Capability::stdout()),
        Error::CapabilityDenied(Capability::filesystem_read("/data")),
        Error::CapabilityDenied(Capability::http_client(vec!["api.example.com"])),
    ];

    for error in &errors {
        println!("  Error: {}", error);
        if let Some(suggestion) = error.suggestion() {
            println!("  → {}", suggestion);
        }
        println!();
    }
}

/// Demonstrates error categorization methods.
async fn demo_error_categories() {
    println!("--- Demo: Error Categories ---");

    let timeout = Error::Timeout(Duration::from_secs(5));
    println!(
        "  Timeout — is_timeout: {}, is_resource_limit: {}, is_capability_error: {}",
        timeout.is_timeout(),
        timeout.is_resource_limit(),
        timeout.is_capability_error()
    );

    let fuel = Error::FuelExhausted { limit: 100, consumed: 101 };
    println!(
        "  FuelExhausted — is_timeout: {}, is_resource_limit: {}, is_capability_error: {}",
        fuel.is_timeout(),
        fuel.is_resource_limit(),
        fuel.is_capability_error()
    );

    let cap = Error::CapabilityDenied(Capability::stdout());
    println!(
        "  CapabilityDenied — is_timeout: {}, is_resource_limit: {}, is_capability_error: {}",
        cap.is_timeout(),
        cap.is_resource_limit(),
        cap.is_capability_error()
    );
    println!();
}
