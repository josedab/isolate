//! Module marketplace and registry for verified WASM modules.
//!
//! Provides an in-process registry for storing module metadata, performing
//! basic security scanning, and enabling search/retrieval of published modules.
//!
//! Also includes the full Plugin Marketplace Protocol with manifest-based
//! discovery, distribution, and trust for WASM modules.

// This module is experimental and not all APIs are used yet.
#![allow(dead_code)]

pub mod analytics;
pub mod content_store;
pub mod curation;
pub mod monetization;
mod registry;
pub mod resolver;
pub mod reviews;
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
pub use scanner::{ModuleScanner, ScanResult, ScanFinding, FindingSeverity};
pub use curation::{CurationEngine, CertificationTier, QualityGateConfig, QualityReport, QualityCheck, FeaturedListing};
pub use monetization::{MonetizationEngine, PricingModel, RevenueShare, MonetizedListing, PublisherPayout};
pub use reviews::{ReviewSystem, Review, ReviewStatus, RatingStats, ReviewError};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Lightweight module registry types (always available, no feature gate)
// ---------------------------------------------------------------------------

/// Risk level classification for security findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketplaceRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl MarketplaceRiskLevel {
    fn ordinal(&self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }
}

impl PartialOrd for MarketplaceRiskLevel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MarketplaceRiskLevel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.ordinal().cmp(&other.ordinal())
    }
}

impl fmt::Display for MarketplaceRiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

/// A single finding from a security scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub severity: MarketplaceRiskLevel,
    pub category: String,
    pub description: String,
}

/// Result of a heuristic security scan on a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScan {
    pub scanned_at: String,
    pub risk_level: MarketplaceRiskLevel,
    pub findings: Vec<SecurityFinding>,
    pub imports_count: usize,
    pub exports_count: usize,
    pub uses_memory: bool,
    pub uses_table: bool,
}

/// Metadata for a published module in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub size_bytes: usize,
    pub hash: String,
    pub created_at: String,
    pub updated_at: String,
    pub downloads: u64,
    pub verified: bool,
    pub security_scan: Option<SecurityScan>,
    pub required_capabilities: Vec<String>,
    pub recommended_profile: Option<String>,
}

/// Builder for creating [`ModuleEntry`] instances.
pub struct ModuleEntryBuilder {
    name: String,
    version: String,
    size_bytes: usize,
    hash: String,
    description: String,
    author: String,
    license: Option<String>,
    tags: Vec<String>,
    verified: bool,
    security_scan: Option<SecurityScan>,
    required_capabilities: Vec<String>,
    recommended_profile: Option<String>,
}

impl ModuleEntryBuilder {
    /// Create a new builder, computing hash and size from WASM bytes.
    pub fn new(name: impl Into<String>, version: impl Into<String>, wasm_bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        Self {
            name: name.into(),
            version: version.into(),
            size_bytes: wasm_bytes.len(),
            hash,
            description: String::new(),
            author: String::new(),
            license: None,
            tags: Vec::new(),
            verified: false,
            security_scan: None,
            required_capabilities: Vec::new(),
            recommended_profile: None,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn verified(mut self, verified: bool) -> Self {
        self.verified = verified;
        self
    }

    pub fn security_scan(mut self, scan: SecurityScan) -> Self {
        self.security_scan = Some(scan);
        self
    }

    pub fn required_capabilities(mut self, caps: Vec<String>) -> Self {
        self.required_capabilities = caps;
        self
    }

    pub fn recommended_profile(mut self, profile: impl Into<String>) -> Self {
        self.recommended_profile = Some(profile.into());
        self
    }

    /// Build the [`ModuleEntry`] with current timestamps.
    pub fn build(self) -> ModuleEntry {
        let now = chrono::Utc::now().to_rfc3339();
        ModuleEntry {
            name: self.name,
            version: self.version,
            description: self.description,
            author: self.author,
            license: self.license,
            tags: self.tags,
            size_bytes: self.size_bytes,
            hash: self.hash,
            created_at: now.clone(),
            updated_at: now,
            downloads: 0,
            verified: self.verified,
            security_scan: self.security_scan,
            required_capabilities: self.required_capabilities,
            recommended_profile: self.recommended_profile,
        }
    }
}

/// Perform a heuristic security scan of raw WASM bytes.
///
/// This is a lightweight static analysis that looks for WASM section headers
/// to count imports/exports and detect memory/table usage.
pub fn scan_wasm_module(wasm_bytes: &[u8]) -> SecurityScan {
    let mut imports_count = 0;
    let mut exports_count = 0;
    let mut uses_memory = false;
    let mut uses_table = false;
    let mut findings = Vec::new();

    // Skip the 8-byte WASM header and walk section headers.
    let mut pos = 8;
    while pos < wasm_bytes.len() {
        let section_id = wasm_bytes[pos];
        pos += 1;

        // Decode LEB128 section length
        let mut section_len: usize = 0;
        let mut shift = 0;
        while pos < wasm_bytes.len() {
            let byte = wasm_bytes[pos];
            pos += 1;
            section_len |= ((byte & 0x7F) as usize) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }

        let section_end = pos + section_len;

        match section_id {
            0x02 => {
                // Import section — first value is the count (LEB128)
                if pos < section_end {
                    let mut count: usize = 0;
                    let mut s = 0;
                    while pos < section_end {
                        let byte = wasm_bytes[pos];
                        pos += 1;
                        count |= ((byte & 0x7F) as usize) << s;
                        if byte & 0x80 == 0 {
                            break;
                        }
                        s += 7;
                    }
                    imports_count = count;
                }
            }
            0x07 => {
                // Export section — first value is the count (LEB128)
                if pos < section_end {
                    let mut count: usize = 0;
                    let mut s = 0;
                    while pos < section_end {
                        let byte = wasm_bytes[pos];
                        pos += 1;
                        count |= ((byte & 0x7F) as usize) << s;
                        if byte & 0x80 == 0 {
                            break;
                        }
                        s += 7;
                    }
                    exports_count = count;
                }
            }
            0x05 => uses_memory = true,
            0x04 => uses_table = true,
            _ => {}
        }

        pos = section_end;
    }

    let risk_level = if imports_count == 0 {
        MarketplaceRiskLevel::Low
    } else if imports_count > 10 {
        MarketplaceRiskLevel::High
    } else {
        MarketplaceRiskLevel::Medium
    };

    if imports_count > 10 {
        findings.push(SecurityFinding {
            severity: MarketplaceRiskLevel::High,
            category: "imports".to_string(),
            description: format!("Module has {} imports, which is unusually high", imports_count),
        });
    }

    if uses_memory && imports_count > 0 {
        findings.push(SecurityFinding {
            severity: MarketplaceRiskLevel::Medium,
            category: "memory".to_string(),
            description: "Module uses memory and has imports".to_string(),
        });
    }

    let now = chrono::Utc::now().to_rfc3339();
    SecurityScan {
        scanned_at: now,
        risk_level,
        findings,
        imports_count,
        exports_count,
        uses_memory,
        uses_table,
    }
}

/// In-memory registry of published WASM modules.
pub struct ModuleRegistry {
    modules: HashMap<String, Vec<ModuleEntry>>,
}

impl ModuleRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    /// Publish a module entry. Returns an error if name or version is empty,
    /// or if the same name+version already exists.
    pub fn publish(&mut self, entry: ModuleEntry) -> Result<(), String> {
        if entry.name.is_empty() {
            return Err("Module name cannot be empty".to_string());
        }
        if entry.version.is_empty() {
            return Err("Module version cannot be empty".to_string());
        }

        let versions = self.modules.entry(entry.name.clone()).or_default();
        if versions.iter().any(|e| e.version == entry.version) {
            return Err(format!(
                "Version {} of module '{}' already exists",
                entry.version, entry.name
            ));
        }
        versions.push(entry);
        Ok(())
    }

    /// Get a specific version of a module.
    pub fn get(&self, name: &str, version: &str) -> Option<&ModuleEntry> {
        self.modules
            .get(name)
            .and_then(|vs| vs.iter().find(|e| e.version == version))
    }

    /// Get the latest version of a module (highest by string comparison).
    pub fn get_latest(&self, name: &str) -> Option<&ModuleEntry> {
        self.modules
            .get(name)
            .and_then(|vs| vs.iter().max_by(|a, b| a.version.cmp(&b.version)))
    }

    /// Search modules by name, description, or tags (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&ModuleEntry> {
        let q = query.to_lowercase();
        self.modules
            .values()
            .flatten()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// List the latest version of every module.
    pub fn list_all(&self) -> Vec<&ModuleEntry> {
        self.modules
            .keys()
            .filter_map(|name| self.get_latest(name))
            .collect()
    }

    /// List modules that have a specific tag.
    pub fn list_by_tag(&self, tag: &str) -> Vec<&ModuleEntry> {
        let t = tag.to_lowercase();
        self.modules
            .values()
            .flatten()
            .filter(|e| e.tags.iter().any(|et| et.to_lowercase() == t))
            .collect()
    }

    /// List only verified modules.
    pub fn list_verified(&self) -> Vec<&ModuleEntry> {
        self.modules
            .values()
            .flatten()
            .filter(|e| e.verified)
            .collect()
    }

    /// Remove a specific version. Returns `true` if it was found and removed.
    pub fn remove(&mut self, name: &str, version: &str) -> bool {
        if let Some(versions) = self.modules.get_mut(name) {
            let before = versions.len();
            versions.retain(|e| e.version != version);
            let removed = versions.len() < before;
            if versions.is_empty() {
                self.modules.remove(name);
            }
            removed
        } else {
            false
        }
    }

    /// Number of unique module names in the registry.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // \0asm
        0x01, 0x00, 0x00, 0x00, // version 1
    ];

    // -- Existing test --

    #[test]
    fn test_module_exports() {
        let registry = Registry::new(RegistryConfig::default());
        assert_eq!(registry.count(), 0);
    }

    // -- New marketplace tests --

    fn make_entry(name: &str, version: &str) -> ModuleEntry {
        ModuleEntryBuilder::new(name, version, MINIMAL_WASM)
            .description("A test module")
            .author("tester")
            .build()
    }

    #[test]
    fn test_marketplace_builder_creates_valid_entry() {
        let entry = ModuleEntryBuilder::new("my-mod", "1.0.0", MINIMAL_WASM)
            .description("desc")
            .author("alice")
            .license("MIT")
            .tags(vec!["wasm".to_string()])
            .verified(true)
            .required_capabilities(vec!["stdio:stdout".to_string()])
            .recommended_profile("compute")
            .build();

        assert_eq!(entry.name, "my-mod");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.description, "desc");
        assert_eq!(entry.author, "alice");
        assert_eq!(entry.license, Some("MIT".to_string()));
        assert_eq!(entry.tags, vec!["wasm".to_string()]);
        assert!(entry.verified);
        assert_eq!(entry.size_bytes, MINIMAL_WASM.len());
        assert!(!entry.hash.is_empty());
        assert!(!entry.created_at.is_empty());
        assert!(!entry.updated_at.is_empty());
        assert_eq!(entry.downloads, 0);
        assert_eq!(entry.required_capabilities, vec!["stdio:stdout".to_string()]);
        assert_eq!(entry.recommended_profile, Some("compute".to_string()));
    }

    #[test]
    fn test_marketplace_registry_publish_and_get() {
        let mut reg = ModuleRegistry::new();
        let entry = make_entry("foo", "1.0.0");
        reg.publish(entry).unwrap();

        let found = reg.get("foo", "1.0.0").unwrap();
        assert_eq!(found.name, "foo");
        assert_eq!(found.version, "1.0.0");
    }

    #[test]
    fn test_marketplace_publish_duplicate_version_fails() {
        let mut reg = ModuleRegistry::new();
        reg.publish(make_entry("foo", "1.0.0")).unwrap();
        let result = reg.publish(make_entry("foo", "1.0.0"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_marketplace_get_latest_returns_highest_version() {
        let mut reg = ModuleRegistry::new();
        reg.publish(make_entry("foo", "0.1.0")).unwrap();
        reg.publish(make_entry("foo", "1.0.0")).unwrap();
        reg.publish(make_entry("foo", "0.9.0")).unwrap();

        let latest = reg.get_latest("foo").unwrap();
        assert_eq!(latest.version, "1.0.0");
    }

    #[test]
    fn test_marketplace_search_by_name() {
        let mut reg = ModuleRegistry::new();
        reg.publish(make_entry("hello-world", "1.0.0")).unwrap();
        reg.publish(make_entry("goodbye", "1.0.0")).unwrap();

        let results = reg.search("hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "hello-world");
    }

    #[test]
    fn test_marketplace_search_by_description() {
        let mut reg = ModuleRegistry::new();
        let mut entry = make_entry("mod-a", "1.0.0");
        entry.description = "A crypto library".to_string();
        reg.publish(entry).unwrap();

        let results = reg.search("crypto");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "mod-a");
    }

    #[test]
    fn test_marketplace_search_by_tag() {
        let mut reg = ModuleRegistry::new();
        let entry = ModuleEntryBuilder::new("tagged", "1.0.0", MINIMAL_WASM)
            .tags(vec!["networking".to_string()])
            .build();
        reg.publish(entry).unwrap();

        let results = reg.search("networking");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "tagged");
    }

    #[test]
    fn test_marketplace_list_all_returns_latest_versions_only() {
        let mut reg = ModuleRegistry::new();
        reg.publish(make_entry("a", "1.0.0")).unwrap();
        reg.publish(make_entry("a", "2.0.0")).unwrap();
        reg.publish(make_entry("b", "1.0.0")).unwrap();

        let all = reg.list_all();
        assert_eq!(all.len(), 2);

        let a_entry = all.iter().find(|e| e.name == "a").unwrap();
        assert_eq!(a_entry.version, "2.0.0");
    }

    #[test]
    fn test_marketplace_list_by_tag_filters_correctly() {
        let mut reg = ModuleRegistry::new();
        let entry1 = ModuleEntryBuilder::new("m1", "1.0.0", MINIMAL_WASM)
            .tags(vec!["web".to_string()])
            .build();
        let entry2 = ModuleEntryBuilder::new("m2", "1.0.0", MINIMAL_WASM)
            .tags(vec!["cli".to_string()])
            .build();
        reg.publish(entry1).unwrap();
        reg.publish(entry2).unwrap();

        let web = reg.list_by_tag("web");
        assert_eq!(web.len(), 1);
        assert_eq!(web[0].name, "m1");
    }

    #[test]
    fn test_marketplace_list_verified_filters_correctly() {
        let mut reg = ModuleRegistry::new();
        let v = ModuleEntryBuilder::new("verified-mod", "1.0.0", MINIMAL_WASM)
            .verified(true)
            .build();
        let u = ModuleEntryBuilder::new("unverified-mod", "1.0.0", MINIMAL_WASM)
            .verified(false)
            .build();
        reg.publish(v).unwrap();
        reg.publish(u).unwrap();

        let verified = reg.list_verified();
        assert_eq!(verified.len(), 1);
        assert_eq!(verified[0].name, "verified-mod");
    }

    #[test]
    fn test_marketplace_remove_works() {
        let mut reg = ModuleRegistry::new();
        reg.publish(make_entry("rm-me", "1.0.0")).unwrap();
        assert!(reg.remove("rm-me", "1.0.0"));
        assert!(reg.get("rm-me", "1.0.0").is_none());
        assert!(!reg.remove("rm-me", "1.0.0"));
    }

    #[test]
    fn test_marketplace_module_count() {
        let mut reg = ModuleRegistry::new();
        assert_eq!(reg.module_count(), 0);
        reg.publish(make_entry("a", "1.0.0")).unwrap();
        reg.publish(make_entry("b", "1.0.0")).unwrap();
        reg.publish(make_entry("a", "2.0.0")).unwrap();
        assert_eq!(reg.module_count(), 2);
    }

    #[test]
    fn test_marketplace_scan_hello_wasm() {
        let wasm = include_bytes!("../../tests/fixtures/hello.wasm");
        let scan = scan_wasm_module(wasm);
        assert!(scan.imports_count > 0);
        assert!(scan.exports_count > 0);
    }

    #[test]
    fn test_marketplace_scan_minimal_wasm() {
        let wasm = include_bytes!("../../tests/fixtures/minimal.wasm");
        let scan = scan_wasm_module(wasm);
        // minimal.wasm imports wasi_snapshot_preview1.proc_exit, so it has imports
        assert!(scan.imports_count > 0);
        assert!(scan.uses_memory);
    }

    #[test]
    fn test_marketplace_risk_level_ordering() {
        assert!(MarketplaceRiskLevel::Low < MarketplaceRiskLevel::Medium);
        assert!(MarketplaceRiskLevel::Medium < MarketplaceRiskLevel::High);
        assert!(MarketplaceRiskLevel::High < MarketplaceRiskLevel::Critical);
    }

    #[test]
    fn test_marketplace_risk_level_display() {
        assert_eq!(MarketplaceRiskLevel::Low.to_string(), "Low");
        assert_eq!(MarketplaceRiskLevel::Medium.to_string(), "Medium");
        assert_eq!(MarketplaceRiskLevel::High.to_string(), "High");
        assert_eq!(MarketplaceRiskLevel::Critical.to_string(), "Critical");
    }

    #[test]
    fn test_marketplace_security_finding_creation() {
        let finding = SecurityFinding {
            severity: MarketplaceRiskLevel::High,
            category: "imports".to_string(),
            description: "Too many imports".to_string(),
        };
        assert_eq!(finding.severity, MarketplaceRiskLevel::High);
        assert_eq!(finding.category, "imports");
        assert_eq!(finding.description, "Too many imports");
    }

    #[test]
    fn test_marketplace_empty_registry_operations() {
        let reg = ModuleRegistry::new();
        assert_eq!(reg.module_count(), 0);
        assert!(reg.get("nonexistent", "1.0.0").is_none());
        assert!(reg.get_latest("nonexistent").is_none());
        assert!(reg.search("anything").is_empty());
        assert!(reg.list_all().is_empty());
        assert!(reg.list_by_tag("tag").is_empty());
        assert!(reg.list_verified().is_empty());
    }
}
