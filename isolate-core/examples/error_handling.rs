//! Error Handling Example
//!
//! This example demonstrates:
//! - Handling different error types from Isolate
//! - Using error categorization methods
//! - Accessing error suggestions for debugging
//! - Proper error propagation patterns

use isolate_core::{capability::Capability, error::Error, SandboxConfig};
use std::time::Duration;

// Invalid WASM module (wrong magic number)
const INVALID_WASM: &[u8] = &[0x00, 0x00, 0x00, 0x00];

// A minimal valid WASM module that exits with code 0
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // WASM magic
    0x01, 0x00, 0x00, 0x00, // Version 1
    // Type section
    0x01, 0x08, 0x02, 0x60, 0x01, 0x7f, 0x00, 0x60, 0x00, 0x00,
    // Import section: wasi_snapshot_preview1.proc_exit
    0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61,
    0x70, 0x73, 0x68, 0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65,
    0x77, 0x31, 0x09, 0x70, 0x72, 0x6f, 0x63, 0x5f, 0x65, 0x78, 0x69, 0x74,
    0x00, 0x00,
    // Function section
    0x03, 0x02, 0x01, 0x01,
    // Memory section
    0x05, 0x03, 0x01, 0x00, 0x01,
    // Export section
    0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
    0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01,
    // Code section
    0x0a, 0x08, 0x01, 0x06, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
];

#[tokio::main]
async fn main() {
    println!("Isolate Error Handling Example");
    println!("================================\n");

    // Example 1: Module validation errors
    println!("1. Module Validation Error:");
    demo_invalid_module();
    println!();

    // Example 2: Configuration errors
    println!("2. Configuration Errors:");
    demo_config_error();
    println!();

    // Example 3: Error categorization
    println!("3. Error Categorization:");
    demo_error_categories();
    println!();

    // Example 4: Error suggestions
    println!("4. Error Suggestions:");
    demo_error_suggestions();
    println!();

    // Example 5: Pattern matching on errors
    println!("5. Pattern Matching on Errors:");
    demo_error_matching().await;
    println!();

    // Example 6: Result propagation patterns
    println!("6. Result Propagation Patterns:");
    if let Err(e) = demo_propagation().await {
        println!("   Propagated error: {}", e);
        if let Some(suggestion) = e.suggestion() {
            println!("   Suggestion: {}", suggestion);
        }
    }
    println!();

    println!("Example complete!");
}

fn demo_invalid_module() {
    match SandboxConfig::builder().module(INVALID_WASM) {
        Ok(_) => println!("   Unexpected success!"),
        Err(e) => {
            println!("   Error: {}", e);
            if let Some(suggestion) = e.suggestion() {
                println!("   Suggestion: {}", suggestion);
            }
        }
    }
}

fn demo_config_error() {
    // Try to build config without a module
    let result = SandboxConfig::builder()
        .memory_limit(64 * 1024 * 1024)
        .build();

    match result {
        Ok(_) => println!("   Unexpected success!"),
        Err(e) => {
            println!("   Error: {}", e);
        }
    }
}

fn demo_error_categories() {
    // Create some example errors
    let timeout_error = Error::Timeout(Duration::from_secs(30));
    let fuel_error = Error::FuelExhausted { limit: 1_000_000 };
    let capability_error = Error::CapabilityDenied(Capability::stdout());
    let memory_error = Error::MemoryLimitExceeded {
        limit: 64 * 1024 * 1024,
        requested: 128 * 1024 * 1024,
    };

    println!("   Timeout error:");
    println!("     is_timeout: {}", timeout_error.is_timeout());
    println!("     is_resource_limit: {}", timeout_error.is_resource_limit());
    println!("     is_capability_error: {}", timeout_error.is_capability_error());

    println!("   Fuel exhausted error:");
    println!("     is_timeout: {}", fuel_error.is_timeout());
    println!("     is_resource_limit: {}", fuel_error.is_resource_limit());

    println!("   Capability denied error:");
    println!("     is_capability_error: {}", capability_error.is_capability_error());
    println!("     is_resource_limit: {}", capability_error.is_resource_limit());

    println!("   Memory limit error:");
    println!("     is_resource_limit: {}", memory_error.is_resource_limit());
}

fn demo_error_suggestions() {
    let errors: Vec<Error> = vec![
        Error::Compilation("Invalid module format".to_string()),
        Error::FuelExhausted { limit: 1_000_000 },
        Error::MemoryLimitExceeded {
            limit: 64 * 1024 * 1024,
            requested: 128 * 1024 * 1024,
        },
        Error::CapabilityDenied(Capability::stdout()),
        Error::Timeout(Duration::from_secs(30)),
        Error::FunctionNotFound("custom_entry".to_string()),
    ];

    for error in errors {
        println!("   Error: {}", error);
        if let Some(suggestion) = error.suggestion() {
            println!("     Suggestion: {}", suggestion);
        }
        println!();
    }
}

async fn demo_error_matching() {
    // Simulate getting an error and handling it based on type
    let simulated_error = Error::FuelExhausted { limit: 1_000_000 };

    match simulated_error {
        Error::Timeout(duration) => {
            println!("   Would retry with longer timeout (was {:?})", duration);
        }
        Error::FuelExhausted { limit } => {
            println!("   Would retry with more fuel (was {} units)", limit);
            println!("   Recommended new limit: {} units", limit * 2);
        }
        Error::MemoryLimitExceeded { limit, requested } => {
            println!(
                "   Would retry with more memory (needed {}, had {})",
                requested, limit
            );
        }
        Error::CapabilityDenied(cap) => {
            println!("   Missing capability: {:?}", cap);
            println!("   Would add capability and retry");
        }
        Error::Compilation(msg) => {
            println!("   Cannot recover from compilation error: {}", msg);
        }
        other => {
            println!("   Unexpected error type: {}", other);
        }
    }
}

async fn demo_propagation() -> Result<(), Error> {
    // Pattern 1: Using ? operator for clean propagation
    let _config = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .memory_limit(64 * 1024 * 1024)
        .build()?;

    // Pattern 2: Converting errors with context
    // (shown as demonstration - would require anyhow or similar)

    // Pattern 3: Mapping errors for custom handling
    let result: Result<(), Error> = Err(Error::FuelExhausted { limit: 100 });

    // Transform or wrap the error before propagating
    result.map_err(|e| {
        // Log the error before propagating
        eprintln!("   Logging error before propagation: {}", e);
        e
    })?;

    Ok(())
}
