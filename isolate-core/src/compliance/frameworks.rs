//! Compliance framework definitions and templates.

use serde::{Deserialize, Serialize};

/// Unique identifier for a compliance framework.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct FrameworkId(String);

impl FrameworkId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FrameworkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single compliance control requirement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Control {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub severity: ControlSeverity,
    pub automated: bool,
}

/// How critical a control is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlSeverity {
    Critical,
    High,
    Medium,
    Low,
}

/// Status of a control check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlStatus {
    Pass,
    Fail,
    NotApplicable,
    NotTested,
}

/// Supported compliance framework types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComplianceFramework {
    Soc2,
    Iso27001,
    Hipaa,
    PciDss,
    Gdpr,
}

impl std::fmt::Display for ComplianceFramework {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Soc2 => write!(f, "SOC 2 Type II"),
            Self::Iso27001 => write!(f, "ISO 27001"),
            Self::Hipaa => write!(f, "HIPAA"),
            Self::PciDss => write!(f, "PCI-DSS"),
            Self::Gdpr => write!(f, "GDPR"),
        }
    }
}

/// A complete framework template with controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkTemplate {
    pub id: FrameworkId,
    pub framework: ComplianceFramework,
    pub name: String,
    pub version: String,
    pub controls: Vec<Control>,
}

impl FrameworkTemplate {
    /// SOC 2 Type II template with common controls.
    pub fn soc2() -> Self {
        Self {
            id: FrameworkId::new("soc2-v2"),
            framework: ComplianceFramework::Soc2,
            name: "SOC 2 Type II".to_string(),
            version: "2024".to_string(),
            controls: vec![
                Control {
                    id: "CC1.1".into(), name: "COSO Principle 1".into(),
                    description: "The entity demonstrates commitment to integrity and ethical values".into(),
                    category: "Control Environment".into(), severity: ControlSeverity::High, automated: false,
                },
                Control {
                    id: "CC6.1".into(), name: "Logical Access Security".into(),
                    description: "Logical access security restricts access to information assets".into(),
                    category: "Logical and Physical Access Controls".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "CC6.3".into(), name: "Access Revocation".into(),
                    description: "Access to information assets is revoked when no longer required".into(),
                    category: "Logical and Physical Access Controls".into(), severity: ControlSeverity::High, automated: true,
                },
                Control {
                    id: "CC7.1".into(), name: "Monitoring Activities".into(),
                    description: "The entity monitors system components for anomalies".into(),
                    category: "System Operations".into(), severity: ControlSeverity::High, automated: true,
                },
                Control {
                    id: "CC7.2".into(), name: "Incident Response".into(),
                    description: "The entity monitors anomalies indicative of security incidents".into(),
                    category: "System Operations".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "CC8.1".into(), name: "Change Management".into(),
                    description: "Changes to infrastructure and software are managed".into(),
                    category: "Change Management".into(), severity: ControlSeverity::High, automated: true,
                },
            ],
        }
    }

    /// HIPAA template.
    pub fn hipaa() -> Self {
        Self {
            id: FrameworkId::new("hipaa-v1"),
            framework: ComplianceFramework::Hipaa,
            name: "HIPAA Security Rule".to_string(),
            version: "2024".to_string(),
            controls: vec![
                Control {
                    id: "164.312(a)(1)".into(), name: "Access Control".into(),
                    description: "Implement technical policies to allow access only to authorized persons".into(),
                    category: "Technical Safeguards".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "164.312(a)(2)(iv)".into(), name: "Encryption at Rest".into(),
                    description: "Implement mechanism to encrypt/decrypt ePHI".into(),
                    category: "Technical Safeguards".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "164.312(b)".into(), name: "Audit Controls".into(),
                    description: "Implement hardware/software mechanisms to record and examine access".into(),
                    category: "Technical Safeguards".into(), severity: ControlSeverity::High, automated: true,
                },
                Control {
                    id: "164.312(c)(1)".into(), name: "Data Integrity".into(),
                    description: "Protect ePHI from improper alteration or destruction".into(),
                    category: "Technical Safeguards".into(), severity: ControlSeverity::High, automated: true,
                },
                Control {
                    id: "164.312(e)(1)".into(), name: "Transmission Security".into(),
                    description: "Implement technical security measures to protect ePHI transmitted electronically".into(),
                    category: "Technical Safeguards".into(), severity: ControlSeverity::Critical, automated: true,
                },
            ],
        }
    }

    /// GDPR template.
    pub fn gdpr() -> Self {
        Self {
            id: FrameworkId::new("gdpr-v1"),
            framework: ComplianceFramework::Gdpr,
            name: "GDPR".to_string(),
            version: "2024".to_string(),
            controls: vec![
                Control {
                    id: "Art5.1(f)".into(), name: "Integrity and Confidentiality".into(),
                    description: "Personal data processed in a manner ensuring appropriate security".into(),
                    category: "Data Processing Principles".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "Art25.1".into(), name: "Data Protection by Design".into(),
                    description: "Implement data protection principles in processing activities".into(),
                    category: "Data Protection by Design".into(), severity: ControlSeverity::High, automated: false,
                },
                Control {
                    id: "Art30.1".into(), name: "Records of Processing".into(),
                    description: "Maintain a record of processing activities".into(),
                    category: "Record Keeping".into(), severity: ControlSeverity::High, automated: true,
                },
                Control {
                    id: "Art32.1".into(), name: "Security of Processing".into(),
                    description: "Implement appropriate technical and organisational measures".into(),
                    category: "Security".into(), severity: ControlSeverity::Critical, automated: true,
                },
            ],
        }
    }

    /// PCI-DSS template.
    pub fn pci_dss() -> Self {
        Self {
            id: FrameworkId::new("pci-dss-v4"),
            framework: ComplianceFramework::PciDss,
            name: "PCI DSS v4.0".to_string(),
            version: "4.0".to_string(),
            controls: vec![
                Control {
                    id: "Req1.1".into(), name: "Network Security Controls".into(),
                    description: "Install and maintain network security controls".into(),
                    category: "Build and Maintain a Secure Network".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "Req3.1".into(), name: "Protect Stored Account Data".into(),
                    description: "Account data storage is kept to a minimum".into(),
                    category: "Protect Account Data".into(), severity: ControlSeverity::Critical, automated: true,
                },
                Control {
                    id: "Req6.1".into(), name: "Secure Development".into(),
                    description: "Develop and maintain secure systems and software".into(),
                    category: "Secure Systems and Software".into(), severity: ControlSeverity::High, automated: true,
                },
                Control {
                    id: "Req10.1".into(), name: "Log and Monitor Access".into(),
                    description: "Log and monitor all access to system components".into(),
                    category: "Logging and Monitoring".into(), severity: ControlSeverity::High, automated: true,
                },
            ],
        }
    }

    /// Count automated controls.
    pub fn automated_count(&self) -> usize {
        self.controls.iter().filter(|c| c.automated).count()
    }

    /// Count critical controls.
    pub fn critical_count(&self) -> usize {
        self.controls.iter().filter(|c| c.severity == ControlSeverity::Critical).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soc2_template() {
        let t = FrameworkTemplate::soc2();
        assert_eq!(t.framework, ComplianceFramework::Soc2);
        assert_eq!(t.controls.len(), 6);
        assert!(t.automated_count() >= 4);
        assert!(t.critical_count() >= 2);
    }

    #[test]
    fn test_hipaa_template() {
        let t = FrameworkTemplate::hipaa();
        assert_eq!(t.framework, ComplianceFramework::Hipaa);
        assert_eq!(t.controls.len(), 5);
        assert!(t.critical_count() >= 3);
    }

    #[test]
    fn test_gdpr_template() {
        let t = FrameworkTemplate::gdpr();
        assert_eq!(t.framework, ComplianceFramework::Gdpr);
        assert_eq!(t.controls.len(), 4);
    }

    #[test]
    fn test_pci_dss_template() {
        let t = FrameworkTemplate::pci_dss();
        assert_eq!(t.framework, ComplianceFramework::PciDss);
        assert_eq!(t.controls.len(), 4);
    }

    #[test]
    fn test_framework_display() {
        assert_eq!(ComplianceFramework::Soc2.to_string(), "SOC 2 Type II");
        assert_eq!(ComplianceFramework::Hipaa.to_string(), "HIPAA");
        assert_eq!(ComplianceFramework::Gdpr.to_string(), "GDPR");
    }

    #[test]
    fn test_control_severity_critical() {
        let soc2 = FrameworkTemplate::soc2();
        let critical: Vec<_> = soc2.controls.iter().filter(|c| c.severity == ControlSeverity::Critical).collect();
        assert!(critical.len() >= 2);
    }
}
