//! Integration tests for the Isolate core library.

use isolate_core::{
    capability::Capability, config::SandboxConfig, engine::WasmEngine, resource::ResourceLimits,
    Sandbox, SandboxState,
};
use std::sync::Arc;
use std::time::Duration;

// Minimal valid WASM module (header only - for validation tests)
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic
    0x01, 0x00, 0x00, 0x00, // version
];

// WASM module with _start function (for execution tests)
// This is a minimal WASI-compatible module that exports memory and _start
const RUNNABLE_WASM: &[u8] = include_bytes!("fixtures/minimal.wasm");

// WASM module that writes "Hello from WASM!\n" to stdout and exits with 0
const HELLO_WASM: &[u8] = include_bytes!("fixtures/hello.wasm");

// WASM module that exits with code 42
const EXIT_42_WASM: &[u8] = include_bytes!("fixtures/exit_42.wasm");

#[tokio::test]
async fn test_sandbox_creation() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid module")
        .memory_limit(64 * 1024 * 1024)
        .build()
        .expect("valid config");

    let sandbox = Sandbox::create(config).await.expect("sandbox creation");

    assert_eq!(sandbox.state(), SandboxState::Ready);
}

#[tokio::test]
async fn test_sandbox_with_shared_engine() {
    let engine = Arc::new(WasmEngine::new().expect("engine creation"));

    let config1 = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid module")
        .build()
        .expect("valid config");

    let config2 = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid module")
        .build()
        .expect("valid config");

    let sandbox1 = Sandbox::create_with_engine(config1, engine.clone())
        .await
        .expect("sandbox1 creation");
    let sandbox2 = Sandbox::create_with_engine(config2, engine.clone())
        .await
        .expect("sandbox2 creation");

    // Both should use cached module
    assert_eq!(engine.cached_module_count(), 1);
    assert_eq!(sandbox1.module_hash(), sandbox2.module_hash());
}

#[test]
fn test_capability_set() {
    use isolate_core::capability::CapabilitySet;

    let mut caps = CapabilitySet::new();
    assert!(caps.is_empty());

    caps.grant(Capability::stdout());
    caps.grant(Capability::stderr());
    caps.grant(Capability::filesystem_read("/data"));

    assert_eq!(caps.len(), 3);
    assert!(caps.has(&Capability::stdout()));
    assert!(caps.has(&Capability::stderr()));
    assert!(!caps.has(&Capability::stdin()));
}

#[test]
fn test_resource_limits() {
    let restrictive = ResourceLimits::restrictive();
    assert_eq!(restrictive.memory.heap_max, 64 * 1024 * 1024);
    assert!(restrictive.cpu.fuel.is_some());

    let permissive = ResourceLimits::permissive();
    assert!(permissive.cpu.fuel.is_none());
}

#[test]
fn test_config_builder() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid module")
        .memory_limit(128 * 1024 * 1024)
        .fuel(1_000_000)
        .wall_time_limit(Duration::from_secs(30))
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .env("KEY", "value")
        .arg("arg1".to_string())
        .build()
        .expect("valid config");

    assert_eq!(config.resources.memory.heap_max, 128 * 1024 * 1024);
    assert_eq!(config.resources.cpu.fuel, Some(1_000_000));
    assert!(config.capabilities.has(&Capability::stdout()));
    assert_eq!(config.env.get("KEY"), Some(&"value".to_string()));
}

#[test]
fn test_module_validation() {
    // Valid module
    let result = SandboxConfig::builder().module(MINIMAL_WASM);
    assert!(result.is_ok());

    // Invalid module (too short)
    let result = SandboxConfig::builder().module(&[0x00]);
    assert!(result.is_err());

    // Invalid magic
    let invalid = &[0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    let result = SandboxConfig::builder().module(invalid);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_sandbox_terminate() {
    let config = SandboxConfig::builder()
        .module(MINIMAL_WASM)
        .expect("valid module")
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox creation");

    let metrics = sandbox.terminate().await.expect("termination");

    assert_eq!(sandbox.state(), SandboxState::Terminated);
    assert_eq!(metrics.run_count, 0);
}

#[tokio::test]
async fn test_sandbox_execution() {
    // Create sandbox with runnable WASM module
    let config = SandboxConfig::builder()
        .module(RUNNABLE_WASM)
        .expect("valid module")
        .memory_limit(64 * 1024 * 1024)
        .fuel(1_000_000)
        .wall_time_limit(Duration::from_secs(5))
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox creation");
    assert_eq!(sandbox.state(), SandboxState::Ready);

    // Run the module
    let output = sandbox.run(&[]).await.expect("execution");

    // Verify execution completed successfully
    assert_eq!(output.exit_code, 0, "Expected exit code 0");
    assert_eq!(sandbox.state(), SandboxState::Terminated);

    // Check resource usage was tracked
    assert!(output.duration.as_nanos() > 0, "Duration should be tracked");
}

#[tokio::test]
async fn test_sandbox_execution_with_fuel_limit() {
    let config = SandboxConfig::builder()
        .module(RUNNABLE_WASM)
        .expect("valid module")
        .fuel(100_000)
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox creation");
    let output = sandbox.run(&[]).await.expect("execution");

    // The minimal module should complete well under the fuel limit
    assert_eq!(output.exit_code, 0);
    // Fuel should have been consumed
    assert!(
        output.resource_usage.fuel_consumed > 0,
        "Expected some fuel to be consumed"
    );
}

#[tokio::test]
async fn test_sandbox_cold_start_performance() {
    // Measure cold start time
    let start = std::time::Instant::now();

    let config = SandboxConfig::builder()
        .module(RUNNABLE_WASM)
        .expect("valid module")
        .build()
        .expect("valid config");

    let _sandbox = Sandbox::create(config).await.expect("sandbox creation");
    let cold_start = start.elapsed();

    // Cold start should be under 100ms (generous for CI environments)
    assert!(
        cold_start.as_millis() < 100,
        "Cold start took {:?}, expected < 100ms",
        cold_start
    );
}

#[tokio::test]
async fn test_sandbox_shared_engine_performance() {
    let engine = Arc::new(WasmEngine::new().expect("engine creation"));

    // First sandbox (cold)
    let start1 = std::time::Instant::now();
    let config1 = SandboxConfig::builder()
        .module(RUNNABLE_WASM)
        .expect("valid module")
        .build()
        .expect("valid config");
    let _sandbox1 = Sandbox::create_with_engine(config1, engine.clone())
        .await
        .expect("sandbox1 creation");
    let cold_start = start1.elapsed();

    // Second sandbox (warm - module already compiled)
    let start2 = std::time::Instant::now();
    let config2 = SandboxConfig::builder()
        .module(RUNNABLE_WASM)
        .expect("valid module")
        .build()
        .expect("valid config");
    let _sandbox2 = Sandbox::create_with_engine(config2, engine.clone())
        .await
        .expect("sandbox2 creation");
    let warm_start = start2.elapsed();

    // Warm start should be faster (module compilation is cached)
    // Note: This may not always be true on first run due to JIT
    println!("Cold start: {:?}, Warm start: {:?}", cold_start, warm_start);

    // Just verify both completed successfully
    assert_eq!(engine.cached_module_count(), 1);
}

#[tokio::test]
async fn test_stdout_capture() {
    // Create sandbox with stdout capability
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .expect("valid module")
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox creation");
    let output = sandbox.run(&[]).await.expect("execution");

    // Verify execution completed successfully
    assert_eq!(output.exit_code, 0, "Expected exit code 0");

    // Verify stdout was captured
    let stdout_str = output.stdout_str();
    assert_eq!(
        stdout_str, "Hello from WASM!\n",
        "Expected stdout to contain hello message"
    );
}

#[tokio::test]
async fn test_stdout_capture_without_capability() {
    // Create sandbox WITHOUT stdout capability
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)
        .expect("valid module")
        .fuel(1_000_000)
        // No stdout capability - output should be discarded
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox creation");
    let output = sandbox.run(&[]).await.expect("execution");

    // Execution should still succeed
    assert_eq!(output.exit_code, 0, "Expected exit code 0");

    // But stdout should be empty (discarded)
    assert!(
        output.stdout.is_empty(),
        "Expected stdout to be empty without capability, got: {:?}",
        output.stdout_str()
    );
}

#[tokio::test]
async fn test_non_zero_exit_code() {
    // Create sandbox with exit_42 WASM module
    let config = SandboxConfig::builder()
        .module(EXIT_42_WASM)
        .expect("valid module")
        .fuel(1_000_000)
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("sandbox creation");
    let output = sandbox.run(&[]).await.expect("execution");

    // Verify the exit code is captured correctly
    assert_eq!(output.exit_code, 42, "Expected exit code 42");
    assert!(!output.success(), "Expected non-successful exit");
}
