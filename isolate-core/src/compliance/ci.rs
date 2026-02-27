//! CI/CD integration for continuous compliance verification.
//!
//! Provides machine-readable compliance check output suitable for
//! CI/CD pipelines, with exit codes, JSON output, and drift detection.

use super::audit_trail::AuditTrail;
use super::evidence::EvidenceCollector;
use super::frameworks::FrameworkTemplate;
use super::reports::{ComplianceReport, ReportGenerator};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// CI compliance check result with exit code semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiCheckResult {
    /// Whether all checks passed.
    pub passed: bool,
    /// Suggested exit code (0 = pass, 1 = fail, 2 = warnings only).
    pub exit_code: i32,
    /// Summary message.
    pub summary: String,
    /// Per-framework results.
    pub framework_results: Vec<FrameworkCheckResult>,
    /// Drift detected since last check.
    pub drift_detected: bool,
    /// Timestamp of the check.
    pub timestamp: u64,
}

/// Result for a single framework check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkCheckResult {
    /// Framework ID.
    pub framework_id: String,
    /// Framework name.
    pub framework_name: String,
    /// Overall score (0.0 - 1.0).
    pub score: f64,
    /// Whether this framework passes.
    pub passing: bool,
    /// Number of controls that pass.
    pub controls_passed: usize,
    /// Total number of controls.
    pub controls_total: usize,
    /// Critical failures.
    pub critical_failures: Vec<String>,
    /// Controls that need attention.
    pub needs_attention: Vec<String>,
}

/// Configuration for CI compliance checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiConfig {
    /// Frameworks to check.
    pub frameworks: Vec<String>,
    /// Minimum passing score (0.0 - 1.0).
    pub min_score: f64,
    /// Fail on any critical control failure.
    pub fail_on_critical: bool,
    /// Fail if untested controls exceed this count.
    pub max_untested: usize,
    /// Enable drift detection from previous run.
    pub detect_drift: bool,
    /// Previous score for drift detection.
    pub previous_score: Option<f64>,
}

impl Default for CiConfig {
    fn default() -> Self {
        Self {
            frameworks: vec!["soc2".to_string()],
            min_score: 0.80,
            fail_on_critical: true,
            max_untested: 5,
            detect_drift: true,
            previous_score: None,
        }
    }
}

/// Run CI compliance checks against configured frameworks.
pub fn run_ci_check(
    config: &CiConfig,
    trail: &AuditTrail,
    evidence: &EvidenceCollector,
) -> CiCheckResult {
    let generator = ReportGenerator::new();
    let mut framework_results = Vec::new();
    let mut all_passing = true;
    let mut has_critical_failure = false;

    for framework_name in &config.frameworks {
        let template = match framework_name.as_str() {
            "soc2" => FrameworkTemplate::soc2(),
            "hipaa" => FrameworkTemplate::hipaa(),
            "pci-dss" | "pci_dss" => FrameworkTemplate::pci_dss(),
            "gdpr" => FrameworkTemplate::gdpr(),
            _ => continue,
        };

        let report = generator.generate(&template, trail, evidence);
        let result = check_framework(&report, config);

        if !result.passing {
            all_passing = false;
        }
        if !result.critical_failures.is_empty() {
            has_critical_failure = true;
        }

        framework_results.push(result);
    }

    // Drift detection
    let drift_detected = if config.detect_drift {
        if let Some(prev_score) = config.previous_score {
            let current_avg = if framework_results.is_empty() {
                0.0
            } else {
                framework_results.iter().map(|r| r.score).sum::<f64>()
                    / framework_results.len() as f64
            };
            current_avg < prev_score - 0.05 // >5% drop = drift
        } else {
            false
        }
    } else {
        false
    };

    let exit_code = if !all_passing || (config.fail_on_critical && has_critical_failure) {
        1
    } else if drift_detected {
        2
    } else {
        0
    };

    let summary = if exit_code == 0 {
        format!("All {} framework(s) passing", framework_results.len())
    } else if drift_detected {
        "Compliance drift detected".to_string()
    } else {
        let failed: Vec<_> = framework_results
            .iter()
            .filter(|r| !r.passing)
            .map(|r| r.framework_name.clone())
            .collect();
        format!("Failed: {}", failed.join(", "))
    };

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    CiCheckResult {
        passed: exit_code == 0,
        exit_code,
        summary,
        framework_results,
        drift_detected,
        timestamp,
    }
}

fn check_framework(report: &ComplianceReport, config: &CiConfig) -> FrameworkCheckResult {
    let critical_failures: Vec<String> = report
        .critical_failures()
        .iter()
        .map(|c| format!("{}: {}", c.control_id, c.control_name))
        .collect();

    let needs_attention: Vec<String> = report
        .untested_controls()
        .iter()
        .take(10)
        .map(|c| format!("{}: {}", c.control_id, c.control_name))
        .collect();

    let passing = report.overall_score >= config.min_score
        && report.not_tested <= config.max_untested
        && (!config.fail_on_critical || critical_failures.is_empty());

    FrameworkCheckResult {
        framework_id: report.framework_id.as_str().to_string(),
        framework_name: report.framework_name.clone(),
        score: report.overall_score,
        passing,
        controls_passed: report.passed_controls,
        controls_total: report.total_controls,
        critical_failures,
        needs_attention,
    }
}

/// Generate a JSON report suitable for CI artifact storage.
pub fn to_ci_json(result: &CiCheckResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance::evidence::{Evidence, EvidenceType};

    #[test]
    fn test_ci_check_no_evidence() {
        let config = CiConfig::default();
        let trail = AuditTrail::new();
        let evidence = EvidenceCollector::new();

        let result = run_ci_check(&config, &trail, &evidence);
        assert!(!result.passed);
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn test_ci_check_full_evidence() {
        let config = CiConfig {
            frameworks: vec!["soc2".to_string()],
            max_untested: 100, // Allow untested for this test
            fail_on_critical: false,
            ..Default::default()
        };
        let trail = AuditTrail::new();
        let evidence = EvidenceCollector::new();

        // Add evidence for all SOC2 controls
        let template = FrameworkTemplate::soc2();
        for control in &template.controls {
            evidence.add(Evidence {
                id: format!("ev-{}", control.id),
                control_id: control.id.clone(),
                evidence_type: EvidenceType::TestResult,
                description: "Automated test passed".into(),
                collected_at: 1000,
            });
            trail.record_at(&control.category, &format!("Verified {}", control.id), "ci-bot", 1000);
        }

        let result = run_ci_check(&config, &trail, &evidence);
        assert!(result.passed);
        assert_eq!(result.exit_code, 0);
        assert!(!result.framework_results.is_empty());
        assert!(result.framework_results[0].passing);
    }

    #[test]
    fn test_drift_detection() {
        let config = CiConfig {
            previous_score: Some(0.95),
            detect_drift: true,
            fail_on_critical: false,
            ..Default::default()
        };
        let trail = AuditTrail::new();
        let evidence = EvidenceCollector::new();

        let result = run_ci_check(&config, &trail, &evidence);
        assert!(result.drift_detected);
    }

    #[test]
    fn test_ci_json_output() {
        let result = CiCheckResult {
            passed: true,
            exit_code: 0,
            summary: "All passing".to_string(),
            framework_results: vec![],
            drift_detected: false,
            timestamp: 1234567890,
        };

        let json = to_ci_json(&result);
        assert!(json.contains("\"passed\": true"));
        assert!(json.contains("\"exit_code\": 0"));
    }

    #[test]
    fn test_multiple_frameworks() {
        let config = CiConfig {
            frameworks: vec!["soc2".to_string(), "hipaa".to_string()],
            max_untested: 100,
            fail_on_critical: false,
            ..Default::default()
        };
        let trail = AuditTrail::new();
        let evidence = EvidenceCollector::new();

        let result = run_ci_check(&config, &trail, &evidence);
        assert_eq!(result.framework_results.len(), 2);
    }
}
