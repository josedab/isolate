//! Compliance framework for regulated industries.
//!
//! Provides compliance assessment and reporting for frameworks such as
//! SOC 2, GDPR, HIPAA, ISO 27001, PCI DSS, and FedRAMP.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Supported compliance frameworks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComplianceFramework {
    /// General Data Protection Regulation (EU).
    Gdpr,
    /// Health Insurance Portability and Accountability Act (US).
    Hipaa,
    /// Service Organization Control 2.
    Soc2,
    /// Information Security Management (ISO).
    Iso27001,
    /// Payment Card Industry Data Security Standard.
    PciDss,
    /// Federal Risk and Authorization Management Program (US).
    FedRamp,
}

/// Status of a compliance control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlStatus {
    /// The control is fully met.
    Compliant,
    /// The control is not met.
    NonCompliant,
    /// The control is partially met.
    PartiallyCompliant,
    /// The control has not been assessed.
    NotAssessed,
}

/// A single compliance control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceControl {
    /// Control identifier (e.g. "CC6.1").
    pub control_id: String,
    /// Framework this control belongs to.
    pub framework: ComplianceFramework,
    /// Short title.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Current status.
    pub status: ControlStatus,
    /// Evidence items.
    pub evidence: Vec<String>,
    /// When the control was last assessed.
    pub last_assessed: SystemTime,
}

/// Summary statistics for a compliance assessment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComplianceSummary {
    /// Total number of controls assessed.
    pub total_controls: usize,
    /// Number of compliant controls.
    pub compliant: usize,
    /// Number of non-compliant controls.
    pub non_compliant: usize,
    /// Number of partially compliant controls.
    pub partially_compliant: usize,
    /// Number of controls not yet assessed.
    pub not_assessed: usize,
    /// Overall compliance percentage.
    pub compliance_percentage: f64,
}

/// A compliance assessment report for one framework.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    /// Framework assessed.
    pub framework: ComplianceFramework,
    /// When the assessment was performed.
    pub assessment_date: SystemTime,
    /// Individual control results.
    pub controls: Vec<ComplianceControl>,
    /// Summary statistics.
    pub summary: ComplianceSummary,
}

/// Assesses compliance against registered frameworks.
pub struct ComplianceAssessor {
    controls: HashMap<ComplianceFramework, Vec<ComplianceControl>>,
}

impl ComplianceAssessor {
    /// Create a new assessor with default SOC 2 and GDPR controls.
    pub fn new() -> Self {
        let mut assessor = Self { controls: HashMap::new() };
        assessor.register_framework(ComplianceFramework::Soc2, default_soc2_controls());
        assessor.register_framework(ComplianceFramework::Gdpr, default_gdpr_controls());
        assessor
    }

    /// Register (or replace) the controls for a framework.
    pub fn register_framework(
        &mut self,
        framework: ComplianceFramework,
        controls: Vec<ComplianceControl>,
    ) {
        self.controls.insert(framework, controls);
    }

    /// Assess a single framework and produce a report.
    pub fn assess(&self, framework: &ComplianceFramework) -> ComplianceReport {
        let controls = self.controls.get(framework).cloned().unwrap_or_default();
        let summary = build_summary(&controls);
        ComplianceReport {
            framework: framework.clone(),
            assessment_date: SystemTime::now(),
            controls,
            summary,
        }
    }

    /// Assess all registered frameworks.
    pub fn assess_all(&self) -> Vec<ComplianceReport> {
        self.controls.keys().map(|fw| self.assess(fw)).collect()
    }

    /// Get the current status of a specific control.
    pub fn get_control_status(
        &self,
        framework: &ComplianceFramework,
        control_id: &str,
    ) -> Option<&ControlStatus> {
        self.controls.get(framework)?.iter().find(|c| c.control_id == control_id).map(|c| &c.status)
    }

    /// Update the status and evidence for a specific control.
    pub fn update_control(
        &mut self,
        framework: &ComplianceFramework,
        control_id: &str,
        status: ControlStatus,
        evidence: Vec<String>,
    ) -> bool {
        if let Some(controls) = self.controls.get_mut(framework) {
            if let Some(control) = controls.iter_mut().find(|c| c.control_id == control_id) {
                control.status = status;
                control.evidence = evidence;
                control.last_assessed = SystemTime::now();
                return true;
            }
        }
        false
    }
}

impl Default for ComplianceAssessor {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Pre-populated controls
// ------------------------------------------------------------------

fn default_soc2_controls() -> Vec<ComplianceControl> {
    let now = SystemTime::now();
    vec![
        ComplianceControl {
            control_id: "CC6.1".into(),
            framework: ComplianceFramework::Soc2,
            title: "Logical and Physical Access Controls".into(),
            description: "Restrict logical access to information assets".into(),
            status: ControlStatus::Compliant,
            evidence: vec!["TEE-based isolation enforced".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "CC6.2".into(),
            framework: ComplianceFramework::Soc2,
            title: "System Access Authentication".into(),
            description: "Authenticate users and services before granting access".into(),
            status: ControlStatus::Compliant,
            evidence: vec!["Remote attestation verifies enclave identity".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "CC6.3".into(),
            framework: ComplianceFramework::Soc2,
            title: "Encryption of Data".into(),
            description: "Protect data at rest and in transit using encryption".into(),
            status: ControlStatus::Compliant,
            evidence: vec!["Sealed storage with hardware-backed keys".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "CC7.1".into(),
            framework: ComplianceFramework::Soc2,
            title: "System Monitoring".into(),
            description: "Monitor system components for anomalies".into(),
            status: ControlStatus::PartiallyCompliant,
            evidence: vec!["Audit log with hash chain integrity".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "CC7.2".into(),
            framework: ComplianceFramework::Soc2,
            title: "Incident Response".into(),
            description: "Detect and respond to security incidents".into(),
            status: ControlStatus::NotAssessed,
            evidence: Vec::new(),
            last_assessed: now,
        },
    ]
}

fn default_gdpr_controls() -> Vec<ComplianceControl> {
    let now = SystemTime::now();
    vec![
        ComplianceControl {
            control_id: "GDPR-32".into(),
            framework: ComplianceFramework::Gdpr,
            title: "Security of Processing".into(),
            description: "Implement appropriate technical and organizational measures".into(),
            status: ControlStatus::Compliant,
            evidence: vec!["Hardware-backed TEE isolation".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "GDPR-25".into(),
            framework: ComplianceFramework::Gdpr,
            title: "Data Protection by Design".into(),
            description: "Implement data protection principles in processing design".into(),
            status: ControlStatus::Compliant,
            evidence: vec!["Capability-based access controls".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "GDPR-30".into(),
            framework: ComplianceFramework::Gdpr,
            title: "Records of Processing Activities".into(),
            description: "Maintain records of data processing activities".into(),
            status: ControlStatus::PartiallyCompliant,
            evidence: vec!["Audit trail records processing events".into()],
            last_assessed: now,
        },
        ComplianceControl {
            control_id: "GDPR-33".into(),
            framework: ComplianceFramework::Gdpr,
            title: "Notification of Data Breach".into(),
            description: "Notify authorities of personal data breaches within 72 hours".into(),
            status: ControlStatus::NotAssessed,
            evidence: Vec::new(),
            last_assessed: now,
        },
    ]
}

fn build_summary(controls: &[ComplianceControl]) -> ComplianceSummary {
    let total = controls.len();
    let compliant = controls.iter().filter(|c| c.status == ControlStatus::Compliant).count();
    let non_compliant = controls.iter().filter(|c| c.status == ControlStatus::NonCompliant).count();
    let partially =
        controls.iter().filter(|c| c.status == ControlStatus::PartiallyCompliant).count();
    let not_assessed = controls.iter().filter(|c| c.status == ControlStatus::NotAssessed).count();
    let pct = if total > 0 { (compliant as f64 / total as f64) * 100.0 } else { 0.0 };
    ComplianceSummary {
        total_controls: total,
        compliant,
        non_compliant,
        partially_compliant: partially,
        not_assessed,
        compliance_percentage: pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_has_default_frameworks() {
        let assessor = ComplianceAssessor::new();
        assert!(assessor.controls.contains_key(&ComplianceFramework::Soc2));
        assert!(assessor.controls.contains_key(&ComplianceFramework::Gdpr));
    }

    #[test]
    fn test_assess_soc2() {
        let assessor = ComplianceAssessor::new();
        let report = assessor.assess(&ComplianceFramework::Soc2);
        assert_eq!(report.framework, ComplianceFramework::Soc2);
        assert_eq!(report.summary.total_controls, 5);
        assert_eq!(report.summary.compliant, 3);
        assert!(report.summary.compliance_percentage > 0.0);
    }

    #[test]
    fn test_assess_gdpr() {
        let assessor = ComplianceAssessor::new();
        let report = assessor.assess(&ComplianceFramework::Gdpr);
        assert_eq!(report.framework, ComplianceFramework::Gdpr);
        assert_eq!(report.summary.total_controls, 4);
        assert_eq!(report.summary.compliant, 2);
    }

    #[test]
    fn test_assess_all() {
        let assessor = ComplianceAssessor::new();
        let reports = assessor.assess_all();
        assert_eq!(reports.len(), 2);
    }

    #[test]
    fn test_get_control_status() {
        let assessor = ComplianceAssessor::new();
        let status = assessor.get_control_status(&ComplianceFramework::Soc2, "CC6.1");
        assert_eq!(status, Some(&ControlStatus::Compliant));

        let missing = assessor.get_control_status(&ComplianceFramework::Soc2, "MISSING");
        assert!(missing.is_none());
    }

    #[test]
    fn test_update_control() {
        let mut assessor = ComplianceAssessor::new();
        let updated = assessor.update_control(
            &ComplianceFramework::Soc2,
            "CC7.2",
            ControlStatus::Compliant,
            vec!["Incident runbook documented".into()],
        );
        assert!(updated);
        let status = assessor.get_control_status(&ComplianceFramework::Soc2, "CC7.2");
        assert_eq!(status, Some(&ControlStatus::Compliant));
    }

    #[test]
    fn test_update_nonexistent_control() {
        let mut assessor = ComplianceAssessor::new();
        let updated = assessor.update_control(
            &ComplianceFramework::Hipaa,
            "HIPAA-1",
            ControlStatus::Compliant,
            Vec::new(),
        );
        assert!(!updated);
    }

    #[test]
    fn test_register_custom_framework() {
        let mut assessor = ComplianceAssessor::new();
        let controls = vec![ComplianceControl {
            control_id: "HIPAA-164.312".into(),
            framework: ComplianceFramework::Hipaa,
            title: "Access Control".into(),
            description: "Implement technical policies for electronic information systems".into(),
            status: ControlStatus::NotAssessed,
            evidence: Vec::new(),
            last_assessed: SystemTime::now(),
        }];
        assessor.register_framework(ComplianceFramework::Hipaa, controls);
        let report = assessor.assess(&ComplianceFramework::Hipaa);
        assert_eq!(report.summary.total_controls, 1);
        assert_eq!(report.summary.not_assessed, 1);
    }

    #[test]
    fn test_compliance_summary_percentages() {
        let controls = vec![
            ComplianceControl {
                control_id: "A".into(),
                framework: ComplianceFramework::Soc2,
                title: "A".into(),
                description: "".into(),
                status: ControlStatus::Compliant,
                evidence: Vec::new(),
                last_assessed: SystemTime::now(),
            },
            ComplianceControl {
                control_id: "B".into(),
                framework: ComplianceFramework::Soc2,
                title: "B".into(),
                description: "".into(),
                status: ControlStatus::NonCompliant,
                evidence: Vec::new(),
                last_assessed: SystemTime::now(),
            },
        ];
        let summary = build_summary(&controls);
        assert_eq!(summary.total_controls, 2);
        assert_eq!(summary.compliant, 1);
        assert_eq!(summary.non_compliant, 1);
        assert!((summary.compliance_percentage - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_assess_unknown_framework_empty() {
        let assessor = ComplianceAssessor::new();
        let report = assessor.assess(&ComplianceFramework::FedRamp);
        assert_eq!(report.summary.total_controls, 0);
        assert!((report.summary.compliance_percentage - 0.0).abs() < f64::EPSILON);
    }
}
