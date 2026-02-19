//! Module security scanning and trust verification for the marketplace.
//!
//! Scans WASM modules for known vulnerabilities, suspicious patterns,
//! and verifies publisher trust levels.



use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Result of scanning a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Module identifier.
    pub module_id: String,
    /// Module hash.
    pub module_hash: String,
    /// Overall risk level.
    pub risk_level: RiskLevel,
    /// Individual findings.
    pub findings: Vec<ScanFinding>,
    /// Scan timestamp.
    pub scanned_at: SystemTime,
    /// Scan duration.
    pub scan_duration: Duration,
    /// Scanner version.
    pub scanner_version: String,
}

impl ScanResult {
    /// Check if the module passed the scan (no critical/high findings).
    pub fn passed(&self) -> bool {
        !self.findings.iter().any(|f| {
            matches!(f.severity, FindingSeverity::Critical | FindingSeverity::High)
        })
    }

    /// Count findings by severity.
    pub fn finding_counts(&self) -> HashMap<FindingSeverity, usize> {
        let mut counts = HashMap::new();
        for finding in &self.findings {
            *counts.entry(finding.severity).or_default() += 1;
        }
        counts
    }
}

/// Overall risk level assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

/// A finding from the security scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFinding {
    /// Finding identifier.
    pub id: String,
    /// Severity level.
    pub severity: FindingSeverity,
    /// Category of finding.
    pub category: FindingCategory,
    /// Human-readable title.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Remediation advice.
    pub remediation: Option<String>,
    /// Location in the module (byte offset or function).
    pub location: Option<String>,
}

/// Severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Category of security finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FindingCategory {
    /// Memory safety concern.
    MemorySafety,
    /// Suspicious import pattern.
    SuspiciousImport,
    /// Excessive resource usage pattern.
    ResourceAbuse,
    /// Known vulnerability signature.
    KnownVulnerability,
    /// Untrusted or missing signature.
    TrustIssue,
    /// License compliance.
    LicenseIssue,
    /// Dependency vulnerability.
    DependencyVuln,
    /// Code quality concern.
    Quality,
}

/// Module security scanner.
pub struct ModuleScanner {
    /// Scan rules.
    rules: Vec<ScanRule>,
    /// Known vulnerability signatures.
    vuln_signatures: HashMap<String, VulnSignature>,
}

/// A scanning rule.
#[derive(Debug, Clone)]
pub struct ScanRule {
    pub id: String,
    pub name: String,
    pub category: FindingCategory,
    pub severity: FindingSeverity,
    pub check: ScanCheck,
}

/// Type of check to perform.
#[derive(Debug, Clone)]
pub enum ScanCheck {
    /// Check for specific byte patterns.
    BytePattern(Vec<u8>),
    /// Check for suspicious import names.
    ImportPattern(String),
    /// Check module size limits.
    SizeLimit(usize),
    /// Check for known function signatures.
    FunctionSignature(String),
    /// Check for missing features.
    RequiredFeature(String),
}

/// A known vulnerability signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnSignature {
    pub cve_id: String,
    pub description: String,
    pub severity: FindingSeverity,
    pub pattern: Vec<u8>,
}

impl ModuleScanner {
    /// Create a new scanner with default rules.
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
            vuln_signatures: HashMap::new(),
        }
    }

    fn default_rules() -> Vec<ScanRule> {
        vec![
            ScanRule {
                id: "SIZE-001".to_string(),
                name: "Module size check".to_string(),
                category: FindingCategory::Quality,
                severity: FindingSeverity::Info,
                check: ScanCheck::SizeLimit(50 * 1024 * 1024), // 50MB
            },
            ScanRule {
                id: "IMP-001".to_string(),
                name: "Suspicious network import".to_string(),
                category: FindingCategory::SuspiciousImport,
                severity: FindingSeverity::Medium,
                check: ScanCheck::ImportPattern("sock_".to_string()),
            },
            ScanRule {
                id: "IMP-002".to_string(),
                name: "Process spawn import".to_string(),
                category: FindingCategory::SuspiciousImport,
                severity: FindingSeverity::High,
                check: ScanCheck::ImportPattern("proc_exec".to_string()),
            },
        ]
    }

    /// Scan a WASM module.
    pub fn scan(&self, module_id: &str, module_bytes: &[u8]) -> ScanResult {
        let start = std::time::Instant::now();
        let mut findings = Vec::new();

        let module_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(module_bytes);
            hex::encode(hasher.finalize())
        };

        // Run all rules
        for rule in &self.rules {
            if let Some(finding) = self.run_rule(rule, module_bytes) {
                findings.push(finding);
            }
        }

        // Check vulnerability signatures
        for (_, sig) in &self.vuln_signatures {
            if module_bytes.windows(sig.pattern.len()).any(|w| w == sig.pattern) {
                findings.push(ScanFinding {
                    id: sig.cve_id.clone(),
                    severity: sig.severity,
                    category: FindingCategory::KnownVulnerability,
                    title: format!("Known vulnerability: {}", sig.cve_id),
                    description: sig.description.clone(),
                    remediation: Some("Update to a patched version of the module".to_string()),
                    location: None,
                });
            }
        }

        // Determine risk level
        let risk_level = if findings.iter().any(|f| f.severity == FindingSeverity::Critical) {
            RiskLevel::Critical
        } else if findings.iter().any(|f| f.severity == FindingSeverity::High) {
            RiskLevel::High
        } else if findings.iter().any(|f| f.severity == FindingSeverity::Medium) {
            RiskLevel::Medium
        } else if findings.iter().any(|f| f.severity == FindingSeverity::Low) {
            RiskLevel::Low
        } else {
            RiskLevel::None
        };

        ScanResult {
            module_id: module_id.to_string(),
            module_hash,
            risk_level,
            findings,
            scanned_at: SystemTime::now(),
            scan_duration: start.elapsed(),
            scanner_version: "1.0.0".to_string(),
        }
    }

    fn run_rule(&self, rule: &ScanRule, module_bytes: &[u8]) -> Option<ScanFinding> {
        let triggered = match &rule.check {
            ScanCheck::SizeLimit(max_size) => module_bytes.len() > *max_size,
            ScanCheck::BytePattern(pattern) => {
                module_bytes.windows(pattern.len()).any(|w| w == pattern.as_slice())
            }
            ScanCheck::ImportPattern(pattern) => {
                // Simple string search in module bytes
                let pattern_bytes = pattern.as_bytes();
                module_bytes
                    .windows(pattern_bytes.len())
                    .any(|w| w == pattern_bytes)
            }
            ScanCheck::FunctionSignature(sig) => {
                let sig_bytes = sig.as_bytes();
                module_bytes.windows(sig_bytes.len()).any(|w| w == sig_bytes)
            }
            ScanCheck::RequiredFeature(_) => false,
        };

        if triggered {
            Some(ScanFinding {
                id: rule.id.clone(),
                severity: rule.severity,
                category: rule.category,
                title: rule.name.clone(),
                description: format!("Rule {} triggered", rule.id),
                remediation: None,
                location: None,
            })
        } else {
            None
        }
    }

    /// Add a custom scan rule.
    pub fn add_rule(&mut self, rule: ScanRule) {
        self.rules.push(rule);
    }

    /// Add a vulnerability signature.
    pub fn add_vulnerability(&mut self, sig: VulnSignature) {
        self.vuln_signatures.insert(sig.cve_id.clone(), sig);
    }

    /// Number of rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl Default for ModuleScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal WASM module
    const MINIMAL_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_scan_clean_module() {
        let scanner = ModuleScanner::new();
        let result = scanner.scan("test-module", MINIMAL_WASM);

        assert_eq!(result.risk_level, RiskLevel::None);
        assert!(result.passed());
        assert!(result.findings.is_empty());
    }

    #[test]
    fn test_scan_suspicious_import() {
        let scanner = ModuleScanner::new();

        // Module containing suspicious import pattern
        let mut module = MINIMAL_WASM.to_vec();
        module.extend_from_slice(b"sock_connect");

        let result = scanner.scan("suspicious", &module);
        assert!(!result.findings.is_empty());
        assert!(result.findings.iter().any(|f| f.category == FindingCategory::SuspiciousImport));
    }

    #[test]
    fn test_scan_large_module() {
        let scanner = ModuleScanner::new();

        // Module larger than 50MB
        let module = vec![0u8; 51 * 1024 * 1024];
        let result = scanner.scan("large", &module);

        assert!(result.findings.iter().any(|f| f.id == "SIZE-001"));
    }

    #[test]
    fn test_scan_known_vulnerability() {
        let mut scanner = ModuleScanner::new();
        scanner.add_vulnerability(VulnSignature {
            cve_id: "CVE-2024-0001".to_string(),
            description: "Test vulnerability".to_string(),
            severity: FindingSeverity::Critical,
            pattern: b"VULN_PATTERN".to_vec(),
        });

        let mut module = MINIMAL_WASM.to_vec();
        module.extend_from_slice(b"VULN_PATTERN");

        let result = scanner.scan("vuln-module", &module);
        assert_eq!(result.risk_level, RiskLevel::Critical);
        assert!(!result.passed());
    }

    #[test]
    fn test_scan_result_counts() {
        let result = ScanResult {
            module_id: "test".to_string(),
            module_hash: "abc".to_string(),
            risk_level: RiskLevel::Medium,
            findings: vec![
                ScanFinding {
                    id: "1".to_string(),
                    severity: FindingSeverity::Low,
                    category: FindingCategory::Quality,
                    title: "t1".to_string(),
                    description: "d1".to_string(),
                    remediation: None,
                    location: None,
                },
                ScanFinding {
                    id: "2".to_string(),
                    severity: FindingSeverity::Medium,
                    category: FindingCategory::Quality,
                    title: "t2".to_string(),
                    description: "d2".to_string(),
                    remediation: None,
                    location: None,
                },
                ScanFinding {
                    id: "3".to_string(),
                    severity: FindingSeverity::Low,
                    category: FindingCategory::Quality,
                    title: "t3".to_string(),
                    description: "d3".to_string(),
                    remediation: None,
                    location: None,
                },
            ],
            scanned_at: SystemTime::now(),
            scan_duration: Duration::from_millis(10),
            scanner_version: "1.0.0".to_string(),
        };

        let counts = result.finding_counts();
        assert_eq!(counts[&FindingSeverity::Low], 2);
        assert_eq!(counts[&FindingSeverity::Medium], 1);
        assert!(result.passed());
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Critical > RiskLevel::High);
        assert!(RiskLevel::High > RiskLevel::Medium);
        assert!(RiskLevel::Medium > RiskLevel::Low);
        assert!(RiskLevel::Low > RiskLevel::None);
    }
}
