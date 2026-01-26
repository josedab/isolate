//! Snapshot lifecycle manager integrating with the Sandbox API.
//!
//! Provides a high-level interface for creating, managing, and restoring
//! sandbox snapshots, bridging the snapshot engine with the sandbox runtime.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::snapshot::manager::{SnapshotManager, SnapshotManagerConfig};
//!
//! let manager = SnapshotManager::new(SnapshotManagerConfig::default())?;
//!
//! // Take a snapshot of an initialized sandbox
//! let snapshot_id = manager.capture(&sandbox, "after-init").await?;
//!
//! // Restore a new sandbox from the snapshot
//! let restored = manager.restore(&snapshot_id, config).await?;
//! ```

use crate::config::ModuleHash;
use crate::error::Result;
use crate::sandbox::SandboxId;

use super::{
    GlobalValue, Snapshot, SnapshotEngine, SnapshotEngineConfig, SnapshotId, SnapshotMetadata,
};

use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for the snapshot manager.
#[derive(Debug, Clone)]
pub struct SnapshotManagerConfig {
    /// Underlying snapshot engine config.
    pub engine: SnapshotEngineConfig,
    /// Whether to verify checksums on restore.
    pub verify_on_restore: bool,
    /// Maximum age for snapshots before auto-cleanup.
    pub max_snapshot_age: Option<Duration>,
    /// Enable auto-labeling of snapshots.
    pub auto_label: bool,
}

impl Default for SnapshotManagerConfig {
    fn default() -> Self {
        Self {
            engine: SnapshotEngineConfig::default(),
            verify_on_restore: true,
            max_snapshot_age: Some(Duration::from_secs(3600)), // 1 hour
            auto_label: true,
        }
    }
}

/// Statistics for snapshot operations.
#[derive(Debug, Clone, Default)]
pub struct SnapshotManagerStats {
    /// Total snapshots captured.
    pub captures: u64,
    /// Total snapshots restored.
    pub restores: u64,
    /// Total snapshots evicted.
    pub evictions: u64,
    /// Average capture time.
    pub avg_capture_time: Duration,
    /// Average restore time.
    pub avg_restore_time: Duration,
    /// Total bytes saved through compression.
    pub bytes_saved: u64,
}

/// High-level snapshot lifecycle manager.
pub struct SnapshotManager {
    config: SnapshotManagerConfig,
    engine: Arc<SnapshotEngine>,
    capture_count: AtomicU64,
    restore_count: AtomicU64,
    eviction_count: AtomicU64,
    total_capture_ns: AtomicU64,
    total_restore_ns: AtomicU64,
    total_bytes_saved: AtomicU64,
}

impl SnapshotManager {
    /// Create a new snapshot manager.
    pub fn new(config: SnapshotManagerConfig) -> Result<Self> {
        let engine = Arc::new(SnapshotEngine::new(config.engine.clone())?);
        Ok(Self {
            config,
            engine,
            capture_count: AtomicU64::new(0),
            restore_count: AtomicU64::new(0),
            eviction_count: AtomicU64::new(0),
            total_capture_ns: AtomicU64::new(0),
            total_restore_ns: AtomicU64::new(0),
            total_bytes_saved: AtomicU64::new(0),
        })
    }

    /// Capture a snapshot from the given sandbox memory state.
    ///
    /// This captures the memory pages, globals, and metadata into a compact
    /// snapshot representation with zero-page compression.
    pub fn capture(
        &self,
        sandbox_id: SandboxId,
        module_hash: &ModuleHash,
        memory: &[u8],
        globals: Vec<GlobalValue>,
        label: Option<&str>,
    ) -> Result<SnapshotId> {
        let start = Instant::now();

        let mut snapshot = Snapshot::from_memory(sandbox_id, module_hash.clone(), memory, globals);

        // Auto-label if configured
        if let Some(lbl) = label {
            snapshot = snapshot.with_label(lbl);
        } else if self.config.auto_label {
            let count = self.capture_count.load(Ordering::Relaxed);
            snapshot = snapshot.with_label(format!("auto-{}", count));
        }

        let original_size = memory.len() as u64;
        let snapshot_size = snapshot.size() as u64;
        let bytes_saved = original_size.saturating_sub(snapshot_size);

        let id = self.engine.store(snapshot)?;

        let elapsed = start.elapsed();
        self.capture_count.fetch_add(1, Ordering::Relaxed);
        self.total_capture_ns.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
        self.total_bytes_saved.fetch_add(bytes_saved, Ordering::Relaxed);

        tracing::info!(
            snapshot_id = %id,
            sandbox_id = %sandbox_id,
            capture_time_us = elapsed.as_micros() as u64,
            compression_ratio = format!("{:.2}%", (snapshot_size as f64 / original_size.max(1) as f64) * 100.0),
            "Snapshot captured"
        );

        Ok(id)
    }

    /// Capture an incremental snapshot based on a parent.
    pub fn capture_incremental(
        &self,
        sandbox_id: SandboxId,
        module_hash: &ModuleHash,
        parent_id: &SnapshotId,
        memory: &[u8],
        globals: Vec<GlobalValue>,
        label: Option<&str>,
    ) -> Result<SnapshotId> {
        let start = Instant::now();

        let parent = self.engine.load(parent_id)?;
        let mut snapshot =
            Snapshot::incremental(sandbox_id, module_hash.clone(), &parent, memory, globals);

        if let Some(lbl) = label {
            snapshot = snapshot.with_label(lbl);
        }

        let id = self.engine.store(snapshot)?;

        let elapsed = start.elapsed();
        self.capture_count.fetch_add(1, Ordering::Relaxed);
        self.total_capture_ns.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);

        tracing::info!(
            snapshot_id = %id,
            parent_id = %parent_id,
            capture_time_us = elapsed.as_micros() as u64,
            "Incremental snapshot captured"
        );

        Ok(id)
    }

    /// Restore memory state from a snapshot.
    ///
    /// Returns the restored memory bytes, globals, and metadata.
    pub fn restore(&self, snapshot_id: &SnapshotId) -> Result<RestoredState> {
        let start = Instant::now();

        let snapshot = self.engine.load(snapshot_id)?;
        let memory = snapshot.restore_memory(Some(&self.engine))?;

        let restored = RestoredState {
            memory,
            globals: snapshot.globals.clone(),
            module_hash: snapshot.module_hash.clone(),
            fuel_remaining: snapshot.fuel_remaining,
            metadata: snapshot.metadata.clone(),
            original_sandbox_id: snapshot.sandbox_id,
            snapshot_created_at: snapshot.created_at,
        };

        let elapsed = start.elapsed();
        self.restore_count.fetch_add(1, Ordering::Relaxed);
        self.total_restore_ns.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);

        tracing::info!(
            snapshot_id = %snapshot_id,
            restore_time_us = elapsed.as_micros() as u64,
            memory_size = restored.memory.len(),
            "Snapshot restored"
        );

        Ok(restored)
    }

    /// List snapshots for a given module.
    pub fn list_for_module(&self, module_hash: &ModuleHash) -> Vec<SnapshotInfo> {
        self.engine
            .get_for_module(module_hash)
            .iter()
            .filter_map(|id| {
                self.engine.load(id).ok().map(|s| SnapshotInfo {
                    id: s.id,
                    sandbox_id: s.sandbox_id,
                    module_hash: s.module_hash.clone(),
                    created_at: s.created_at,
                    size: s.size(),
                    memory_size: s.memory_size,
                    compression_ratio: s.compression_ratio(),
                    is_incremental: s.is_incremental(),
                    label: s.metadata.label.clone(),
                    tags: s.metadata.tags.clone(),
                })
            })
            .collect()
    }

    /// Delete a snapshot.
    pub fn delete(&self, snapshot_id: &SnapshotId) -> Result<()> {
        self.engine.remove(snapshot_id)?;
        self.eviction_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Run garbage collection, removing snapshots older than the configured max age.
    pub fn gc(&self) -> GcResult {
        let mut removed = 0;
        let mut bytes_freed = 0;

        if let Some(max_age) = self.config.max_snapshot_age {
            let cutoff = Utc::now() - chrono::Duration::from_std(max_age).unwrap_or_default();
            let mut to_remove = Vec::new();

            // Collect IDs to remove (can't remove during iteration with DashMap)
            for entry in self.engine.snapshots.iter() {
                if entry.created_at < cutoff {
                    to_remove.push(entry.id);
                    bytes_freed += entry.size();
                }
            }

            for id in to_remove {
                if self.engine.remove(&id).is_ok() {
                    removed += 1;
                }
            }

            self.eviction_count.fetch_add(removed as u64, Ordering::Relaxed);
        }

        GcResult { snapshots_removed: removed, bytes_freed }
    }

    /// Get snapshot manager statistics.
    pub fn stats(&self) -> SnapshotManagerStats {
        let captures = self.capture_count.load(Ordering::Relaxed);
        let restores = self.restore_count.load(Ordering::Relaxed);
        let total_capture_ns = self.total_capture_ns.load(Ordering::Relaxed);
        let total_restore_ns = self.total_restore_ns.load(Ordering::Relaxed);

        SnapshotManagerStats {
            captures,
            restores,
            evictions: self.eviction_count.load(Ordering::Relaxed),
            avg_capture_time: if captures > 0 {
                Duration::from_nanos(total_capture_ns / captures)
            } else {
                Duration::ZERO
            },
            avg_restore_time: if restores > 0 {
                Duration::from_nanos(total_restore_ns / restores)
            } else {
                Duration::ZERO
            },
            bytes_saved: self.total_bytes_saved.load(Ordering::Relaxed),
        }
    }

    /// Get the underlying snapshot engine (for direct access).
    pub fn engine(&self) -> &Arc<SnapshotEngine> {
        &self.engine
    }

    /// Get the total number of stored snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.engine.snapshot_count()
    }
}

/// Restored state from a snapshot.
#[derive(Debug, Clone)]
pub struct RestoredState {
    /// Restored linear memory.
    pub memory: Vec<u8>,
    /// Restored global values.
    pub globals: Vec<GlobalValue>,
    /// Module hash for verification.
    pub module_hash: ModuleHash,
    /// Fuel remaining at snapshot time.
    pub fuel_remaining: Option<u64>,
    /// Snapshot metadata.
    pub metadata: SnapshotMetadata,
    /// The sandbox ID that created this snapshot.
    pub original_sandbox_id: SandboxId,
    /// When the snapshot was created.
    pub snapshot_created_at: DateTime<Utc>,
}

/// Information about a stored snapshot (without the full memory data).
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// Snapshot ID.
    pub id: SnapshotId,
    /// Original sandbox ID.
    pub sandbox_id: SandboxId,
    /// Module hash.
    pub module_hash: ModuleHash,
    /// When the snapshot was created.
    pub created_at: DateTime<Utc>,
    /// Stored size in bytes.
    pub size: usize,
    /// Original memory size in bytes.
    pub memory_size: usize,
    /// Compression ratio.
    pub compression_ratio: f64,
    /// Whether this is an incremental snapshot.
    pub is_incremental: bool,
    /// Human-readable label.
    pub label: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
}

/// Result of garbage collection.
#[derive(Debug, Clone)]
pub struct GcResult {
    /// Number of snapshots removed.
    pub snapshots_removed: usize,
    /// Bytes freed.
    pub bytes_freed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SnapshotManagerConfig {
        SnapshotManagerConfig {
            engine: SnapshotEngineConfig {
                storage_path: std::env::temp_dir().join("isolate-test-manager"),
                max_snapshots_per_module: 5,
                ..Default::default()
            },
            max_snapshot_age: Some(Duration::from_secs(3600)),
            ..Default::default()
        }
    }

    #[test]
    fn test_manager_creation() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        assert_eq!(manager.snapshot_count(), 0);
    }

    #[test]
    fn test_capture_and_restore() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test-module".to_string());

        // Create test memory
        let mut memory = vec![0u8; 128 * 1024];
        memory[0..5].copy_from_slice(b"hello");
        memory[65536..65541].copy_from_slice(b"world");

        // Capture
        let snapshot_id = manager
            .capture(
                sandbox_id,
                &module_hash,
                &memory,
                vec![GlobalValue::I32(42)],
                Some("test-snapshot"),
            )
            .unwrap();

        assert_eq!(manager.snapshot_count(), 1);

        // Restore
        let restored = manager.restore(&snapshot_id).unwrap();
        assert_eq!(restored.memory, memory);
        assert_eq!(restored.module_hash, module_hash);
        assert_eq!(restored.globals.len(), 1);
    }

    #[test]
    fn test_incremental_capture() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test-module".to_string());

        // Create initial state
        let mut memory1 = vec![0u8; 128 * 1024];
        memory1[0..5].copy_from_slice(b"hello");

        let parent_id = manager.capture(sandbox_id, &module_hash, &memory1, vec![], None).unwrap();

        // Modify and capture incremental
        let mut memory2 = memory1.clone();
        memory2[65536..65541].copy_from_slice(b"world");

        let child_id = manager
            .capture_incremental(sandbox_id, &module_hash, &parent_id, &memory2, vec![], None)
            .unwrap();

        assert_eq!(manager.snapshot_count(), 2);

        // Restore child
        let restored = manager.restore(&child_id).unwrap();
        assert_eq!(restored.memory, memory2);
    }

    #[test]
    fn test_list_for_module() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let hash1 = ModuleHash("module-1".to_string());
        let hash2 = ModuleHash("module-2".to_string());

        let memory = vec![0u8; 65536];

        manager.capture(sandbox_id, &hash1, &memory, vec![], Some("snap-1")).unwrap();
        manager.capture(sandbox_id, &hash1, &memory, vec![], Some("snap-2")).unwrap();
        manager.capture(sandbox_id, &hash2, &memory, vec![], Some("snap-3")).unwrap();

        let list1 = manager.list_for_module(&hash1);
        assert_eq!(list1.len(), 2);
        assert!(list1[0].label.as_deref() == Some("snap-1"));

        let list2 = manager.list_for_module(&hash2);
        assert_eq!(list2.len(), 1);
    }

    #[test]
    fn test_delete_snapshot() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        let id = manager.capture(sandbox_id, &module_hash, &memory, vec![], None).unwrap();
        assert_eq!(manager.snapshot_count(), 1);

        manager.delete(&id).unwrap();
        assert_eq!(manager.snapshot_count(), 0);
        assert!(manager.restore(&id).is_err());
    }

    #[test]
    fn test_stats() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());

        let mut memory = vec![0u8; 65536];
        memory[0..5].copy_from_slice(b"hello");

        let id = manager.capture(sandbox_id, &module_hash, &memory, vec![], None).unwrap();
        manager.restore(&id).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.captures, 1);
        assert_eq!(stats.restores, 1);
        assert!(stats.avg_capture_time > Duration::ZERO);
        assert!(stats.avg_restore_time > Duration::ZERO);
    }

    #[test]
    fn test_gc_no_expired() {
        let manager = SnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        manager.capture(sandbox_id, &module_hash, &memory, vec![], None).unwrap();

        // No snapshots should be expired (just created)
        let result = manager.gc();
        assert_eq!(result.snapshots_removed, 0);
        assert_eq!(manager.snapshot_count(), 1);
    }

    #[test]
    fn test_auto_label() {
        let mut config = test_config();
        config.auto_label = true;
        let manager = SnapshotManager::new(config).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        let _id = manager.capture(sandbox_id, &module_hash, &memory, vec![], None).unwrap();

        let list = manager.list_for_module(&module_hash);
        assert_eq!(list.len(), 1);
        assert!(list[0].label.as_deref().unwrap().starts_with("auto-"));
    }
}
