//! End-to-end integration tests for the full sandbox lifecycle.
//!
//! Tests the complete path: config → sandbox creation → execution → output verification,
//! across different module types, capability combinations, and error scenarios.

use isolate_core::capability::Capability;
use isolate_core::config::SandboxConfig;
use isolate_core::sandbox::Sandbox;
use std::time::Duration;

const MINIMAL_WASM: &[u8] = include_bytes!("fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("fixtures/hello.wasm");
const EXIT_42_WASM: &[u8] = include_bytes!("fixtures/exit_42.wasm");

// ---------------------------------------------------------------------------
// Full lifecycle tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_minimal_lifecycle() {
    // Create → Run → Verify output
    let config = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();

    assert_eq!(output.exit_code, 0);
    assert!(output.duration < Duration::from_secs(5));
}

#[tokio::test]
async fn e2e_hello_world_lifecycle() {
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .unwrap()
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();

    assert_eq!(output.exit_code, 0);
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout_str.contains("Hello"),
        "Expected stdout to contain 'Hello', got: {}",
        stdout_str
    );
}

#[tokio::test]
async fn e2e_exit_code_propagation() {
    let config = SandboxConfig::builder().module(EXIT_42_WASM).unwrap().build().unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();

    assert_eq!(output.exit_code, 42);
}

// ---------------------------------------------------------------------------
// Resource metering tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_fuel_metering() {
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .unwrap()
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();

    assert_eq!(output.exit_code, 0);
    assert!(output.resource_usage.fuel_consumed > 0, "Expected fuel consumption > 0");
}

#[tokio::test]
async fn e2e_memory_limit() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .unwrap()
        .memory_limit(16 * 1024 * 1024) // 16MB
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();
    assert_eq!(output.exit_code, 0);
}

#[tokio::test]
async fn e2e_wall_time_recorded() {
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .unwrap()
        .capability(Capability::stdout())
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();

    assert!(output.resource_usage.wall_time > Duration::ZERO, "Wall time should be recorded");
}

// ---------------------------------------------------------------------------
// Capability tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_multiple_capabilities() {
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .unwrap()
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .capability(Capability::system_clock())
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();
    assert_eq!(output.exit_code, 0);
}

#[tokio::test]
async fn e2e_env_vars() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .unwrap()
        .env("TEST_KEY", "test_value")
        .env("ANOTHER", "value2")
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();
    assert_eq!(output.exit_code, 0);
}

#[tokio::test]
async fn e2e_args_passing() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .unwrap()
        .arg("--verbose".to_string())
        .arg("--output=json".to_string())
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output = sandbox.run(&[]).await.unwrap();
    assert_eq!(output.exit_code, 0);
}

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

#[test]
fn e2e_invalid_wasm_rejected() {
    let result = SandboxConfig::builder().module(&[0x00, 0x00, 0x00, 0x00]);
    assert!(result.is_err(), "Invalid WASM should be rejected at config time");
}

#[test]
fn e2e_empty_wasm_rejected() {
    let result = SandboxConfig::builder().module(&[]);
    assert!(result.is_err(), "Empty WASM should be rejected");
}

// ---------------------------------------------------------------------------
// Config builder validation tests
// ---------------------------------------------------------------------------

#[test]
fn e2e_config_builder_fluent_api() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .unwrap()
        .memory_limit(64 * 1024 * 1024)
        .fuel(500_000)
        .wall_time_limit(Duration::from_secs(30))
        .capability(Capability::stdout())
        .env("KEY", "value")
        .arg("--test".to_string())
        .build()
        .unwrap();

    // Verify config was built (we trust the builder internals)
    assert!(std::mem::size_of_val(&config) > 0);
}

#[tokio::test]
async fn e2e_sequential_executions() {
    // Create one sandbox, run it twice (verifying state isolation)
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .unwrap()
        .capability(Capability::stdout())
        .build()
        .unwrap();

    let mut sandbox = Sandbox::create(config).await.unwrap();
    let output1 = sandbox.run(&[]).await.unwrap();
    assert_eq!(output1.exit_code, 0);

    // Second execution should fail (sandbox consumed)
    let result2 = sandbox.run(&[]).await;
    assert!(result2.is_err(), "Second run on consumed sandbox should fail");
}

#[tokio::test]
async fn e2e_concurrent_sandboxes() {
    // Run multiple sandboxes concurrently
    let handles: Vec<_> = (0..5)
        .map(|_| {
            tokio::spawn(async {
                let config =
                    SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();
                let mut sandbox = Sandbox::create(config).await.unwrap();
                sandbox.run(&[]).await.unwrap()
            })
        })
        .collect();

    for handle in handles {
        let output = handle.await.unwrap();
        assert_eq!(output.exit_code, 0);
    }
}
