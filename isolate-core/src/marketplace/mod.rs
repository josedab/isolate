//! Plugin Marketplace Protocol.
//!
//! Standardized plugin discovery, distribution, and trust for WASM modules.
//!
//! # Features
//!
//! - **Module Manifest**: Capability declaration format
//! - **Registry**: Module search, versioning, and distribution
//! - **Trust Levels**: Signed module verification
//! - **Compatibility**: Version constraint checking

// This module is experimental and not all APIs are used yet.
#![allow(dead_code)]

pub mod analytics;
pub mod content_store;
mod registry;
pub mod resolver;
pub mod scanner;
pub mod search;
pub mod verification;

pub use analytics::{AnalyticsTracker, DownloadRecord, ModuleStats, TrendingModule};
pub use registry::{
    ModuleManifest, ModuleVersion, Registry, RegistryConfig, RegistryEntry, SearchQuery,
    SearchResult, TrustLevel, VersionConstraint,
};
pub use resolver::{DependencyNode, DependencyResolver, ResolveError, ResolvedDependency};
pub use search::{
    IndexEntry, SearchEngine, SearchFacets, SearchField, SearchFilter, SearchHit, SearchResults,
};
pub use verification::{Badge, ModuleVerifier, RiskScore, VerificationCheck, VerificationReport};
pub use scanner::{ModuleScanner, ScanResult, ScanFinding, FindingSeverity, RiskLevel};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let registry = Registry::new(RegistryConfig::default());
        assert_eq!(registry.count(), 0);
    }
}
