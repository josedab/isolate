//! Supply chain security: provenance tracking and vulnerability scanning.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::helpers::{read_leb128_u32, sha256_hex};

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// A reference to a dependency used during the build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRef {
    /// Name of the dependency.
    pub name: String,
    /// Version string.
    pub version: String,
    /// SHA-256 hash of the dependency artifact.
    pub hash: String,
}

/// Provenance metadata tracking origin and build history of a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    /// SHA-256 of the module bytes.
    pub module_hash: String,
    /// Who (or what CI system) built the module.
    pub builder: String,
    /// Git repository URL.
    pub source_repo: String,
    /// When the module was built.
    pub build_timestamp: DateTime<Utc>,
    /// Command used to build the module.
    pub build_command: String,
    /// Dependencies consumed during the build.
    pub dependencies: Vec<DependencyRef>,
    /// Whether the build is reproducible.
    pub reproducible: bool,
}

impl ProvenanceRecord {
    /// Produce a SLSA v0.2-style provenance predicate as JSON.
    pub fn to_slsa_predicate(&self) -> serde_json::Value {
        let materials: Vec<serde_json::Value> = self
            .dependencies
            .iter()
            .map(|d| {
                serde_json::json!({
                    "uri": format!("pkg:{}/{}", d.name, d.version),
                    "digest": { "sha256": d.hash },
                })
            })
            .collect();

        serde_json::json!({
            "buildType": "https://isolate.dev/build/v1",
            "builder": { "id": self.builder },
            "invocation": {
                "configSource": {
                    "uri": self.source_repo,
                },
                "parameters": {
                    "command": self.build_command,
                },
            },
            "materials": materials,
            "metadata": {
                "buildStartedOn": self.build_timestamp.to_rfc3339(),
                "reproducible": self.reproducible,
            },
        })
    }
}

/// In-memory store for provenance records.
pub struct ProvenanceStore {
    records: HashMap<String, ProvenanceRecord>,
}

/// Summary produced by [`ProvenanceStore::verify_supply_chain`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyChainReport {
    /// The module hash being inspected.
    pub module_hash: String,
    /// Whether a provenance record exists.
    pub has_provenance: bool,
    /// Whether the build is marked reproducible.
    pub reproducible: bool,
    /// Number of dependencies.
    pub dependency_count: usize,
    /// Whether all dependency hashes are non-empty.
    pub all_deps_verified: bool,
}

impl ProvenanceStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self { records: HashMap::new() }
    }

    /// Record provenance for a module.
    pub fn record(&mut self, provenance: ProvenanceRecord) -> Result<(), String> {
        if provenance.module_hash.is_empty() {
            return Err("module_hash must not be empty".into());
        }
        self.records.insert(provenance.module_hash.clone(), provenance);
        Ok(())
    }

    /// Retrieve provenance by module hash.
    pub fn get(&self, module_hash: &str) -> Option<&ProvenanceRecord> {
        self.records.get(module_hash)
    }

    /// Produce a supply-chain health report for a module.
    pub fn verify_supply_chain(&self, module_hash: &str) -> SupplyChainReport {
        match self.records.get(module_hash) {
            Some(record) => {
                let all_deps_verified = !record.dependencies.is_empty()
                    && record.dependencies.iter().all(|d| !d.hash.is_empty());
                SupplyChainReport {
                    module_hash: module_hash.to_string(),
                    has_provenance: true,
                    reproducible: record.reproducible,
                    dependency_count: record.dependencies.len(),
                    all_deps_verified,
                }
            }
            None => SupplyChainReport {
                module_hash: module_hash.to_string(),
                has_provenance: false,
                reproducible: false,
                dependency_count: 0,
                all_deps_verified: false,
            },
        }
    }
}

impl Default for ProvenanceStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Vulnerability scanning
// ---------------------------------------------------------------------------

/// Severity of a vulnerability finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Critical impact.
    Critical,
    /// High impact.
    High,
    /// Medium impact.
    Medium,
    /// Low impact.
    Low,
    /// Informational only.
    Info,
}

/// Overall risk level for a scanned module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// At least one critical finding.
    Critical,
    /// At least one high finding.
    High,
    /// At least one medium finding.
    Medium,
    /// Only low findings.
    Low,
    /// No findings.
    None,
}

/// A single vulnerability finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityFinding {
    /// CVE identifier, if applicable.
    pub cve_id: Option<String>,
    /// Short title.
    pub title: String,
    /// Longer description.
    pub description: String,
    /// Severity level.
    pub severity: Severity,
    /// Component or import that triggered the finding.
    pub affected_component: String,
    /// Suggested remediation.
    pub remediation: Option<String>,
}

/// Result of scanning a WASM module for vulnerabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityReport {
    /// SHA-256 hash of the scanned module.
    pub module_hash: String,
    /// When the scan was performed.
    pub scanned_at: DateTime<Utc>,
    /// Individual findings.
    pub findings: Vec<VulnerabilityFinding>,
    /// Counts per severity level.
    pub severity_counts: HashMap<Severity, usize>,
    /// Overall risk level.
    pub overall_risk: RiskLevel,
}

/// A pattern-based rule used by the scanner.
#[derive(Debug, Clone)]
pub struct ScanRule {
    /// Unique rule identifier.
    pub id: String,
    /// Pattern to search for in the module (substring match on UTF-8 view).
    pub pattern: String,
    /// Severity assigned when the pattern is found.
    pub severity: Severity,
    /// Human-readable title for findings produced by this rule.
    pub title: String,
    /// Description included in findings.
    pub description: String,
    /// Optional remediation advice.
    pub remediation: Option<String>,
}

/// Scans WASM modules for known-bad patterns.
pub struct VulnerabilityScanner {
    rules: Vec<ScanRule>,
}

impl VulnerabilityScanner {
    /// Create a scanner pre-loaded with built-in rules.
    pub fn new() -> Self {
        Self { rules: Self::builtin_rules() }
    }

    /// Add a custom scan rule.
    pub fn add_rule(&mut self, rule: ScanRule) {
        self.rules.push(rule);
    }

    /// Scan `module_bytes` and return a vulnerability report.
    pub fn scan(&self, module_bytes: &[u8]) -> VulnerabilityReport {
        let module_hash = sha256_hex(module_bytes);
        let text = String::from_utf8_lossy(module_bytes);
        let mut findings = Vec::new();

        for rule in &self.rules {
            if text.contains(&rule.pattern) {
                findings.push(VulnerabilityFinding {
                    cve_id: None,
                    title: rule.title.clone(),
                    description: rule.description.clone(),
                    severity: rule.severity,
                    affected_component: rule.pattern.clone(),
                    remediation: rule.remediation.clone(),
                });
            }
        }

        // Check for excessive memory requests (> 256 pages = 16 MiB).
        if Self::has_large_memory(module_bytes) {
            findings.push(VulnerabilityFinding {
                cve_id: None,
                title: "Excessive initial memory".into(),
                description: "Module requests more than 256 pages (16 MiB) of initial memory"
                    .into(),
                severity: Severity::Medium,
                affected_component: "memory".into(),
                remediation: Some("Reduce initial memory or use memory.grow dynamically".into()),
            });
        }

        let mut severity_counts: HashMap<Severity, usize> = HashMap::new();
        for f in &findings {
            *severity_counts.entry(f.severity).or_insert(0) += 1;
        }

        let overall_risk = if severity_counts.contains_key(&Severity::Critical) {
            RiskLevel::Critical
        } else if severity_counts.contains_key(&Severity::High) {
            RiskLevel::High
        } else if severity_counts.contains_key(&Severity::Medium) {
            RiskLevel::Medium
        } else if severity_counts.contains_key(&Severity::Low) {
            RiskLevel::Low
        } else {
            RiskLevel::None
        };

        VulnerabilityReport {
            module_hash,
            scanned_at: Utc::now(),
            findings,
            severity_counts,
            overall_risk,
        }
    }

    // --- built-in rules ---------------------------------------------------

    fn builtin_rules() -> Vec<ScanRule> {
        vec![
            ScanRule {
                id: "WASM-001".into(),
                pattern: "proc_exit".into(),
                severity: Severity::Low,
                title: "Uses proc_exit".into(),
                description: "Module imports proc_exit which can terminate the sandbox".into(),
                remediation: Some("Ensure host traps proc_exit appropriately".into()),
            },
            ScanRule {
                id: "WASM-002".into(),
                pattern: "fd_write".into(),
                severity: Severity::Info,
                title: "Uses fd_write".into(),
                description: "Module imports fd_write for I/O".into(),
                remediation: None,
            },
            ScanRule {
                id: "WASM-003".into(),
                pattern: "sock_accept".into(),
                severity: Severity::High,
                title: "Socket accept import".into(),
                description: "Module imports sock_accept which may open network listeners".into(),
                remediation: Some("Remove network capability or audit usage".into()),
            },
            ScanRule {
                id: "WASM-004".into(),
                pattern: "__wasm_call_ctors".into(),
                severity: Severity::Info,
                title: "Static constructors detected".into(),
                description: "Module contains static constructor calls".into(),
                remediation: None,
            },
            ScanRule {
                id: "WASM-005".into(),
                pattern: "eval".into(),
                severity: Severity::Medium,
                title: "Suspicious function name: eval".into(),
                description: "Module contains a reference to 'eval' which may indicate dynamic code execution".into(),
                remediation: Some("Audit eval usage or sandbox further".into()),
            },
        ]
    }

    /// Heuristic: check if a WASM module requests > 256 initial pages.
    ///
    /// The memory section (id 5) contains a limits entry.  We do a minimal
    /// parse rather than pulling in a full WASM parser.
    pub(crate) fn has_large_memory(bytes: &[u8]) -> bool {
        // Minimal WASM header: 8 bytes
        if bytes.len() < 8 {
            return false;
        }
        let mut pos = 8;
        while pos < bytes.len() {
            if pos + 1 >= bytes.len() {
                break;
            }
            let section_id = bytes[pos];
            pos += 1;
            let (section_len, adv) = read_leb128_u32(bytes, pos);
            pos += adv;
            if section_id == 5 {
                // Memory section – first byte is count
                if pos < bytes.len() {
                    let mem_start = pos + 1; // skip count
                    if mem_start < bytes.len() {
                        let flags = bytes[mem_start];
                        let (initial, _) = read_leb128_u32(bytes, mem_start + 1);
                        let _ = flags; // limits flags (has-max)
                        return initial > 256;
                    }
                }
                return false;
            }
            pos += section_len as usize;
        }
        false
    }
}

impl Default for VulnerabilityScanner {
    fn default() -> Self {
        Self::new()
    }
}
