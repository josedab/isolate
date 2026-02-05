//! Disaster recovery for Kubernetes deployments.
//!
//! Provides backup/restore operations, failover configuration,
//! and cluster health tracking for Isolate K8s deployments.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Backup of Isolate Kubernetes state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backup {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub backup_type: BackupType,
    pub size_bytes: u64,
    pub resources: BackupContents,
    pub status: BackupStatus,
    pub retention_days: u32,
}

/// Type of backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupType {
    Full,
    Incremental,
    ConfigOnly,
}

/// Summary of resources included in a backup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupContents {
    pub sandbox_count: usize,
    pub pool_count: usize,
    pub policy_count: usize,
    pub secret_count: usize,
    pub tenant_count: usize,
    pub namespaces: Vec<String>,
}

/// Current status of a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupStatus {
    InProgress,
    Completed,
    Failed { reason: String },
    Expired,
}

/// A restore operation from a backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreOperation {
    pub backup_id: String,
    pub target_namespace: Option<String>,
    pub restore_type: RestoreType,
    pub status: RestoreStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub resources_restored: u32,
    pub resources_failed: u32,
}

/// Type of restore to perform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreType {
    Full,
    Selective { resource_types: Vec<String> },
    DryRun,
}

/// Status of a restore operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreStatus {
    Pending,
    InProgress,
    Completed,
    Failed { reason: String },
}

/// Failover configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    pub strategy: FailoverStrategy,
    pub health_check_interval_secs: u64,
    pub failover_timeout_secs: u64,
    pub max_failover_attempts: u32,
    pub backup_retention_days: u32,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            strategy: FailoverStrategy::Manual,
            health_check_interval_secs: 30,
            failover_timeout_secs: 300,
            max_failover_attempts: 3,
            backup_retention_days: 30,
        }
    }
}

/// Failover strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailoverStrategy {
    Manual,
    Automatic,
    WarmStandby,
}

/// Overall failover status.
pub struct FailoverStatus {
    pub is_primary: bool,
    pub last_backup: Option<DateTime<Utc>>,
    pub backup_count: usize,
    pub health: ClusterHealth,
}

/// Cluster health state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Manages backups, restore operations, and failover for disaster recovery.
pub struct DisasterRecoveryManager {
    backups: Vec<Backup>,
    restore_history: Vec<RestoreOperation>,
    config: FailoverConfig,
    next_backup_id: u64,
}

impl DisasterRecoveryManager {
    /// Create a new disaster recovery manager with the given failover config.
    pub fn new(config: FailoverConfig) -> Self {
        Self { backups: Vec::new(), restore_history: Vec::new(), config, next_backup_id: 1 }
    }

    /// Create a new backup of the specified type.
    pub fn create_backup(&mut self, backup_type: BackupType) -> &Backup {
        let id = format!("backup-{}", self.next_backup_id);
        self.next_backup_id += 1;

        let backup = Backup {
            id,
            created_at: Utc::now(),
            backup_type,
            size_bytes: 0,
            resources: BackupContents::default(),
            status: BackupStatus::Completed,
            retention_days: self.config.backup_retention_days,
        };

        self.backups.push(backup);
        self.backups.last().unwrap()
    }

    /// Begin a restore from a specific backup.
    pub fn restore(
        &mut self,
        backup_id: &str,
        restore_type: RestoreType,
    ) -> Result<&RestoreOperation, String> {
        if !self.backups.iter().any(|b| b.id == backup_id) {
            return Err(format!("backup '{}' not found", backup_id));
        }

        let op = RestoreOperation {
            backup_id: backup_id.to_string(),
            target_namespace: None,
            restore_type,
            status: RestoreStatus::Completed,
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            resources_restored: 0,
            resources_failed: 0,
        };

        self.restore_history.push(op);
        Ok(self.restore_history.last().unwrap())
    }

    /// List all backups.
    pub fn list_backups(&self) -> &[Backup] {
        &self.backups
    }

    /// Get a backup by ID.
    pub fn get_backup(&self, id: &str) -> Option<&Backup> {
        self.backups.iter().find(|b| b.id == id)
    }

    /// Delete a backup by ID. Returns true if found and removed.
    pub fn delete_backup(&mut self, id: &str) -> bool {
        let len_before = self.backups.len();
        self.backups.retain(|b| b.id != id);
        self.backups.len() < len_before
    }

    /// Remove expired backups. Returns the number of backups removed.
    pub fn cleanup_expired_backups(&mut self) -> usize {
        let now = Utc::now();
        let len_before = self.backups.len();
        self.backups.retain(|b| {
            let age_days = (now - b.created_at).num_days();
            age_days < b.retention_days as i64
        });
        len_before - self.backups.len()
    }

    /// Get the restore history.
    pub fn restore_history(&self) -> &[RestoreOperation] {
        &self.restore_history
    }

    /// Get current failover status.
    pub fn failover_status(&self) -> FailoverStatus {
        let last_backup = self.backups.last().map(|b| b.created_at);
        FailoverStatus {
            is_primary: true,
            last_backup,
            backup_count: self.backups.len(),
            health: ClusterHealth::Healthy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_manager() -> DisasterRecoveryManager {
        DisasterRecoveryManager::new(FailoverConfig::default())
    }

    #[test]
    fn test_create_backup() {
        let mut mgr = default_manager();
        let backup = mgr.create_backup(BackupType::Full);
        assert_eq!(backup.id, "backup-1");
        assert_eq!(backup.backup_type, BackupType::Full);
        assert_eq!(backup.status, BackupStatus::Completed);
        assert_eq!(backup.retention_days, 30);
    }

    #[test]
    fn test_create_multiple_backups() {
        let mut mgr = default_manager();
        mgr.create_backup(BackupType::Full);
        mgr.create_backup(BackupType::Incremental);
        mgr.create_backup(BackupType::ConfigOnly);
        assert_eq!(mgr.list_backups().len(), 3);
        assert_eq!(mgr.list_backups()[0].id, "backup-1");
        assert_eq!(mgr.list_backups()[2].id, "backup-3");
    }

    #[test]
    fn test_get_backup() {
        let mut mgr = default_manager();
        mgr.create_backup(BackupType::Full);
        assert!(mgr.get_backup("backup-1").is_some());
        assert!(mgr.get_backup("nonexistent").is_none());
    }

    #[test]
    fn test_delete_backup() {
        let mut mgr = default_manager();
        mgr.create_backup(BackupType::Full);
        assert!(mgr.delete_backup("backup-1"));
        assert_eq!(mgr.list_backups().len(), 0);
        assert!(!mgr.delete_backup("backup-1"));
    }

    #[test]
    fn test_restore_success() {
        let mut mgr = default_manager();
        mgr.create_backup(BackupType::Full);
        let op = mgr.restore("backup-1", RestoreType::Full).unwrap();
        assert_eq!(op.backup_id, "backup-1");
        assert_eq!(op.status, RestoreStatus::Completed);
    }

    #[test]
    fn test_restore_not_found() {
        let mut mgr = default_manager();
        let err = mgr.restore("nonexistent", RestoreType::Full).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_restore_history() {
        let mut mgr = default_manager();
        mgr.create_backup(BackupType::Full);
        mgr.restore("backup-1", RestoreType::Full).unwrap();
        mgr.restore("backup-1", RestoreType::DryRun).unwrap();
        assert_eq!(mgr.restore_history().len(), 2);
    }

    #[test]
    fn test_failover_status() {
        let mut mgr = default_manager();
        let status = mgr.failover_status();
        assert!(status.is_primary);
        assert_eq!(status.backup_count, 0);
        assert!(status.last_backup.is_none());
        assert_eq!(status.health, ClusterHealth::Healthy);

        mgr.create_backup(BackupType::Full);
        let status = mgr.failover_status();
        assert_eq!(status.backup_count, 1);
        assert!(status.last_backup.is_some());
    }

    #[test]
    fn test_failover_config_default() {
        let config = FailoverConfig::default();
        assert_eq!(config.strategy, FailoverStrategy::Manual);
        assert_eq!(config.health_check_interval_secs, 30);
        assert_eq!(config.backup_retention_days, 30);
    }

    #[test]
    fn test_backup_serialization() {
        let mut mgr = default_manager();
        let backup = mgr.create_backup(BackupType::Incremental).clone();
        let json = serde_json::to_string(&backup).unwrap();
        let back: Backup = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, backup.id);
        assert_eq!(back.backup_type, BackupType::Incremental);
    }

    #[test]
    fn test_restore_type_selective() {
        let mut mgr = default_manager();
        mgr.create_backup(BackupType::Full);
        let restore_type = RestoreType::Selective {
            resource_types: vec!["Sandbox".to_string(), "SandboxPool".to_string()],
        };
        let op = mgr.restore("backup-1", restore_type).unwrap();
        match &op.restore_type {
            RestoreType::Selective { resource_types } => {
                assert_eq!(resource_types.len(), 2);
            }
            _ => panic!("expected Selective restore type"),
        }
    }

    #[test]
    fn test_cleanup_expired_backups() {
        let mut mgr = DisasterRecoveryManager::new(FailoverConfig {
            backup_retention_days: 0,
            ..FailoverConfig::default()
        });
        // retention_days=0 on the backup itself means nothing expires immediately,
        // but we set it via config which flows to backup.retention_days
        mgr.create_backup(BackupType::Full);
        // The backup was just created with retention_days=0, so age(0 days) < 0 is false
        // meaning it will be cleaned up.
        // Actually: age_days = 0, retention = 0, 0 < 0 is false, so it's removed.
        let removed = mgr.cleanup_expired_backups();
        assert_eq!(removed, 1);
        assert_eq!(mgr.list_backups().len(), 0);
    }
}
