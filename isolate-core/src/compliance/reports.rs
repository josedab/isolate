//! Compliance report generation.

use serde::{Deserialize, Serialize};

use super::audit_trail::AuditTrail;
use super::evidence::EvidenceCollector;
use super::frameworks::{ControlSeverity, ControlStatus, FrameworkId, FrameworkTemplate};

/// Coverage status for a single control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCoverage {
    pub control_id: String,
    pub control_name: String,
    pub severity: ControlSeverity,
    pub status: ControlStatus,
    pub evidence_count: usize,
    pub audit_entries: usize,
    pub notes: String,
}

/// A generated compliance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub framework_id: FrameworkId,
    pub framework_name: String,
    pub generated_at: u64,
    pub controls: Vec<ControlCoverage>,
    pub overall_score: f64,
    pub total_controls: usize,
    pub passed_controls: usize,
    pub failed_controls: usize,
    pub not_tested: usize,
}

impl ComplianceReport {
    /// Is the overall compliance passing (>80% controls pass)?
    pub fn is_passing(&self) -> bool {
        self.overall_score >= 0.80
    }

    /// Get critical controls that failed.
    pub fn critical_failures(&self) -> Vec<&ControlCoverage> {
        self.controls
            .iter()
            .filter(|c| c.severity == ControlSeverity::Critical && c.status == ControlStatus::Fail)
            .collect()
    }

    /// Controls that still need testing.
    pub fn untested_controls(&self) -> Vec<&ControlCoverage> {
        self.controls.iter().filter(|c| c.status == ControlStatus::NotTested).collect()
    }
}

/// Report generator that combines framework, audit trail, and evidence.
pub struct ReportGenerator;

impl ReportGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate a compliance report.
    pub fn generate(
        &self,
        template: &FrameworkTemplate,
        trail: &AuditTrail,
        evidence: &EvidenceCollector,
    ) -> ComplianceReport {
        let coverage_map = evidence.coverage_map();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut controls = Vec::new();
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut not_tested = 0usize;

        for control in &template.controls {
            let ev_count = coverage_map.get(&control.id).copied().unwrap_or(0);
            let audit_count = trail.entries_by_type(&control.id).len()
                + trail.entries_by_type(&control.category).len();

            // Determine status based on evidence and audit trail
            let status = if ev_count >= 1 && (audit_count > 0 || !control.automated) {
                passed += 1;
                ControlStatus::Pass
            } else if ev_count == 0 && audit_count == 0 {
                not_tested += 1;
                ControlStatus::NotTested
            } else {
                failed += 1;
                ControlStatus::Fail
            };

            let notes = match status {
                ControlStatus::Pass => {
                    format!("{} evidence items, {} audit entries", ev_count, audit_count)
                }
                ControlStatus::Fail => "Insufficient evidence or audit trail".to_string(),
                ControlStatus::NotTested => "No evidence collected".to_string(),
                ControlStatus::NotApplicable => "Not applicable".to_string(),
            };

            controls.push(ControlCoverage {
                control_id: control.id.clone(),
                control_name: control.name.clone(),
                severity: control.severity,
                status,
                evidence_count: ev_count,
                audit_entries: audit_count,
                notes,
            });
        }

        let total = template.controls.len();
        let score = if total > 0 { passed as f64 / total as f64 } else { 0.0 };

        ComplianceReport {
            framework_id: template.id.clone(),
            framework_name: template.name.clone(),
            generated_at: ts,
            controls,
            overall_score: score,
            total_controls: total,
            passed_controls: passed,
            failed_controls: failed,
            not_tested,
        }
    }
}

impl Default for ReportGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance::evidence::{Evidence, EvidenceType};
    use crate::compliance::frameworks::FrameworkTemplate;

    #[test]
    fn test_report_with_full_evidence() {
        let template = FrameworkTemplate::soc2();
        let trail = AuditTrail::new();
        let collector = EvidenceCollector::new();

        // Add evidence and audit entries for each control
        for control in &template.controls {
            collector.add(Evidence {
                id: format!("ev-{}", control.id),
                control_id: control.id.clone(),
                evidence_type: EvidenceType::Log,
                description: "Test evidence".into(),
                collected_at: 1000,
            });
            trail.record_at(&control.category, &format!("Tested {}", control.id), "auditor", 1000);
        }

        let gen = ReportGenerator::new();
        let report = gen.generate(&template, &trail, &collector);

        assert!(report.is_passing());
        assert_eq!(report.total_controls, 6);
        assert_eq!(report.passed_controls, 6);
        assert_eq!(report.failed_controls, 0);
    }

    #[test]
    fn test_report_with_no_evidence() {
        let template = FrameworkTemplate::hipaa();
        let trail = AuditTrail::new();
        let collector = EvidenceCollector::new();

        let gen = ReportGenerator::new();
        let report = gen.generate(&template, &trail, &collector);

        assert!(!report.is_passing());
        assert_eq!(report.not_tested, template.controls.len());
        assert_eq!(report.overall_score, 0.0);
    }

    #[test]
    fn test_critical_failures() {
        let template = FrameworkTemplate::soc2();
        let trail = AuditTrail::new();
        let collector = EvidenceCollector::new();

        // Only add evidence for non-critical controls
        for control in &template.controls {
            if control.severity != ControlSeverity::Critical {
                collector.add(Evidence {
                    id: format!("ev-{}", control.id),
                    control_id: control.id.clone(),
                    evidence_type: EvidenceType::TestResult,
                    description: "OK".into(),
                    collected_at: 1000,
                });
                trail.record_at(&control.id, "passed", "bot", 1000);
            }
        }

        let gen = ReportGenerator::new();
        let report = gen.generate(&template, &trail, &collector);

        // Critical controls without evidence should not pass
        let non_passing: Vec<_> = report
            .controls
            .iter()
            .filter(|c| c.severity == ControlSeverity::Critical && c.status != ControlStatus::Pass)
            .collect();
        assert!(!non_passing.is_empty());
    }

    #[test]
    fn test_partial_compliance() {
        let template = FrameworkTemplate::gdpr();
        let trail = AuditTrail::new();
        let collector = EvidenceCollector::new();

        // Add evidence for half the controls
        let half = template.controls.len() / 2;
        for control in template.controls.iter().take(half) {
            collector.add(Evidence {
                id: format!("ev-{}", control.id),
                control_id: control.id.clone(),
                evidence_type: EvidenceType::Configuration,
                description: "Config snapshot".into(),
                collected_at: 1000,
            });
            trail.record_at(&control.category, "verified", "sys", 1000);
        }

        let gen = ReportGenerator::new();
        let report = gen.generate(&template, &trail, &collector);

        assert!(report.overall_score > 0.0);
        assert!(report.overall_score < 1.0);
        assert!(report.passed_controls > 0);
    }

    #[test]
    fn test_report_framework_metadata() {
        let template = FrameworkTemplate::pci_dss();
        let trail = AuditTrail::new();
        let collector = EvidenceCollector::new();

        let gen = ReportGenerator::new();
        let report = gen.generate(&template, &trail, &collector);

        assert_eq!(report.framework_id.as_str(), "pci-dss-v4");
        assert_eq!(report.framework_name, "PCI DSS v4.0");
    }
}
