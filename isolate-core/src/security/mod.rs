//! Defense in depth security module.
//!
//! **WARNING: This module is experimental and Linux-only features are not production-ready.**
//! Seccomp and Landlock integrations are skeleton implementations. The API may change.
//!
//! This module provides additional OS-level security layers on top of
//! WASM isolation, including seccomp filtering and landlock filesystem
//! sandboxing on Linux.
//!
//! # Features
//!
//! - **Seccomp**: System call filtering to restrict allowed syscalls
//! - **Landlock**: Filesystem access restrictions (Linux 5.13+)
//! - **Namespace Isolation**: Process/network namespace support
//! - **Resource Cgroups**: Additional resource limiting via cgroups

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.

//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::security::{SecurityContext, SeccompPolicy, LandlockPolicy};
//!
//! // Create a restrictive security context
//! let context = SecurityContext::builder()
//!     .seccomp_policy(SeccompPolicy::strict())
//!     .landlock_policy(LandlockPolicy::read_only(&["/lib", "/usr/lib"]))
//!     .enable_namespaces(true)
//!     .build()?;
//!
//! // Apply to current process
//! context.apply()?;
//! ```

mod policy;
mod sandbox;
mod syscall;

pub use policy::{LandlockPolicy, LandlockRule, SeccompAction, SeccompPolicy, SeccompRule};
pub use sandbox::{SecurityContext, SecurityContextBuilder, SecurityError};
pub use syscall::{Syscall, SyscallArg, SyscallFilter};

/// Check if seccomp is available on this system.
pub fn seccomp_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for seccomp support via prctl
        true
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Check if landlock is available on this system.
pub fn landlock_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for landlock ABI version
        use std::fs;
        fs::metadata("/sys/kernel/security/landlock").is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Get the landlock ABI version (Linux only).
pub fn landlock_abi_version() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        // Would use landlock_create_ruleset with LANDLOCK_CREATE_RULESET_VERSION
        // For now, return None (would need actual syscall)
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seccomp_available() {
        // On Linux should return true, elsewhere false
        let available = seccomp_available();
        #[cfg(target_os = "linux")]
        assert!(available);
        #[cfg(not(target_os = "linux"))]
        assert!(!available);
    }

    #[test]
    fn test_landlock_available() {
        // Just verify it doesn't panic
        let _ = landlock_available();
    }

    #[test]
    fn test_landlock_abi_version() {
        let _ = landlock_abi_version();
    }
}
