//! Checkpoint persistence for cross-node snapshot restore.
//!
//! Provides checkpoint creation, persistence to disk, and restore operations
//! with cross-node compatibility validation.

use super::storage::{SnapshotEntry, SnapshotStore, StorageStats};
use super::{Snapshot, SnapshotId};
use crate::config::ModuleHash;
use crate::error::{Error, Result};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Checkpoint metadata with cross-node restore information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Unique checkpoint ID.
    pub id: SnapshotId,
    /// Module hash this checkpoint belongs to.
    pub module_hash: ModuleHash,
    /// When the checkpoint was created.
    pub created_at: DateTime<Utc>,
    /// Node ID where the checkpoint was created.
    pub origin_node: String,
    /// Wasmtime engine version for compatibility.
    pub engine_version: String,
    /// Memory page count at checkpoint time.
    pub page_count: usize,
    /// Total memory size in bytes.
    pub memory_size: usize,
    /// Number of globals captured.
    pub global_count: usize,
    /// Whether this is an incremental checkpoint.
    pub is_incremental: bool,
    /// Parent checkpoint ID for incremental chains.
    pub parent_id: Option<SnapshotId>,
    /// User-defined labels.
    pub labels: HashMap<String, String>,
    /// Platform compatibility hash (arch + engine version).
    pub compat_hash: String,
}

impl Checkpoint {
    /// Create a checkpoint from a snapshot.
    pub fn from_snapshot(snapshot: &Snapshot, node_id: &str) -> Self {
        let engine_version = env!("CARGO_PKG_VERSION").to_string();
        let compat_hash = format!(
            "{}-{}-{}",
            std::env::consts::ARCH,
            std::env::consts::OS,
            engine_version
        );

        Self {
            id: snapshot.id,
            module_hash: snapshot.module_hash.clone(),
            created_at: snapshot.created_at,
            origin_node: node_id.to_string(),
            engine_version: engine_version.clone(),
            page_count: snapshot.memory_pages.len(),
            memory_size: snapshot.memory_size,
            global_count: snapshot.globals.len(),
            is_incremental: snapshot.parent_id.is_some(),
            parent_id: snapshot.parent_id,
            labels: snapshot.metadata.custom.clone(),
            compat_hash,
        }
    }

    /// Check if this checkpoint is compatible with the current node.
    pub fn is_compatible(&self) -> bool {
        let current_compat = format!(
            "{}-{}-{}",
            std::env::consts::ARCH,
            std::env::consts::OS,
            env!("CARGO_PKG_VERSION"),
        );
        self.compat_hash == current_compat
    }

    /// Validate that a checkpoint can be restored.
    pub fn validate_for_restore(&self) -> RestoreValidation {
        let mut issues = Vec::new();
        let mut warnings = Vec::new();

        if !self.is_compatible() {
            issues.push(format!(
                "Incompatible platform: checkpoint from '{}', current '{}-{}-{}'",
                self.compat_hash,
                std::env::consts::ARCH,
                std::env::consts::OS,
                env!("CARGO_PKG_VERSION"),
            ));
        }

        if self.memory_size > 4 * 1024 * 1024 * 1024 {
            warnings.push("Checkpoint memory exceeds 4GB - restore may be slow".to_string());
        }

        if self.is_incremental && self.parent_id.is_some() {
            warnings.push(
                "Incremental checkpoint - parent chain must be available for full restore"
                    .to_string(),
            );
        }

        RestoreValidation {
            can_restore: issues.is_empty(),
            issues,
            warnings,
        }
    }
}

/// Result of checkpoint restore validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreValidation {
    /// Whether the checkpoint can be restored.
    pub can_restore: bool,
    /// Blocking issues preventing restore.
    pub issues: Vec<String>,
    /// Non-blocking warnings.
    pub warnings: Vec<String>,
}

/// Manages checkpoint lifecycle: create, persist, restore, and GC.
pub struct CheckpointManager {
    /// Storage backend for persisting checkpoints.
    store: Arc<dyn SnapshotStore>,
    /// Node ID for this instance.
    node_id: String,
    /// Checkpoint metadata index.
    checkpoints: HashMap<SnapshotId, Checkpoint>,
    /// Clone timing statistics.
    clone_timings: CloneTimingStats,
}

/// Statistics for clone/restore timing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloneTimingStats {
    /// Number of clone operations.
    pub clone_count: u64,
    /// Total clone time.
    pub total_clone_time: Duration,
    /// Minimum clone time.
    pub min_clone_time: Option<Duration>,
    /// Maximum clone time.
    pub max_clone_time: Option<Duration>,
    /// Number of clones under 100μs target.
    pub clones_under_target: u64,
    /// Target clone time (100μs).
    pub target: Duration,
}

impl CloneTimingStats {
    fn new() -> Self {
        Self {
            target: Duration::from_micros(100),
            ..Default::default()
        }
    }

    /// Record a clone operation timing.
    pub fn record(&mut self, duration: Duration) {
        self.clone_count += 1;
        self.total_clone_time += duration;

        if duration < self.target {
            self.clones_under_target += 1;
        }

        self.min_clone_time = Some(
            self.min_clone_time.map_or(duration, |min| min.min(duration)),
        );
        self.max_clone_time = Some(
            self.max_clone_time.map_or(duration, |max| max.max(duration)),
        );
    }

    /// Average clone time.
    pub fn avg_clone_time(&self) -> Duration {
        if self.clone_count == 0 {
            Duration::ZERO
        } else {
            self.total_clone_time / self.clone_count as u32
        }
    }

    /// Percentage of clones meeting the <100μs target.
    pub fn target_hit_rate(&self) -> f64 {
        if self.clone_count == 0 {
            0.0
        } else {
            (self.clones_under_target as f64 / self.clone_count as f64) * 100.0
        }
    }
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    pub fn new(store: Arc<dyn SnapshotStore>, node_id: impl Into<String>) -> Self {
        Self {
            store,
            node_id: node_id.into(),
            checkpoints: HashMap::new(),
            clone_timings: CloneTimingStats::new(),
        }
    }

    /// Create and persist a checkpoint from a snapshot.
    pub async fn create_checkpoint(&mut self, snapshot: &Snapshot) -> Result<Checkpoint> {
        let checkpoint = Checkpoint::from_snapshot(snapshot, &self.node_id);

        // Persist to storage
        self.store.store(snapshot).await?;

        // Index locally
        self.checkpoints.insert(checkpoint.id, checkpoint.clone());

        tracing::info!(
            checkpoint_id = %checkpoint.id.0,
            module_hash = %checkpoint.module_hash.0,
            memory_size = checkpoint.memory_size,
            "Checkpoint created"
        );

        Ok(checkpoint)
    }

    /// Restore a snapshot from a checkpoint, recording timing.
    pub async fn restore_from_checkpoint(&mut self, id: SnapshotId) -> Result<(Snapshot, Duration)> {
        // Validate if we have metadata
        if let Some(checkpoint) = self.checkpoints.get(&id) {
            let validation = checkpoint.validate_for_restore();
            if !validation.can_restore {
                return Err(Error::Snapshot(format!(
                    "Cannot restore checkpoint: {}",
                    validation.issues.join("; ")
                )));
            }
        }

        let start = Instant::now();
        let snapshot = self.store.load(id).await?;
        let clone_time = start.elapsed();

        // Record timing
        self.clone_timings.record(clone_time);

        tracing::info!(
            checkpoint_id = %id.0,
            clone_time_us = clone_time.as_micros(),
            "Checkpoint restored"
        );

        Ok((snapshot, clone_time))
    }

    /// Get a checkpoint by ID.
    pub fn get_checkpoint(&self, id: &SnapshotId) -> Option<&Checkpoint> {
        self.checkpoints.get(id)
    }

    /// List all checkpoints.
    pub fn list_checkpoints(&self) -> Vec<&Checkpoint> {
        self.checkpoints.values().collect()
    }

    /// List checkpoints for a specific module.
    pub fn checkpoints_for_module(&self, module_hash: &ModuleHash) -> Vec<&Checkpoint> {
        self.checkpoints.values()
            .filter(|c| c.module_hash == *module_hash)
            .collect()
    }

    /// Get clone timing statistics.
    pub fn clone_timings(&self) -> &CloneTimingStats {
        &self.clone_timings
    }

    /// Get storage statistics.
    pub async fn storage_stats(&self) -> Result<StorageStats> {
        self.store.stats().await
    }

    /// Delete a checkpoint.
    pub async fn delete_checkpoint(&mut self, id: SnapshotId) -> Result<()> {
        self.store.delete(id).await?;
        self.checkpoints.remove(&id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxId;
    use crate::snapshot::storage::MemoryStore;

    fn make_test_snapshot() -> Snapshot {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test_module".to_string());
        let memory = vec![0u8; 65536];
        Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![])
    }

    #[test]
    fn test_checkpoint_from_snapshot() {
        let snapshot = make_test_snapshot();
        let checkpoint = Checkpoint::from_snapshot(&snapshot, "node-1");

        assert_eq!(checkpoint.id, snapshot.id);
        assert_eq!(checkpoint.origin_node, "node-1");
        assert!(checkpoint.is_compatible());
        assert!(!checkpoint.is_incremental);
    }

    #[test]
    fn test_checkpoint_compatibility() {
        let snapshot = make_test_snapshot();
        let checkpoint = Checkpoint::from_snapshot(&snapshot, "node-1");

        assert!(checkpoint.is_compatible());

        let validation = checkpoint.validate_for_restore();
        assert!(validation.can_restore);
        assert!(validation.issues.is_empty());
    }

    #[test]
    fn test_checkpoint_incompatible() {
        let snapshot = make_test_snapshot();
        let mut checkpoint = Checkpoint::from_snapshot(&snapshot, "node-1");
        checkpoint.compat_hash = "aarch64-linux-999.0.0".to_string();

        assert!(!checkpoint.is_compatible());
        let validation = checkpoint.validate_for_restore();
        assert!(!validation.can_restore);
    }

    #[test]
    fn test_clone_timing_stats() {
        let mut stats = CloneTimingStats::new();
        assert_eq!(stats.clone_count, 0);
        assert_eq!(stats.avg_clone_time(), Duration::ZERO);
        assert_eq!(stats.target_hit_rate(), 0.0);

        stats.record(Duration::from_micros(50));
        stats.record(Duration::from_micros(80));
        stats.record(Duration::from_micros(150));

        assert_eq!(stats.clone_count, 3);
        assert_eq!(stats.clones_under_target, 2);
        assert!((stats.target_hit_rate() - 66.66).abs() < 1.0);
        assert_eq!(stats.min_clone_time, Some(Duration::from_micros(50)));
        assert_eq!(stats.max_clone_time, Some(Duration::from_micros(150)));
    }

    #[tokio::test]
    async fn test_checkpoint_manager() {
        let store = Arc::new(MemoryStore::new());
        let mut manager = CheckpointManager::new(store, "test-node");

        let snapshot = make_test_snapshot();
        let checkpoint = manager.create_checkpoint(&snapshot).await.unwrap();

        assert!(manager.get_checkpoint(&checkpoint.id).is_some());
        assert_eq!(manager.list_checkpoints().len(), 1);

        let (restored, timing) = manager.restore_from_checkpoint(checkpoint.id).await.unwrap();
        assert_eq!(restored.id, snapshot.id);
        assert!(timing.as_nanos() > 0);

        assert_eq!(manager.clone_timings().clone_count, 1);
    }

    #[tokio::test]
    async fn test_checkpoint_delete() {
        let store = Arc::new(MemoryStore::new());
        let mut manager = CheckpointManager::new(store, "test-node");

        let snapshot = make_test_snapshot();
        let checkpoint = manager.create_checkpoint(&snapshot).await.unwrap();

        manager.delete_checkpoint(checkpoint.id).await.unwrap();
        assert!(manager.get_checkpoint(&checkpoint.id).is_none());
        assert!(manager.list_checkpoints().is_empty());
    }
}
