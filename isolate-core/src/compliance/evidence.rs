//! Evidence collection for compliance audits.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Type of compliance evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceType {
    Log,
    Configuration,
    Screenshot,
    PolicyDocument,
    TestResult,
    AccessReview,
}

impl std::fmt::Display for EvidenceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Log => write!(f, "Log"),
            Self::Configuration => write!(f, "Configuration"),
            Self::Screenshot => write!(f, "Screenshot"),
            Self::PolicyDocument => write!(f, "Policy Document"),
            Self::TestResult => write!(f, "Test Result"),
            Self::AccessReview => write!(f, "Access Review"),
        }
    }
}

/// A piece of compliance evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub control_id: String,
    pub evidence_type: EvidenceType,
    pub description: String,
    pub collected_at: u64,
}

/// Collector that gathers and organizes compliance evidence.
#[derive(Clone)]
pub struct EvidenceCollector {
    inner: Arc<EvidenceCollectorInner>,
}

struct EvidenceCollectorInner {
    evidence: RwLock<Vec<Evidence>>,
}

impl EvidenceCollector {
    pub fn new() -> Self {
        Self { inner: Arc::new(EvidenceCollectorInner { evidence: RwLock::new(Vec::new()) }) }
    }

    /// Add evidence to the collection.
    pub fn add(&self, evidence: Evidence) {
        self.inner.evidence.write().push(evidence);
    }

    /// Get all evidence for a specific control.
    pub fn for_control(&self, control_id: &str) -> Vec<Evidence> {
        self.inner.evidence.read().iter().filter(|e| e.control_id == control_id).cloned().collect()
    }

    /// Get all evidence of a specific type.
    pub fn by_type(&self, evidence_type: EvidenceType) -> Vec<Evidence> {
        self.inner
            .evidence
            .read()
            .iter()
            .filter(|e| e.evidence_type == evidence_type)
            .cloned()
            .collect()
    }

    /// Get coverage map: control_id → number of evidence items.
    pub fn coverage_map(&self) -> HashMap<String, usize> {
        let evidence = self.inner.evidence.read();
        let mut map = HashMap::new();
        for e in evidence.iter() {
            *map.entry(e.control_id.clone()).or_insert(0) += 1;
        }
        map
    }

    /// Total evidence count.
    pub fn count(&self) -> usize {
        self.inner.evidence.read().len()
    }

    /// Get all evidence items.
    pub fn all(&self) -> Vec<Evidence> {
        self.inner.evidence.read().clone()
    }

    /// Check if a control has sufficient evidence (at least `min` items).
    pub fn has_sufficient(&self, control_id: &str, min: usize) -> bool {
        self.for_control(control_id).len() >= min
    }
}

impl Default for EvidenceCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_evidence(id: &str, control: &str, etype: EvidenceType) -> Evidence {
        Evidence {
            id: id.into(),
            control_id: control.into(),
            evidence_type: etype,
            description: format!("Evidence {}", id),
            collected_at: 1000,
        }
    }

    #[test]
    fn test_add_and_retrieve() {
        let c = EvidenceCollector::new();
        c.add(sample_evidence("e1", "CC6.1", EvidenceType::Log));
        c.add(sample_evidence("e2", "CC6.1", EvidenceType::Configuration));
        c.add(sample_evidence("e3", "CC7.1", EvidenceType::TestResult));

        assert_eq!(c.count(), 3);
        assert_eq!(c.for_control("CC6.1").len(), 2);
        assert_eq!(c.for_control("CC7.1").len(), 1);
    }

    #[test]
    fn test_by_type() {
        let c = EvidenceCollector::new();
        c.add(sample_evidence("e1", "c1", EvidenceType::Log));
        c.add(sample_evidence("e2", "c2", EvidenceType::Log));
        c.add(sample_evidence("e3", "c1", EvidenceType::Configuration));

        assert_eq!(c.by_type(EvidenceType::Log).len(), 2);
        assert_eq!(c.by_type(EvidenceType::Configuration).len(), 1);
        assert_eq!(c.by_type(EvidenceType::Screenshot).len(), 0);
    }

    #[test]
    fn test_coverage_map() {
        let c = EvidenceCollector::new();
        c.add(sample_evidence("e1", "c1", EvidenceType::Log));
        c.add(sample_evidence("e2", "c1", EvidenceType::TestResult));
        c.add(sample_evidence("e3", "c2", EvidenceType::Log));

        let map = c.coverage_map();
        assert_eq!(map.get("c1"), Some(&2));
        assert_eq!(map.get("c2"), Some(&1));
    }

    #[test]
    fn test_sufficient_evidence() {
        let c = EvidenceCollector::new();
        c.add(sample_evidence("e1", "c1", EvidenceType::Log));
        assert!(c.has_sufficient("c1", 1));
        assert!(!c.has_sufficient("c1", 2));
        assert!(!c.has_sufficient("c2", 1));
    }

    #[test]
    fn test_evidence_type_display() {
        assert_eq!(EvidenceType::Log.to_string(), "Log");
        assert_eq!(EvidenceType::PolicyDocument.to_string(), "Policy Document");
    }
}
