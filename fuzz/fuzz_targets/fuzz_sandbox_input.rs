//! Fuzz test for sandbox input handling.
//!
//! This target tests how the sandbox handles arbitrary input data
//! passed to the WASM module.
//!
//! Run with: `cargo +nightly fuzz run fuzz_sandbox_input`

#![no_main]

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use libfuzzer_sys::fuzz_target;
use std::time::Duration;

// A simple WASM module that reads stdin (hello.wasm from fixtures)
const HELLO_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/hello.wasm");

fuzz_target!(|data: &[u8]| {
    // Limit input size to prevent OOM
    if data.len() > 1024 * 1024 {
        return;
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let config = SandboxConfig::builder()
            .module(HELLO_WASM)
            .unwrap()
            .memory_limit(32 * 1024 * 1024) // 32MB
            .fuel(1_000_000)
            .wall_time_limit(Duration::from_millis(500))
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .capability(Capability::stdin())
            .build()
            .unwrap();

        if let Ok(mut sandbox) = Sandbox::create(config).await {
            // Run with the fuzzed input
            let _ = sandbox.run(data).await;
        }
    });
});
