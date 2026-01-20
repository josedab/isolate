//! Compliance auditing for secret access patterns.
//!
//! Tracks and reports on secret access for SOC2/ISO27001 compliance,
//! detecting anomalies and generating audit reports.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A secret access event for compliance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceEvent {
    pub timestamp_epoch_ms: u64,
    pub secret_path: String,
    pub accessor: String,
    pub access_type: ComplianceAccessType,
    pub success: bool,
    pub sandbox_id: Option<String>,
}

/// Types of secret access operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplianceAccessType {
    Read,
    Write,
    Delete,
    Rotate,
    List,
}

/// Compliance violation type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceViolation {
    pub violation_type: ViolationType,
    pub secret_path: String,
    pub details: String,
    pub severity: ViolationSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationType {
    UnauthorizedAccess,
    ExcessiveAccess,
    UnusedSecret,
    MissingRotation,
    AccessOutsideHours,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ViolationSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Compliance audit report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub total_accesses: u64,
    pub unique_secrets_accessed: usize,
    pub unique_accessors: usize,
    pub violations: Vec<ComplianceViolation>,
    pub access_by_type: HashMap<String, u64>,
    pub generated_epoch_ms: u64,
}

/// Auditor that tracks secret access patterns for compliance.
pub struct ComplianceAuditor {
    events: parking_lot::Mutex<Vec<ComplianceEvent>>,
    max_events: usize,
    max_access_per_hour: u64,
}

impl ComplianceAuditor {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: parking_lot::Mutex::new(Vec::with_capacity(max_events.min(1024))),
            max_events,
            max_access_per_hour: 1000,
        }
    }

    pub fn with_max_access_per_hour(mut self, max: u64) -> Self {
        self.max_access_per_hour = max;
        self
    }

    /// Record a secret access event.
    pub fn record(&self, event: ComplianceEvent) {
        let mut events = self.events.lock();
        if events.len() >= self.max_events {
            let half = events.len() / 2;
            events.drain(..half);
        }
        events.push(event);
    }

    /// Generate a compliance report from recorded events.
    pub fn generate_report(&self) -> ComplianceReport {
        let events = self.events.lock();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut secrets: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut accessors: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut access_by_type: HashMap<String, u64> = HashMap::new();
        let mut violations = Vec::new();

        // Track access frequency per accessor
        let mut accessor_counts: HashMap<String, u64> = HashMap::new();

        for event in events.iter() {
            secrets.insert(event.secret_path.clone());
            accessors.insert(event.accessor.clone());

            let type_name = format!("{:?}", event.access_type);
            *access_by_type.entry(type_name).or_insert(0) += 1;

            *accessor_counts
                .entry(event.accessor.clone())
                .or_insert(0) += 1;

            if !event.success {
                violations.push(ComplianceViolation {
                    violation_type: ViolationType::UnauthorizedAccess,
                    secret_path: event.secret_path.clone(),
                    details: format!(
                        "Failed {:?} by {}",
                        event.access_type, event.accessor
                    ),
                    severity: ViolationSeverity::High,
                });
            }
        }

        // Check for excessive access
        for (accessor, count) in &accessor_counts {
            if *count > self.max_access_per_hour {
                violations.push(ComplianceViolation {
                    violation_type: ViolationType::ExcessiveAccess,
                    secret_path: String::new(),
                    details: format!(
                        "Accessor '{}' made {} accesses (limit: {})",
                        accessor, count, self.max_access_per_hour
                    ),
                    severity: ViolationSeverity::Medium,
                });
            }
        }

        ComplianceReport {
            total_accesses: events.len() as u64,
            unique_secrets_accessed: secrets.len(),
            unique_accessors: accessors.len(),
            violations,
            access_by_type,
            generated_epoch_ms: now,
        }
    }

    /// Get all recorded events.
    pub fn events(&self) -> Vec<ComplianceEvent> {
        self.events.lock().clone()
    }

    /// Clear all events.
    pub fn clear(&self) {
        self.events.lock().clear();
    }
}

impl Default for ComplianceAuditor {
    fn default() -> Self {
        Self::new(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(path: &str, accessor: &str, success: bool) -> ComplianceEvent {
        ComplianceEvent {
            timestamp_epoch_ms: 0,
            secret_path: path.to_string(),
            accessor: accessor.to_string(),
            access_type: ComplianceAccessType::Read,
            success,
            sandbox_id: None,
        }
    }

    #[test]
    fn test_record_and_report() {
        let auditor = ComplianceAuditor::new(100);
        auditor.record(make_event("db/pass", "user-1", true));
        auditor.record(make_event("api/key", "user-2", true));

        let report = auditor.generate_report();
        assert_eq!(report.total_accesses, 2);
        assert_eq!(report.unique_secrets_accessed, 2);
        assert_eq!(report.unique_accessors, 2);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_failed_access_violation() {
        let auditor = ComplianceAuditor::new(100);
        auditor.record(make_event("secret/path", "attacker", false));

        let report = auditor.generate_report();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(
            report.violations[0].violation_type,
            ViolationType::UnauthorizedAccess
        );
        assert_eq!(report.violations[0].severity, ViolationSeverity::High);
    }

    #[test]
    fn test_excessive_access_detection() {
        let auditor = ComplianceAuditor::new(2000).with_max_access_per_hour(5);
        for _ in 0..10 {
            auditor.record(make_event("secret", "greedy-user", true));
        }

        let report = auditor.generate_report();
        let excessive: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.violation_type == ViolationType::ExcessiveAccess)
            .collect();
        assert_eq!(excessive.len(), 1);
    }

    #[test]
    fn test_access_by_type() {
        let auditor = ComplianceAuditor::new(100);
        auditor.record(make_event("a", "u", true));
        auditor.record(ComplianceEvent {
            access_type: ComplianceAccessType::Write,
            ..make_event("b", "u", true)
        });

        let report = auditor.generate_report();
        assert!(report.access_by_type.contains_key("Read"));
        assert!(report.access_by_type.contains_key("Write"));
    }

    #[test]
    fn test_buffer_eviction() {
        let auditor = ComplianceAuditor::new(5);
        for i in 0..10 {
            auditor.record(make_event(&format!("s{i}"), "u", true));
        }
        assert!(auditor.events().len() <= 5);
    }

    #[test]
    fn test_clear() {
        let auditor = ComplianceAuditor::new(100);
        auditor.record(make_event("s", "u", true));
        auditor.clear();
        assert!(auditor.events().is_empty());
    }
}
