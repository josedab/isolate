//! Fuzz test for capability creation and enforcement.
//!
//! This target tests the capability system with arbitrary inputs.
//!
//! Run with: `cargo +nightly fuzz run fuzz_capability_parsing`

#![no_main]

use arbitrary::Arbitrary;
use isolate_core::capability::{Capability, CapabilitySet};
use libfuzzer_sys::fuzz_target;

/// Arbitrary capability configuration for fuzzing.
#[derive(Debug, Arbitrary)]
struct FuzzCapabilities {
    paths: Vec<String>,
    hosts: Vec<String>,
    env_vars: Vec<String>,
    operations: Vec<CapabilityOp>,
}

#[derive(Debug, Arbitrary)]
enum CapabilityOp {
    GrantStdout,
    GrantStderr,
    GrantStdin,
    GrantClock,
    GrantRandom,
    GrantFsRead(usize),  // index into paths
    GrantFsWrite(usize), // index into paths
    GrantEnvVar(usize),  // index into env_vars
    GrantHttpClient(usize), // index into hosts
    RevokeAll,
    CheckHas(usize), // check if capability at index exists
}

fuzz_target!(|input: FuzzCapabilities| {
    let mut cap_set = CapabilitySet::default();

    for op in input.operations.iter().take(1000) {
        match op {
            CapabilityOp::GrantStdout => {
                cap_set.grant(Capability::stdout());
            }
            CapabilityOp::GrantStderr => {
                cap_set.grant(Capability::stderr());
            }
            CapabilityOp::GrantStdin => {
                cap_set.grant(Capability::stdin());
            }
            CapabilityOp::GrantClock => {
                cap_set.grant(Capability::clock());
            }
            CapabilityOp::GrantRandom => {
                cap_set.grant(Capability::random());
            }
            CapabilityOp::GrantFsRead(idx) => {
                if let Some(path) = input.paths.get(*idx % input.paths.len().max(1)) {
                    if !path.is_empty() && path.len() < 4096 {
                        cap_set.grant(Capability::filesystem_read(path));
                    }
                }
            }
            CapabilityOp::GrantFsWrite(idx) => {
                if let Some(path) = input.paths.get(*idx % input.paths.len().max(1)) {
                    if !path.is_empty() && path.len() < 4096 {
                        cap_set.grant(Capability::filesystem_write(path));
                    }
                }
            }
            CapabilityOp::GrantEnvVar(idx) => {
                if let Some(var) = input.env_vars.get(*idx % input.env_vars.len().max(1)) {
                    if !var.is_empty() && var.len() < 256 {
                        cap_set.grant(Capability::env_var(var));
                    }
                }
            }
            CapabilityOp::GrantHttpClient(idx) => {
                if let Some(host) = input.hosts.get(*idx % input.hosts.len().max(1)) {
                    if !host.is_empty() && host.len() < 256 {
                        cap_set.grant(Capability::http_client(vec![host.clone()]));
                    }
                }
            }
            CapabilityOp::RevokeAll => {
                cap_set = CapabilitySet::default();
            }
            CapabilityOp::CheckHas(idx) => {
                // Check various capabilities
                let _ = cap_set.has(&Capability::stdout());
                let _ = cap_set.has(&Capability::stderr());
                if let Some(path) = input.paths.get(*idx % input.paths.len().max(1)) {
                    if !path.is_empty() {
                        let _ = cap_set.has(&Capability::filesystem_read(path));
                    }
                }
            }
        }
    }

    // Verify internal consistency
    let _ = cap_set.has(&Capability::stdout());
    let _ = cap_set.has(&Capability::stderr());
});
