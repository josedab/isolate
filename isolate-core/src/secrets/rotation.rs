//! Secret rotation management for automatic credential lifecycle.
//!
//! Tracks rotation schedules, enforces expiration policies, and triggers
//! sandbox restarts when secrets are rotated.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Rotation status for a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationStatus {
    /// Secret is current and within its rotation window.
    Current,
    /// Secret is approaching its rotation deadline.
    PendingRotation,
    /// Secret has exceeded its rotation deadline.
    Overdue,
    /// Secret has been rotated; old value may still be in use.
    Rotated,
}

/// Configuration for secret rotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationPolicy {
    /// Maximum age of a secret before it must be rotated.
    pub max_age: Duration,
    /// Warning threshold before max_age (triggers PendingRotation).
    pub warning_before: Duration,
    /// Whether to automatically restart sandboxes using the old secret.
    pub auto_restart_on_rotate: bool,
    /// Grace period during which both old and new secrets are valid.
    pub grace_period: Duration,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            max_age: Duration::from_secs(90 * 24 * 3600), // 90 days
            warning_before: Duration::from_secs(7 * 24 * 3600), // 7 days before
            auto_restart_on_rotate: true,
            grace_period: Duration::from_secs(3600), // 1 hour
        }
    }
}

/// Tracks a single secret's rotation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationRecord {
    pub secret_path: String,
    pub version: String,
    pub created_epoch_ms: u64,
    pub last_rotated_epoch_ms: Option<u64>,
    pub rotation_count: u32,
    pub status: RotationStatus,
}

/// Manager for secret rotation schedules and compliance.
pub struct RotationManager {
    policy: RotationPolicy,
    records: parking_lot::RwLock<HashMap<String, RotationRecord>>,
}

impl RotationManager {
    pub fn new(policy: RotationPolicy) -> Self {
        Self { policy, records: parking_lot::RwLock::new(HashMap::new()) }
    }

    /// Register a secret for rotation tracking.
    pub fn register(&self, secret_path: impl Into<String>, version: impl Into<String>) {
        let path = secret_path.into();
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

        self.records.write().insert(
            path.clone(),
            RotationRecord {
                secret_path: path,
                version: version.into(),
                created_epoch_ms: now,
                last_rotated_epoch_ms: None,
                rotation_count: 0,
                status: RotationStatus::Current,
            },
        );
    }

    /// Record that a secret has been rotated to a new version.
    pub fn record_rotation(&self, secret_path: &str, new_version: impl Into<String>) {
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

        if let Some(record) = self.records.write().get_mut(secret_path) {
            record.version = new_version.into();
            record.last_rotated_epoch_ms = Some(now);
            record.rotation_count += 1;
            record.status = RotationStatus::Rotated;
        }
    }

    /// Check and update rotation status for all tracked secrets.
    pub fn check_rotation_status(&self) -> Vec<RotationRecord> {
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

        let max_age_ms = self.policy.max_age.as_millis() as u64;
        let warning_ms = self.policy.warning_before.as_millis() as u64;

        let mut records = self.records.write();
        let mut results = Vec::new();

        for record in records.values_mut() {
            let age_ms = now.saturating_sub(record.created_epoch_ms);
            let effective_age = if let Some(rotated) = record.last_rotated_epoch_ms {
                now.saturating_sub(rotated)
            } else {
                age_ms
            };

            record.status = if effective_age > max_age_ms {
                RotationStatus::Overdue
            } else if effective_age > max_age_ms.saturating_sub(warning_ms) {
                RotationStatus::PendingRotation
            } else if record.last_rotated_epoch_ms.is_some()
                && record.status == RotationStatus::Rotated
            {
                RotationStatus::Current
            } else {
                RotationStatus::Current
            };

            results.push(record.clone());
        }

        results
    }

    /// Get secrets that need rotation.
    pub fn overdue_secrets(&self) -> Vec<RotationRecord> {
        self.check_rotation_status()
            .into_iter()
            .filter(|r| {
                matches!(r.status, RotationStatus::Overdue | RotationStatus::PendingRotation)
            })
            .collect()
    }

    /// Get a specific secret's rotation record.
    pub fn get_record(&self, secret_path: &str) -> Option<RotationRecord> {
        self.records.read().get(secret_path).cloned()
    }

    /// Number of tracked secrets.
    pub fn tracked_count(&self) -> usize {
        self.records.read().len()
    }

    /// Get the rotation policy.
    pub fn policy(&self) -> &RotationPolicy {
        &self.policy
    }
}

impl Default for RotationManager {
    fn default() -> Self {
        Self::new(RotationPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get() {
        let mgr = RotationManager::new(RotationPolicy::default());
        mgr.register("db/password", "v1");

        let record = mgr.get_record("db/password").unwrap();
        assert_eq!(record.version, "v1");
        assert_eq!(record.rotation_count, 0);
    }

    #[test]
    fn test_record_rotation() {
        let mgr = RotationManager::new(RotationPolicy::default());
        mgr.register("db/password", "v1");
        mgr.record_rotation("db/password", "v2");

        let record = mgr.get_record("db/password").unwrap();
        assert_eq!(record.version, "v2");
        assert_eq!(record.rotation_count, 1);
        assert!(record.last_rotated_epoch_ms.is_some());
    }

    #[test]
    fn test_check_status_current() {
        let mgr = RotationManager::new(RotationPolicy::default());
        mgr.register("api/key", "v1");

        let statuses = mgr.check_rotation_status();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].status, RotationStatus::Current);
    }

    #[test]
    fn test_overdue_detection() {
        let mgr = RotationManager::new(RotationPolicy {
            max_age: Duration::from_millis(1), // immediately overdue
            warning_before: Duration::from_millis(0),
            ..Default::default()
        });
        mgr.register("old/secret", "v1");

        // Small delay to ensure it's overdue
        std::thread::sleep(Duration::from_millis(5));

        let overdue = mgr.overdue_secrets();
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].secret_path, "old/secret");
    }

    #[test]
    fn test_tracked_count() {
        let mgr = RotationManager::new(RotationPolicy::default());
        mgr.register("a", "v1");
        mgr.register("b", "v1");
        assert_eq!(mgr.tracked_count(), 2);
    }

    #[test]
    fn test_missing_record() {
        let mgr = RotationManager::new(RotationPolicy::default());
        assert!(mgr.get_record("nonexistent").is_none());
    }

    #[test]
    fn test_multiple_rotations() {
        let mgr = RotationManager::new(RotationPolicy::default());
        mgr.register("key", "v1");
        mgr.record_rotation("key", "v2");
        mgr.record_rotation("key", "v3");
        mgr.record_rotation("key", "v4");

        let record = mgr.get_record("key").unwrap();
        assert_eq!(record.version, "v4");
        assert_eq!(record.rotation_count, 3);
    }
}
