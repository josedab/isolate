//! Module curation and quality gates for the marketplace.
//!
//! Provides certification tiers, featured listings, and automated quality scoring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Certification tier indicating trust and quality level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum CertificationTier {
    /// Community-submitted, basic automated checks passed.
    Community,
    /// Verified by automated security scanning pipeline.
    Verified,
    /// Manually reviewed and approved by curators.
    Curated,
    /// Enterprise-grade: full audit, SLA-backed, compliance-certified.
    Enterprise,
}

impl std::fmt::Display for CertificationTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Community => write!(f, "community"),
            Self::Verified => write!(f, "verified"),
            Self::Curated => write!(f, "curated"),
            Self::Enterprise => write!(f, "enterprise"),
        }
    }
}

/// Quality gate check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityCheck {
    pub name: String,
    pub passed: bool,
    pub score: f64,
    pub details: String,
}

/// Quality report for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub module_id: String,
    pub checks: Vec<QualityCheck>,
    pub overall_score: f64,
    pub tier: CertificationTier,
    pub generated_at: u64,
}

impl QualityReport {
    pub fn passed_all(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    pub fn failed_checks(&self) -> Vec<&QualityCheck> {
        self.checks.iter().filter(|c| !c.passed).collect()
    }
}

/// Featured listing with editorial metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturedListing {
    pub module_id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub featured_at: u64,
    pub expires_at: Option<u64>,
    pub priority: u32,
}

/// Configuration for the quality gate thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityGateConfig {
    pub min_score_community: f64,
    pub min_score_verified: f64,
    pub min_score_curated: f64,
    pub min_score_enterprise: f64,
    pub require_tests: bool,
    pub require_docs: bool,
    pub max_cve_severity: String,
}

impl Default for QualityGateConfig {
    fn default() -> Self {
        Self {
            min_score_community: 3.0,
            min_score_verified: 5.0,
            min_score_curated: 7.0,
            min_score_enterprise: 9.0,
            require_tests: true,
            require_docs: true,
            max_cve_severity: "medium".to_string(),
        }
    }
}

/// Curation engine managing quality gates, certification, and featured listings.
#[derive(Clone)]
pub struct CurationEngine {
    inner: Arc<CurationEngineInner>,
}

struct CurationEngineInner {
    config: QualityGateConfig,
    certifications: RwLock<HashMap<String, CertificationTier>>,
    reports: RwLock<HashMap<String, QualityReport>>,
    featured: RwLock<Vec<FeaturedListing>>,
}

impl CurationEngine {
    pub fn new(config: QualityGateConfig) -> Self {
        Self {
            inner: Arc::new(CurationEngineInner {
                config,
                certifications: RwLock::new(HashMap::new()),
                reports: RwLock::new(HashMap::new()),
                featured: RwLock::new(Vec::new()),
            }),
        }
    }

    /// Run quality gates against module metadata.
    pub fn evaluate(&self, module_id: &str, has_tests: bool, has_docs: bool, cve_count: u32, code_coverage: f64) -> QualityReport {
        let mut checks = Vec::new();
        let config = &self.inner.config;

        let test_score = if has_tests { 10.0 } else { 0.0 };
        checks.push(QualityCheck {
            name: "test_coverage".to_string(),
            passed: !config.require_tests || has_tests,
            score: test_score,
            details: if has_tests { "Tests present".to_string() } else { "No tests found".to_string() },
        });

        let doc_score = if has_docs { 10.0 } else { 0.0 };
        checks.push(QualityCheck {
            name: "documentation".to_string(),
            passed: !config.require_docs || has_docs,
            score: doc_score,
            details: if has_docs { "Documentation present".to_string() } else { "No documentation found".to_string() },
        });

        let cve_score = if cve_count == 0 { 10.0 } else { (10.0 - cve_count as f64 * 2.5).max(0.0) };
        checks.push(QualityCheck {
            name: "security_vulnerabilities".to_string(),
            passed: cve_count == 0,
            score: cve_score,
            details: format!("{} CVEs found", cve_count),
        });

        let coverage_score = code_coverage * 10.0;
        checks.push(QualityCheck {
            name: "code_coverage".to_string(),
            passed: code_coverage >= 0.5,
            score: coverage_score,
            details: format!("{:.0}% coverage", code_coverage * 100.0),
        });

        let overall = checks.iter().map(|c| c.score).sum::<f64>() / checks.len() as f64;

        let tier = if overall >= config.min_score_enterprise {
            CertificationTier::Enterprise
        } else if overall >= config.min_score_curated {
            CertificationTier::Curated
        } else if overall >= config.min_score_verified {
            CertificationTier::Verified
        } else {
            CertificationTier::Community
        };

        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let report = QualityReport {
            module_id: module_id.to_string(),
            checks,
            overall_score: overall,
            tier,
            generated_at: ts,
        };

        self.inner.reports.write().insert(module_id.to_string(), report.clone());
        self.inner.certifications.write().insert(module_id.to_string(), tier);

        report
    }

    /// Get current certification tier for a module.
    pub fn get_tier(&self, module_id: &str) -> Option<CertificationTier> {
        self.inner.certifications.read().get(module_id).copied()
    }

    /// Get the quality report for a module.
    pub fn get_report(&self, module_id: &str) -> Option<QualityReport> {
        self.inner.reports.read().get(module_id).cloned()
    }

    /// Add a featured listing.
    pub fn feature_module(&self, listing: FeaturedListing) {
        self.inner.featured.write().push(listing);
    }

    /// Get active featured listings sorted by priority.
    pub fn featured_listings(&self) -> Vec<FeaturedListing> {
        let mut listings = self.inner.featured.read().clone();
        listings.sort_by(|a, b| b.priority.cmp(&a.priority));
        listings
    }

    /// Remove expired featured listings.
    pub fn prune_expired(&self, now: u64) {
        self.inner.featured.write().retain(|l| {
            l.expires_at.map_or(true, |exp| exp > now)
        });
    }

    /// Promote a module to a higher tier manually (curator override).
    pub fn promote(&self, module_id: &str, tier: CertificationTier) -> bool {
        let current = self.inner.certifications.read().get(module_id).copied();
        match current {
            Some(c) if tier > c => {
                self.inner.certifications.write().insert(module_id.to_string(), tier);
                true
            }
            None => {
                self.inner.certifications.write().insert(module_id.to_string(), tier);
                true
            }
            _ => false,
        }
    }

    /// Count modules by certification tier.
    pub fn tier_counts(&self) -> HashMap<CertificationTier, usize> {
        let certs = self.inner.certifications.read();
        let mut counts = HashMap::new();
        for tier in certs.values() {
            *counts.entry(*tier).or_insert(0) += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_high_quality() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        let report = engine.evaluate("mod-a", true, true, 0, 0.9);
        assert!(report.overall_score >= 7.0);
        assert!(report.passed_all());
        assert!(report.tier >= CertificationTier::Curated);
    }

    #[test]
    fn test_evaluate_low_quality() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        let report = engine.evaluate("mod-b", false, false, 3, 0.1);
        assert!(report.overall_score < 5.0);
        assert!(!report.passed_all());
        assert_eq!(report.tier, CertificationTier::Community);
    }

    #[test]
    fn test_certification_tier_ordering() {
        assert!(CertificationTier::Enterprise > CertificationTier::Curated);
        assert!(CertificationTier::Curated > CertificationTier::Verified);
        assert!(CertificationTier::Verified > CertificationTier::Community);
    }

    #[test]
    fn test_promote_module() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        engine.evaluate("mod-x", false, false, 0, 0.3);
        assert_eq!(engine.get_tier("mod-x"), Some(CertificationTier::Community));
        assert!(engine.promote("mod-x", CertificationTier::Enterprise));
        assert_eq!(engine.get_tier("mod-x"), Some(CertificationTier::Enterprise));
        // Cannot demote
        assert!(!engine.promote("mod-x", CertificationTier::Verified));
    }

    #[test]
    fn test_featured_listings() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        engine.feature_module(FeaturedListing {
            module_id: "a".into(), title: "A".into(), description: "".into(),
            category: "utils".into(), featured_at: 100, expires_at: Some(200), priority: 1,
        });
        engine.feature_module(FeaturedListing {
            module_id: "b".into(), title: "B".into(), description: "".into(),
            category: "utils".into(), featured_at: 100, expires_at: None, priority: 10,
        });
        let listings = engine.featured_listings();
        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].module_id, "b"); // higher priority first
    }

    #[test]
    fn test_prune_expired() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        engine.feature_module(FeaturedListing {
            module_id: "old".into(), title: "Old".into(), description: "".into(),
            category: "".into(), featured_at: 100, expires_at: Some(150), priority: 1,
        });
        engine.feature_module(FeaturedListing {
            module_id: "new".into(), title: "New".into(), description: "".into(),
            category: "".into(), featured_at: 100, expires_at: Some(300), priority: 1,
        });
        engine.prune_expired(200);
        assert_eq!(engine.featured_listings().len(), 1);
        assert_eq!(engine.featured_listings()[0].module_id, "new");
    }

    #[test]
    fn test_tier_counts() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        engine.evaluate("a", true, true, 0, 0.9);
        engine.evaluate("b", false, false, 3, 0.1);
        engine.evaluate("c", true, true, 0, 0.8);
        let counts = engine.tier_counts();
        assert!(counts.values().sum::<usize>() == 3);
    }

    #[test]
    fn test_get_report() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        assert!(engine.get_report("nonexistent").is_none());
        engine.evaluate("mod-z", true, true, 0, 0.7);
        let report = engine.get_report("mod-z").unwrap();
        assert_eq!(report.module_id, "mod-z");
        assert_eq!(report.checks.len(), 4);
    }

    #[test]
    fn test_failed_checks() {
        let engine = CurationEngine::new(QualityGateConfig::default());
        let report = engine.evaluate("mod-fail", false, true, 2, 0.3);
        let failed = report.failed_checks();
        assert!(!failed.is_empty());
        assert!(failed.iter().any(|c| c.name == "test_coverage"));
    }
}
