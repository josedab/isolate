//! Compliance Autopilot.
//!
//! Pre-configured policy templates and tamper-proof audit trails for
//! SOC2, ISO 27001, HIPAA, PCI-DSS, and GDPR frameworks.
//!
//! # Features
//!
//! - **Framework Templates**: Ready-to-use compliance control mappings
//! - **Audit Trail**: Cryptographically chained, tamper-proof event log
//! - **Evidence Collection**: Automated gathering of compliance evidence
//! - **Report Generation**: Audit-ready compliance reports

#![allow(dead_code)]

pub mod audit_trail;
pub mod evidence;
pub mod frameworks;
pub mod reports;

pub use audit_trail::{AuditTrail, AuditEntry, AuditChain};
pub use evidence::{EvidenceCollector, Evidence, EvidenceType};
pub use frameworks::{ComplianceFramework, FrameworkId, Control, ControlStatus, FrameworkTemplate};
pub use reports::{ComplianceReport, ReportGenerator, ControlCoverage};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_end_to_end_compliance_flow() {
        // Create framework from template
        let template = FrameworkTemplate::soc2();
        assert!(!template.controls.is_empty());

        // Set up audit trail
        let trail = AuditTrail::new();
        trail.record("sandbox.create", "Sandbox created with restricted capabilities", "system");
        trail.record("capability.check", "Filesystem read denied for /etc/passwd", "enforcer");

        // Collect evidence
        let collector = EvidenceCollector::new();
        collector.add(Evidence {
            id: "ev-1".into(),
            control_id: template.controls[0].id.clone(),
            evidence_type: EvidenceType::Log,
            description: "Access control log showing denied access".into(),
            collected_at: 1000,
        });

        // Generate report
        let generator = ReportGenerator::new();
        let report = generator.generate(&template, &trail, &collector);
        assert_eq!(report.framework_id, template.id);
        assert!(report.overall_score > 0.0);
    }
}
