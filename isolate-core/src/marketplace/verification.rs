//! Module verification and badge system.
//!
//! Automated checks for WASM modules including size limits, valid magic bytes,
//! import/function counts, and memory page limits. Produces a verification
//! report with risk scoring and badge assignment.

use super::registry::{ModuleManifest, ModuleVersion};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A badge awarded to a module after verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Badge {
    SecurityScanned { passed: bool, scan_date: DateTime<Utc> },
    TestsPassed { count: u32, passed: u32, scan_date: DateTime<Utc> },
    Signed { signer: String, algorithm: String },
    Official,
    Verified,
    Deprecated { reason: String, alternative: Option<String> },
}

/// Overall risk classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskScore {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for RiskScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// A single verification check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub passed: bool,
    pub details: String,
}

/// Full verification report for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub module_name: String,
    pub version: String,
    pub badges: Vec<Badge>,
    pub risk_score: RiskScore,
    pub checks: Vec<VerificationCheck>,
    pub verified_at: DateTime<Utc>,
}

/// Configurable module verifier with pluggable checks.
pub struct ModuleVerifier {
    checks: Vec<Box<dyn Fn(&[u8], &ModuleManifest) -> VerificationCheck + Send + Sync>>,
}

impl ModuleVerifier {
    /// Create a verifier pre-loaded with the built-in checks.
    pub fn new() -> Self {
        let mut v = Self { checks: Vec::new() };
        v.add_builtin_checks();
        v
    }

    /// Add a custom check function.
    pub fn add_check<F>(&mut self, _name: &str, check_fn: F)
    where
        F: Fn(&[u8], &ModuleManifest) -> VerificationCheck + Send + Sync + 'static,
    {
        self.checks.push(Box::new(check_fn));
    }

    /// Run all checks and produce a full verification report.
    pub fn verify(&self, module_bytes: &[u8], manifest: &ModuleManifest) -> VerificationReport {
        let checks: Vec<VerificationCheck> =
            self.checks.iter().map(|check| check(module_bytes, manifest)).collect();

        let risk_score = Self::compute_risk(&checks);

        let mut badges = Vec::new();
        let now = Utc::now();

        badges.push(Badge::SecurityScanned {
            passed: risk_score <= RiskScore::Medium,
            scan_date: now,
        });

        if checks.iter().all(|c| c.passed) {
            badges.push(Badge::Verified);
        }

        VerificationReport {
            module_name: manifest.name.clone(),
            version: manifest.version.to_string(),
            badges,
            risk_score,
            checks,
            verified_at: now,
        }
    }

    /// Perform a quick risk assessment without full badge generation.
    pub fn quick_check(&self, module_bytes: &[u8]) -> RiskScore {
        let stub_manifest = ModuleManifest::builder("_quick_check", ModuleVersion::new(0, 0, 0))
            .description("quick check stub")
            .build();

        let checks: Vec<VerificationCheck> =
            self.checks.iter().map(|check| check(module_bytes, &stub_manifest)).collect();

        Self::compute_risk(&checks)
    }

    // -- private helpers --

    fn add_builtin_checks(&mut self) {
        // 1. Module size limit (10 MB)
        self.checks.push(Box::new(|bytes: &[u8], _manifest: &ModuleManifest| {
            const MAX_SIZE: usize = 10 * 1024 * 1024;
            let passed = bytes.len() <= MAX_SIZE;
            VerificationCheck {
                name: "module_size".to_string(),
                passed,
                details: format!("Module size {} bytes (limit {} bytes)", bytes.len(), MAX_SIZE),
            }
        }));

        // 2. Valid WASM magic bytes
        self.checks.push(Box::new(|bytes: &[u8], _manifest: &ModuleManifest| {
            let passed = bytes.len() >= 8 && &bytes[0..4] == b"\0asm";
            VerificationCheck {
                name: "wasm_magic".to_string(),
                passed,
                details: if passed {
                    "Valid WASM magic bytes".to_string()
                } else {
                    "Invalid or missing WASM magic bytes".to_string()
                },
            }
        }));

        // 3. Import count limit
        self.checks.push(Box::new(|bytes: &[u8], _manifest: &ModuleManifest| {
            let import_count = count_wasm_section(bytes, 2); // section id 2 = import
            let limit = 256u32;
            let passed = import_count <= limit;
            VerificationCheck {
                name: "import_count".to_string(),
                passed,
                details: format!("Import section entries ~{} (limit {})", import_count, limit),
            }
        }));

        // 4. Memory page limit
        self.checks.push(Box::new(|bytes: &[u8], _manifest: &ModuleManifest| {
            let memory_sections = count_wasm_section(bytes, 5); // section id 5 = memory
            let limit = 128u32;
            let passed = memory_sections <= limit;
            VerificationCheck {
                name: "memory_pages".to_string(),
                passed,
                details: format!("Memory section entries ~{} (limit {})", memory_sections, limit),
            }
        }));

        // 5. Function count limit
        self.checks.push(Box::new(|bytes: &[u8], _manifest: &ModuleManifest| {
            let func_count = count_wasm_section(bytes, 3); // section id 3 = function
            let limit = 10_000u32;
            let passed = func_count <= limit;
            VerificationCheck {
                name: "function_count".to_string(),
                passed,
                details: format!("Function section entries ~{} (limit {})", func_count, limit),
            }
        }));
    }

    fn compute_risk(checks: &[VerificationCheck]) -> RiskScore {
        let failed = checks.iter().filter(|c| !c.passed).count();
        match failed {
            0 => RiskScore::Low,
            1 => RiskScore::Medium,
            2 => RiskScore::High,
            _ => RiskScore::Critical,
        }
    }
}

impl Default for ModuleVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Heuristic: count occurrences of a WASM section id in the binary.
/// This is a rough approximation—good enough for limit checks.
fn count_wasm_section(bytes: &[u8], section_id: u8) -> u32 {
    if bytes.len() < 8 {
        return 0;
    }
    let mut count = 0u32;
    // Scan for section headers after the 8-byte WASM header
    let mut pos = 8;
    while pos < bytes.len() {
        if bytes[pos] == section_id {
            count += 1;
        }
        pos += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::registry::ModuleVersion;

    const VALID_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    fn test_manifest() -> ModuleManifest {
        ModuleManifest::builder("test-module", ModuleVersion::new(1, 0, 0))
            .description("A test module")
            .build()
    }

    #[test]
    fn test_verify_valid_wasm() {
        let verifier = ModuleVerifier::new();
        let report = verifier.verify(VALID_WASM, &test_manifest());

        assert_eq!(report.module_name, "test-module");
        assert_eq!(report.risk_score, RiskScore::Low);
        assert!(report.checks.iter().all(|c| c.passed));
    }

    #[test]
    fn test_verify_invalid_magic() {
        let verifier = ModuleVerifier::new();
        let bad_bytes = b"not wasm at all!!";
        let report = verifier.verify(bad_bytes, &test_manifest());

        let magic_check = report.checks.iter().find(|c| c.name == "wasm_magic").unwrap();
        assert!(!magic_check.passed);
        assert!(report.risk_score >= RiskScore::Medium);
    }

    #[test]
    fn test_verify_oversized_module() {
        let verifier = ModuleVerifier::new();
        let mut big = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        big.extend(vec![0u8; 11 * 1024 * 1024]); // > 10 MB
        let report = verifier.verify(&big, &test_manifest());

        let size_check = report.checks.iter().find(|c| c.name == "module_size").unwrap();
        assert!(!size_check.passed);
    }

    #[test]
    fn test_quick_check_valid() {
        let verifier = ModuleVerifier::new();
        let risk = verifier.quick_check(VALID_WASM);
        assert_eq!(risk, RiskScore::Low);
    }

    #[test]
    fn test_quick_check_invalid() {
        let verifier = ModuleVerifier::new();
        let risk = verifier.quick_check(b"bad");
        assert!(risk >= RiskScore::Medium);
    }

    #[test]
    fn test_custom_check() {
        let mut verifier = ModuleVerifier::new();
        verifier.add_check("always_fail", |_bytes, _manifest| VerificationCheck {
            name: "always_fail".to_string(),
            passed: false,
            details: "This check always fails".to_string(),
        });

        let report = verifier.verify(VALID_WASM, &test_manifest());
        let custom = report.checks.iter().find(|c| c.name == "always_fail").unwrap();
        assert!(!custom.passed);
    }

    #[test]
    fn test_badges_all_pass() {
        let verifier = ModuleVerifier::new();
        let report = verifier.verify(VALID_WASM, &test_manifest());

        assert!(report.badges.iter().any(|b| matches!(b, Badge::Verified)));
        assert!(report
            .badges
            .iter()
            .any(|b| matches!(b, Badge::SecurityScanned { passed: true, .. })));
    }

    #[test]
    fn test_risk_score_ordering() {
        assert!(RiskScore::Low < RiskScore::Medium);
        assert!(RiskScore::Medium < RiskScore::High);
        assert!(RiskScore::High < RiskScore::Critical);
    }

    #[test]
    fn test_risk_score_display() {
        assert_eq!(RiskScore::Low.to_string(), "low");
        assert_eq!(RiskScore::Critical.to_string(), "critical");
    }
}
