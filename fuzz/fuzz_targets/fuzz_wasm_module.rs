//! Fuzz test for WASM module parsing and sandbox creation.
//!
//! This target attempts to create sandboxes from arbitrary byte sequences,
//! testing the robustness of WASM validation and sandbox creation.
//!
//! Run with: `cargo +nightly fuzz run fuzz_wasm_module`

#![no_main]

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use libfuzzer_sys::fuzz_target;
use std::time::Duration;

fuzz_target!(|data: &[u8]| {
    // Skip very small inputs that can't be valid WASM
    if data.len() < 8 {
        return;
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        // Try to create a config with the fuzzed data as WASM
        let config_result = SandboxConfig::builder()
            .module(data)
            .map(|builder| {
                builder
                    // Set strict resource limits to prevent runaway execution
                    .memory_limit(16 * 1024 * 1024) // 16MB
                    .fuel(100_000) // Limited instructions
                    .wall_time_limit(Duration::from_millis(100))
                    .capability(Capability::stdout())
                    .capability(Capability::stderr())
                    .build()
            });

        // If config creation succeeded, try to create and run the sandbox
        if let Ok(Ok(config)) = config_result {
            if let Ok(mut sandbox) = Sandbox::create(config).await {
                // Try to run with empty input
                let _ = sandbox.run(&[]).await;
            }
        }
    });
});
