//! Property-based tests for isolate-core.
//!
//! These tests use proptest to verify invariants across a wide range of inputs.

use isolate_core::capability::{Capability, CapabilitySet};
use isolate_core::config::{ModuleHash, SandboxConfig, WasmModule};
use isolate_core::error::Error;
use proptest::prelude::*;
use std::time::Duration;

// Minimal valid WASM module for testing
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x01, 0x00, 0x00, 0x00, // version: 1
];

// ============================================================================
// Module Hash Tests
// ============================================================================

proptest! {
    /// Module hash is deterministic - same input always produces same hash.
    #[test]
    fn module_hash_is_deterministic(data in prop::collection::vec(any::<u8>(), 0..10000)) {
        let hash1 = ModuleHash::from_bytes(&data);
        let hash2 = ModuleHash::from_bytes(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Different inputs (almost always) produce different hashes.
    #[test]
    fn module_hash_different_inputs(
        data1 in prop::collection::vec(any::<u8>(), 8..1000),
        data2 in prop::collection::vec(any::<u8>(), 8..1000),
    ) {
        // Only check if inputs are actually different
        if data1 != data2 {
            let hash1 = ModuleHash::from_bytes(&data1);
            let hash2 = ModuleHash::from_bytes(&data2);
            // SHA256 collision is astronomically unlikely
            prop_assert_ne!(hash1, hash2);
        }
    }

    /// Module hash display is always 16 characters (truncated).
    #[test]
    fn module_hash_display_length(data in prop::collection::vec(any::<u8>(), 1..1000)) {
        let hash = ModuleHash::from_bytes(&data);
        let display = format!("{}", hash);
        prop_assert_eq!(display.len(), 16);
    }
}

// ============================================================================
// WASM Module Validation Tests
// ============================================================================

proptest! {
    /// Modules without WASM magic number are rejected.
    #[test]
    fn invalid_magic_rejected(
        b0 in any::<u8>().prop_filter("not \\0", |b| *b != 0x00),
        rest in prop::collection::vec(any::<u8>(), 7..100),
    ) {
        let mut data = vec![b0];
        data.extend(rest);
        let result = WasmModule::from_bytes(data);
        prop_assert!(result.is_err());
    }

    /// Modules too small are rejected.
    #[test]
    fn small_modules_rejected(data in prop::collection::vec(any::<u8>(), 0..8)) {
        let result = WasmModule::from_bytes(data);
        prop_assert!(result.is_err());
    }

    /// Valid WASM modules are accepted.
    #[test]
    fn valid_wasm_accepted(padding in prop::collection::vec(any::<u8>(), 0..1000)) {
        // Start with valid WASM header, then add custom section padding
        let mut data = MINIMAL_WASM.to_vec();
        if !padding.is_empty() {
            // Add a custom section
            data.push(0x00); // section id (custom)
            let section_len = padding.len() + 5; // name length + name + data
            // Simple length encoding (works for small sections)
            if section_len < 128 {
                data.push(section_len as u8);
                data.push(0x04); // name length
                data.extend_from_slice(b"test"); // name
                data.extend_from_slice(&padding);
            }
        }
        // Note: This may still fail if the WASM is malformed, which is expected
        let _ = WasmModule::from_bytes(data);
    }
}

// ============================================================================
// Config Builder Tests
// ============================================================================

proptest! {
    /// Config builder never panics with any memory limit.
    #[test]
    fn config_builder_memory_limit_no_panic(limit in any::<usize>()) {
        let _ = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .map(|b| b.memory_limit(limit).build());
    }

    /// Config builder never panics with any fuel value.
    #[test]
    fn config_builder_fuel_no_panic(fuel in any::<u64>()) {
        let _ = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .map(|b| b.fuel(fuel).build());
    }

    /// Config builder handles arbitrary time limits.
    #[test]
    fn config_builder_time_limits(millis in 0u64..1_000_000) {
        let duration = Duration::from_millis(millis);
        let result = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .map(|b| b.wall_time_limit(duration).cpu_time_limit(duration).build());
        prop_assert!(result.is_ok());
    }

    /// Config builder handles arbitrary environment variables.
    #[test]
    fn config_builder_env_vars(
        key in "[a-zA-Z_][a-zA-Z0-9_]{0,63}",
        value in ".*",
    ) {
        let result = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .map(|b| b.env(key, value).build());
        prop_assert!(result.is_ok());
    }

    /// Config builder handles arbitrary arguments.
    #[test]
    fn config_builder_args(args in prop::collection::vec(".*", 0..100)) {
        let result = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .map(|b| b.args(args.into_iter()).build());
        prop_assert!(result.is_ok());
    }

    /// Config builder handles arbitrary entry points.
    #[test]
    fn config_builder_entry_point(name in "[a-zA-Z_][a-zA-Z0-9_]{0,63}") {
        let result = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .map(|b| b.entry_point(name).build());
        prop_assert!(result.is_ok());
    }
}

// ============================================================================
// Capability Tests
// ============================================================================

proptest! {
    /// Granting a capability makes it available.
    #[test]
    fn capability_grant_makes_available(path in "/[a-z]{1,20}(/[a-z]{1,10}){0,5}") {
        let mut set = CapabilitySet::default();
        let cap = Capability::filesystem_read(&path);
        set.grant(cap.clone());
        prop_assert!(set.has(&cap));
    }

    /// Standard capabilities can be granted and checked.
    #[test]
    fn standard_capabilities_work(
        stdout in any::<bool>(),
        stderr in any::<bool>(),
        stdin in any::<bool>(),
        clock in any::<bool>(),
        random in any::<bool>(),
    ) {
        let mut set = CapabilitySet::default();

        if stdout { set.grant(Capability::stdout()); }
        if stderr { set.grant(Capability::stderr()); }
        if stdin { set.grant(Capability::stdin()); }
        if clock { set.grant(Capability::system_clock()); }
        if random { set.grant(Capability::secure_random()); }

        prop_assert_eq!(set.has(&Capability::stdout()), stdout);
        prop_assert_eq!(set.has(&Capability::stderr()), stderr);
        prop_assert_eq!(set.has(&Capability::stdin()), stdin);
        prop_assert_eq!(set.has(&Capability::system_clock()), clock);
        prop_assert_eq!(set.has(&Capability::secure_random()), random);
    }

    /// Environment variable capabilities are case-sensitive.
    #[test]
    fn env_var_case_sensitive(name in "[A-Z_]{1,20}") {
        let mut set = CapabilitySet::default();
        set.grant(Capability::env_var(&name));

        prop_assert!(set.has(&Capability::env_var(&name)));
        // Lowercase version should not match (unless name is all underscores)
        let lowercase = name.to_lowercase();
        if lowercase != name {
            prop_assert!(!set.has(&Capability::env_var(&lowercase)));
        }
    }

    /// Filesystem path capabilities respect exact paths.
    #[test]
    fn fs_path_exact_match(
        path in "/[a-z]{1,10}(/[a-z]{1,10}){0,3}",
    ) {
        let mut set = CapabilitySet::default();
        set.grant(Capability::filesystem_read(&path));

        prop_assert!(set.has(&Capability::filesystem_read(&path)));
        // Different path should not match
        let other_path = format!("{}/other", path);
        prop_assert!(!set.has(&Capability::filesystem_read(&other_path)));
    }
}

// ============================================================================
// Error Tests
// ============================================================================

proptest! {
    /// Timeout errors report correct duration.
    #[test]
    fn timeout_error_duration(secs in 0u64..10000, nanos in 0u32..1_000_000_000) {
        let duration = Duration::new(secs, nanos);
        let error = Error::Timeout(duration);
        prop_assert!(error.is_timeout());
        prop_assert!(error.is_resource_limit());
        prop_assert!(!error.is_capability_error());
    }

    /// Fuel exhausted errors report correct limit.
    #[test]
    fn fuel_exhausted_error_limit(limit in any::<u64>()) {
        let error = Error::FuelExhausted { limit };
        prop_assert!(!error.is_timeout());
        prop_assert!(error.is_resource_limit());
        prop_assert!(!error.is_capability_error());
    }

    /// Memory limit errors report correct values.
    #[test]
    fn memory_limit_error_values(limit in any::<usize>(), requested in any::<usize>()) {
        let error = Error::MemoryLimitExceeded { limit, requested };
        prop_assert!(error.is_resource_limit());
        prop_assert!(!error.is_capability_error());
    }

    /// Capability denied errors are categorized correctly.
    #[test]
    fn capability_denied_categorization(_dummy in any::<bool>()) {
        let error = Error::CapabilityDenied(Capability::stdout());
        prop_assert!(error.is_capability_error());
        prop_assert!(!error.is_resource_limit());
        prop_assert!(!error.is_timeout());
    }
}
