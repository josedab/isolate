//! Crate version and build information.
//!
//! Provides version strings and a [`BuildInfo`] struct listing compiled
//! features, the Wasmtime version, and the build target.
//!
//! # Example
//!
//! ```
//! use isolate_core::version::{version, build_info};
//!
//! let info = build_info();
//! assert!(!info.version.is_empty());
//! assert!(!info.wasmtime_version.is_empty());
//! println!("{}", info);
//! ```

/// Returns the current version of isolate-core.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns version information as a formatted string.
pub fn version_info() -> String {
    format!("isolate-core v{}", version())
}

/// Compile-time and runtime build metadata.
#[derive(Debug, Clone)]
pub struct BuildInfo {
    /// Crate version (from Cargo.toml).
    pub version: &'static str,
    /// Minimum supported Rust version.
    pub msrv: &'static str,
    /// Wasmtime runtime version.
    pub wasmtime_version: &'static str,
    /// Architecture (e.g., "x86_64", "aarch64").
    pub target: &'static str,
    /// Cargo profile (debug or release).
    pub profile: &'static str,
    /// Enabled feature flags at compile time.
    pub features: Vec<&'static str>,
}

impl std::fmt::Display for BuildInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "isolate-core v{} (wasmtime {}, {}, {})",
            self.version, self.wasmtime_version, self.target, self.profile
        )?;
        if !self.features.is_empty() {
            write!(f, " [{}]", self.features.join(", "))?;
        }
        Ok(())
    }
}

/// Returns build metadata for diagnostics and debugging.
pub fn build_info() -> BuildInfo {
    let mut features = Vec::new();

    macro_rules! check_feature {
        ($name:expr) => {
            if cfg!(feature = $name) {
                features.push($name);
            }
        };
    }

    check_feature!("pool");
    check_feature!("networking");
    check_feature!("agent");
    check_feature!("policy-engine");
    check_feature!("platform");
    check_feature!("extras");
    check_feature!("observability");
    check_feature!("billing");
    check_feature!("deployment");
    check_feature!("federation");
    check_feature!("snapshots");
    check_feature!("wasi-preview2");
    check_feature!("debug-support");
    check_feature!("module-signing");
    check_feature!("kubernetes");
    check_feature!("otel-telemetry");
    check_feature!("chaos-testing");
    check_feature!("gpu-compute");
    check_feature!("distributed-mesh");
    check_feature!("hotpatch");

    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };

    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        msrv: env!("CARGO_PKG_RUST_VERSION"),
        wasmtime_version: env!("WASMTIME_VERSION"),
        target: std::env::consts::ARCH,
        profile,
        features,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_not_empty() {
        assert!(!version().is_empty());
    }

    #[test]
    fn test_version_info_format() {
        let info = version_info();
        assert!(info.starts_with("isolate-core v"));
    }

    #[test]
    fn test_build_info_populated() {
        let info = build_info();
        assert!(!info.version.is_empty());
        assert!(!info.wasmtime_version.is_empty());
        assert!(!info.target.is_empty());
        assert_eq!(info.profile, "debug"); // tests run in debug profile
    }

    #[test]
    fn test_build_info_display() {
        let info = build_info();
        let display = format!("{}", info);
        assert!(display.contains("isolate-core v"));
        assert!(display.contains("wasmtime"));
    }
}
