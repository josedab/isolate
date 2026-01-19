//! Fuzz test for sandbox configuration.
//!
//! This target tests the configuration builder with arbitrary inputs
//! to ensure it handles edge cases gracefully.
//!
//! Run with: `cargo +nightly fuzz run fuzz_config`

#![no_main]

use arbitrary::Arbitrary;
use isolate_core::{capability::Capability, SandboxConfig};
use libfuzzer_sys::fuzz_target;
use std::time::Duration;

/// Arbitrary configuration parameters for fuzzing.
#[derive(Debug, Arbitrary)]
struct FuzzConfig {
    memory_limit: usize,
    stack_size: usize,
    fuel: Option<u64>,
    wall_time_ms: Option<u64>,
    cpu_time_ms: Option<u64>,
    io_read_limit: Option<u64>,
    io_write_limit: Option<u64>,
    env_vars: Vec<(String, String)>,
    args: Vec<String>,
    entry_point: String,
    enable_stdout: bool,
    enable_stderr: bool,
    fs_read_paths: Vec<String>,
    fs_write_paths: Vec<String>,
}

// Minimal valid WASM module for testing config
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x01, 0x00, 0x00, 0x00, // version: 1
];

fuzz_target!(|config: FuzzConfig| {
    let mut builder = match SandboxConfig::builder().module(MINIMAL_WASM) {
        Ok(b) => b,
        Err(_) => return,
    };

    // Apply memory limits (clamp to reasonable values)
    let memory_limit = config.memory_limit.clamp(1024, 1024 * 1024 * 1024);
    builder = builder.memory_limit(memory_limit);

    let stack_size = config.stack_size.clamp(1024, 64 * 1024 * 1024);
    builder = builder.stack_size(stack_size);

    // Apply fuel limit
    if let Some(fuel) = config.fuel {
        builder = builder.fuel(fuel);
    }

    // Apply time limits
    if let Some(wall_time_ms) = config.wall_time_ms {
        if wall_time_ms > 0 && wall_time_ms < 60_000 {
            builder = builder.wall_time_limit(Duration::from_millis(wall_time_ms));
        }
    }

    if let Some(cpu_time_ms) = config.cpu_time_ms {
        if cpu_time_ms > 0 && cpu_time_ms < 60_000 {
            builder = builder.cpu_time_limit(Duration::from_millis(cpu_time_ms));
        }
    }

    // Apply I/O limits
    if let Some(limit) = config.io_read_limit {
        builder = builder.io_read_limit(limit);
    }
    if let Some(limit) = config.io_write_limit {
        builder = builder.io_write_limit(limit);
    }

    // Apply capabilities
    if config.enable_stdout {
        builder = builder.capability(Capability::stdout());
    }
    if config.enable_stderr {
        builder = builder.capability(Capability::stderr());
    }

    // Apply filesystem capabilities (limit count to prevent OOM)
    for path in config.fs_read_paths.iter().take(10) {
        if !path.is_empty() && path.len() < 1000 {
            builder = builder.capability(Capability::filesystem_read(path));
        }
    }
    for path in config.fs_write_paths.iter().take(10) {
        if !path.is_empty() && path.len() < 1000 {
            builder = builder.capability(Capability::filesystem_write(path));
        }
    }

    // Apply environment variables (limit count)
    for (key, value) in config.env_vars.iter().take(50) {
        if !key.is_empty() && key.len() < 256 && value.len() < 4096 {
            builder = builder.env(key.clone(), value.clone());
        }
    }

    // Apply arguments (limit count)
    for arg in config.args.iter().take(100) {
        if arg.len() < 4096 {
            builder = builder.arg(arg.clone());
        }
    }

    // Apply entry point
    if !config.entry_point.is_empty() && config.entry_point.len() < 256 {
        builder = builder.entry_point(config.entry_point.clone());
    }

    // Try to build - should not panic
    let _ = builder.build();
});
