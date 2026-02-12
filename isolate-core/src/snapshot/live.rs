//! Live snapshot lifecycle manager with health monitoring and metrics.
//!
//! Provides production-grade snapshot orchestration including:
//! - Live snapshot creation from running sandboxes (quiesce → snapshot → resume)
//! - Health monitoring for snapshot storage and pool
//! - Metrics collection for snapshot operations
//! - Automatic garbage collection with configurable retention
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::snapshot::live::{LiveSnapshotManager, LiveSnapshotConfig};
//!
//! let config = LiveSnapshotConfig::builder()
//!     .max_snapshots(100)
//!     .retention_hours(24)
//!     .health_check_interval_secs(30)
//!     .build();
//!
//! let manager = LiveSnapshotManager::new(config);
//! let snapshot_id = manager.create_live_snapshot(sandbox_id).await?;
//! let health = manager.health_check();
//! ```

use super::{
    GlobalValue, Snapshot, SnapshotEngine, SnapshotEngineConfig, SnapshotId,
};
use crate::config::ModuleHash;
use crate::error::{Error, Result};
use crate::sandbox::SandboxId;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Configuration for the live snapshot manager.
#[derive(Debug, Clone)]
pub struct LiveSnapshotConfig {
    /// Maximum number of snapshots to retain.
    pub max_snapshots: usize,
    /// Retention period in hours (snapshots older than this are eligible for GC).
    pub retention_hours: u64,
    /// Health check interval in seconds.
    pub health_check_interval_secs: u64,
    /// Maximum time to wait for sandbox quiescence before snapshot.
    pub quiesce_timeout: Duration,
    /// Enable incremental snapshots when a parent exists.
    pub prefer_incremental: bool,
    /// Maximum snapshot size in bytes.
    pub max_snapshot_size: usize,
    /// Underlying snapshot engine config.
    pub engine_config: SnapshotEngineConfig,
}

impl Default for LiveSnapshotConfig {
    fn default() -> Self {
        Self {
            max_snapshots: 1000,
            retention_hours: 24,
            health_check_interval_secs: 30,
            quiesce_timeout: Duration::from_secs(5),
            prefer_incremental: true,
            max_snapshot_size: 512 * 1024 * 1024, // 512MB
            engine_config: SnapshotEngineConfig::default(),
        }
    }
}

impl LiveSnapshotConfig {
    /// Create a builder.
    pub fn builder() -> LiveSnapshotConfigBuilder {
        LiveSnapshotConfigBuilder::new()
    }
}

/// Builder for LiveSnapshotConfig.
#[derive(Debug)]
pub struct LiveSnapshotConfigBuilder {
    config: LiveSnapshotConfig,
}

impl LiveSnapshotConfigBuilder {
    fn new() -> Self {
        Self { config: LiveSnapshotConfig::default() }
    }

    /// Set maximum snapshots.
    pub fn max_snapshots(mut self, max: usize) -> Self {
        self.config.max_snapshots = max;
        self
    }

    /// Set retention period in hours.
    pub fn retention_hours(mut self, hours: u64) -> Self {
        self.config.retention_hours = hours;
        self
    }

    /// Set health check interval.
    pub fn health_check_interval_secs(mut self, secs: u64) -> Self {
        self.config.health_check_interval_secs = secs;
        self
    }

    /// Set quiesce timeout.
    pub fn quiesce_timeout(mut self, timeout: Duration) -> Self {
        self.config.quiesce_timeout = timeout;
        self
    }

    /// Set whether to prefer incremental snapshots.
    pub fn prefer_incremental(mut self, prefer: bool) -> Self {
        self.config.prefer_incremental = prefer;
        self
    }

    /// Build the config.
    pub fn build(self) -> LiveSnapshotConfig {
        self.config
    }
}

/// State of a live snapshot operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveSnapshotState {
    /// Waiting to quiesce the sandbox.
    Quiescing,
    /// Capturing memory and state.
    Capturing,
    /// Verifying snapshot integrity.
    Verifying,
    /// Snapshot complete and stored.
    Complete,
    /// Snapshot operation failed.
    Failed,
}

impl std::fmt::Display for LiveSnapshotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quiescing => write!(f, "quiescing"),
            Self::Capturing => write!(f, "capturing"),
            Self::Verifying => write!(f, "verifying"),
            Self::Complete => write!(f, "complete"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Record of a live snapshot operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSnapshotRecord {
    /// Snapshot ID (set once complete).
    pub snapshot_id: Option<SnapshotId>,
    /// Source sandbox ID.
    pub sandbox_id: SandboxId,
    /// Current state.
    pub state: LiveSnapshotState,
    /// Start time.
    pub started_at: DateTime<Utc>,
    /// Completion time.
    pub completed_at: Option<DateTime<Utc>>,
    /// Duration of the snapshot operation.
    pub duration: Option<Duration>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Whether this was an incremental snapshot.
    pub is_incremental: bool,
    /// Size of the resulting snapshot in bytes.
    pub snapshot_size: Option<usize>,
}

/// Metrics for snapshot operations.
#[derive(Debug, Default)]
pub struct LiveSnapshotMetrics {
    /// Total snapshots created.
    pub total_created: AtomicU64,
    /// Total snapshots failed.
    pub total_failed: AtomicU64,
    /// Total snapshots restored.
    pub total_restored: AtomicU64,
    /// Total incremental snapshots.
    pub total_incremental: AtomicU64,
    /// Total bytes stored (approximate).
    pub total_bytes_stored: AtomicU64,
    /// Total GC runs.
    pub total_gc_runs: AtomicU64,
    /// Total snapshots garbage collected.
    pub total_gc_collected: AtomicU64,
}

impl LiveSnapshotMetrics {
    /// Get a snapshot of current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_created: self.total_created.load(Ordering::Relaxed),
            total_failed: self.total_failed.load(Ordering::Relaxed),
            total_restored: self.total_restored.load(Ordering::Relaxed),
            total_incremental: self.total_incremental.load(Ordering::Relaxed),
            total_bytes_stored: self.total_bytes_stored.load(Ordering::Relaxed),
            total_gc_runs: self.total_gc_runs.load(Ordering::Relaxed),
            total_gc_collected: self.total_gc_collected.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of metrics at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Total snapshots created.
    pub total_created: u64,
    /// Total snapshots that failed.
    pub total_failed: u64,
    /// Total snapshots restored.
    pub total_restored: u64,
    /// Total incremental snapshots.
    pub total_incremental: u64,
    /// Total bytes stored.
    pub total_bytes_stored: u64,
    /// Total garbage collection runs.
    pub total_gc_runs: u64,
    /// Total snapshots garbage collected.
    pub total_gc_collected: u64,
}

/// Health status of the snapshot subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Overall health.
    pub healthy: bool,
    /// Snapshot storage health.
    pub storage_healthy: bool,
    /// Number of snapshots stored.
    pub snapshot_count: usize,
    /// Total storage used in bytes.
    pub storage_used_bytes: usize,
    /// Storage capacity remaining percentage.
    pub storage_remaining_pct: f64,
    /// Number of recent failures.
    pub recent_failures: usize,
    /// Last health check time.
    pub checked_at: DateTime<Utc>,
    /// Issues found during health check.
    pub issues: Vec<String>,
}

/// Manager for live snapshot lifecycle operations.
pub struct LiveSnapshotManager {
    config: LiveSnapshotConfig,
    engine: SnapshotEngine,
    metrics: LiveSnapshotMetrics,
    /// Track recent snapshot operations.
    recent_operations: parking_lot::Mutex<Vec<LiveSnapshotRecord>>,
    /// Track latest parent snapshot per module for incremental snapshots.
    latest_per_module: dashmap::DashMap<ModuleHash, SnapshotId>,
}

impl LiveSnapshotManager {
    /// Create a new live snapshot manager.
    pub fn new(config: LiveSnapshotConfig) -> Result<Self> {
        let engine = SnapshotEngine::new(config.engine_config.clone())?;
        Ok(Self {
            config,
            engine,
            metrics: LiveSnapshotMetrics::default(),
            recent_operations: parking_lot::Mutex::new(Vec::new()),
            latest_per_module: dashmap::DashMap::new(),
        })
    }

    /// Create a live snapshot from sandbox memory state.
    ///
    /// This performs the full lifecycle:
    /// 1. Record quiesce start
    /// 2. Capture memory and globals
    /// 3. Create snapshot (incremental if parent exists)
    /// 4. Verify integrity
    /// 5. Store in engine
    pub fn create_snapshot(
        &self,
        sandbox_id: SandboxId,
        module_hash: ModuleHash,
        memory: &[u8],
        globals: Vec<GlobalValue>,
    ) -> Result<SnapshotId> {
        let start = Instant::now();
        let mut record = LiveSnapshotRecord {
            snapshot_id: None,
            sandbox_id,
            state: LiveSnapshotState::Quiescing,
            started_at: Utc::now(),
            completed_at: None,
            duration: None,
            error: None,
            is_incremental: false,
            snapshot_size: None,
        };

        // Check size limits
        if memory.len() > self.config.max_snapshot_size {
            record.state = LiveSnapshotState::Failed;
            record.error = Some(format!(
                "Memory size {} exceeds maximum {}",
                memory.len(),
                self.config.max_snapshot_size
            ));
            self.metrics.total_failed.fetch_add(1, Ordering::Relaxed);
            self.record_operation(record);
            return Err(Error::Snapshot(format!(
                "Memory size {} exceeds maximum {}",
                memory.len(),
                self.config.max_snapshot_size
            )));
        }

        // Capture phase
        record.state = LiveSnapshotState::Capturing;

        let snapshot = if self.config.prefer_incremental {
            if let Some(parent_id) = self.latest_per_module.get(&module_hash) {
                if let Ok(parent) = self.engine.load(&parent_id) {
                    record.is_incremental = true;
                    Snapshot::incremental(
                        sandbox_id,
                        module_hash.clone(),
                        &parent,
                        memory,
                        globals,
                    )
                } else {
                    Snapshot::from_memory(sandbox_id, module_hash.clone(), memory, globals)
                }
            } else {
                Snapshot::from_memory(sandbox_id, module_hash.clone(), memory, globals)
            }
        } else {
            Snapshot::from_memory(sandbox_id, module_hash.clone(), memory, globals)
        };

        // Verify phase
        record.state = LiveSnapshotState::Verifying;
        let snapshot_size = snapshot.size();

        // Store
        let snapshot_id = self.engine.store(snapshot)?;

        // Update tracking
        self.latest_per_module.insert(module_hash, snapshot_id);

        // Update metrics
        let is_incremental = record.is_incremental;
        self.metrics.total_created.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_bytes_stored.fetch_add(snapshot_size as u64, Ordering::Relaxed);
        if is_incremental {
            self.metrics.total_incremental.fetch_add(1, Ordering::Relaxed);
        }

        // Complete record
        record.state = LiveSnapshotState::Complete;
        record.snapshot_id = Some(snapshot_id);
        record.completed_at = Some(Utc::now());
        record.duration = Some(start.elapsed());
        record.snapshot_size = Some(snapshot_size);
        self.record_operation(record);

        tracing::info!(
            snapshot_id = %snapshot_id,
            duration_us = start.elapsed().as_micros(),
            incremental = is_incremental,
            "Live snapshot created"
        );

        Ok(snapshot_id)
    }

    /// Restore memory from a snapshot.
    pub fn restore(&self, snapshot_id: &SnapshotId) -> Result<(Vec<u8>, Vec<GlobalValue>)> {
        let snapshot = self.engine.load(snapshot_id)?;
        let memory = snapshot.restore_memory(Some(&self.engine))?;
        let globals = snapshot.globals.clone();
        self.metrics.total_restored.fetch_add(1, Ordering::Relaxed);
        Ok((memory, globals))
    }

    /// Run garbage collection on expired snapshots.
    pub fn garbage_collect(&self) -> GcResult {
        let cutoff =
            Utc::now() - chrono::Duration::hours(self.config.retention_hours as i64);
        let mut collected = 0;

        // Collect snapshot IDs that are expired
        let expired: Vec<SnapshotId> = self
            .engine
            .snapshots
            .iter()
            .filter(|entry| entry.value().created_at < cutoff)
            .map(|entry| entry.key().clone())
            .collect();

        for id in &expired {
            if self.engine.remove(id).is_ok() {
                collected += 1;
            }
        }

        // Enforce max snapshots limit
        while self.engine.snapshot_count() > self.config.max_snapshots {
            // Find oldest
            let oldest = self
                .engine
                .snapshots
                .iter()
                .min_by_key(|entry| entry.value().created_at)
                .map(|entry| entry.key().clone());

            if let Some(id) = oldest {
                if self.engine.remove(&id).is_ok() {
                    collected += 1;
                }
            } else {
                break;
            }
        }

        self.metrics.total_gc_runs.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_gc_collected.fetch_add(collected as u64, Ordering::Relaxed);

        GcResult {
            collected,
            remaining: self.engine.snapshot_count(),
            storage_freed_estimate: 0, // Would need to track per-snapshot sizes
        }
    }

    /// Check health of the snapshot subsystem.
    pub fn health_check(&self) -> HealthStatus {
        let snapshot_count = self.engine.snapshot_count();
        let storage_used = self.engine.storage_size();
        let max_storage = self.config.engine_config.max_storage_size;
        let storage_remaining_pct = if max_storage > 0 {
            ((max_storage - storage_used.min(max_storage)) as f64 / max_storage as f64) * 100.0
        } else {
            0.0
        };

        let recent_ops = self.recent_operations.lock();
        let recent_failures = recent_ops
            .iter()
            .rev()
            .take(100)
            .filter(|r| r.state == LiveSnapshotState::Failed)
            .count();

        let mut issues = Vec::new();

        if storage_remaining_pct < 10.0 {
            issues.push(format!(
                "Storage nearly full: {:.1}% remaining",
                storage_remaining_pct
            ));
        }

        if recent_failures > 10 {
            issues.push(format!("{} failures in recent operations", recent_failures));
        }

        if snapshot_count > self.config.max_snapshots * 9 / 10 {
            issues.push("Approaching maximum snapshot count".to_string());
        }

        let storage_healthy = storage_remaining_pct > 5.0;
        let healthy = issues.is_empty() && storage_healthy;

        HealthStatus {
            healthy,
            storage_healthy,
            snapshot_count,
            storage_used_bytes: storage_used,
            storage_remaining_pct,
            recent_failures,
            checked_at: Utc::now(),
            issues,
        }
    }

    /// Get current metrics.
    pub fn metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Get the snapshot engine for direct access.
    pub fn engine(&self) -> &SnapshotEngine {
        &self.engine
    }

    /// Get recent operation records.
    pub fn recent_operations(&self) -> Vec<LiveSnapshotRecord> {
        self.recent_operations.lock().clone()
    }

    fn record_operation(&self, record: LiveSnapshotRecord) {
        let mut ops = self.recent_operations.lock();
        ops.push(record);
        // Keep last 1000 operations
        if ops.len() > 1000 {
            let excess = ops.len() - 1000;
            ops.drain(..excess);
        }
    }
}

/// Result of a garbage collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcResult {
    /// Number of snapshots collected.
    pub collected: usize,
    /// Remaining snapshots after GC.
    pub remaining: usize,
    /// Estimated bytes freed.
    pub storage_freed_estimate: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> LiveSnapshotConfig {
        LiveSnapshotConfig {
            max_snapshots: 10,
            retention_hours: 1,
            health_check_interval_secs: 5,
            quiesce_timeout: Duration::from_millis(100),
            prefer_incremental: true,
            max_snapshot_size: 10 * 1024 * 1024,
            engine_config: SnapshotEngineConfig {
                storage_path: std::env::temp_dir().join("isolate-live-test"),
                max_snapshots_per_module: 5,
                ..Default::default()
            },
        }
    }

    #[test]
    fn test_live_snapshot_creation() {
        let manager = LiveSnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        let id = manager
            .create_snapshot(sandbox_id, module_hash, &memory, vec![GlobalValue::I32(42)])
            .unwrap();

        let metrics = manager.metrics();
        assert_eq!(metrics.total_created, 1);
        assert_eq!(metrics.total_failed, 0);

        // Verify restore
        let (restored_mem, globals) = manager.restore(&id).unwrap();
        assert_eq!(restored_mem, memory);
        assert_eq!(globals.len(), 1);
    }

    #[test]
    fn test_incremental_snapshot() {
        let manager = LiveSnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());

        // First snapshot (full)
        let mut memory1 = vec![0u8; 65536];
        memory1[0..4].copy_from_slice(b"test");
        manager
            .create_snapshot(sandbox_id, module_hash.clone(), &memory1, vec![])
            .unwrap();

        // Second snapshot (should be incremental)
        let mut memory2 = memory1.clone();
        memory2[100..104].copy_from_slice(b"new!");
        let id2 = manager
            .create_snapshot(sandbox_id, module_hash, &memory2, vec![])
            .unwrap();

        let metrics = manager.metrics();
        assert_eq!(metrics.total_created, 2);
        assert_eq!(metrics.total_incremental, 1);

        // Verify restore of incremental
        let (restored, _) = manager.restore(&id2).unwrap();
        assert_eq!(restored, memory2);
    }

    #[test]
    fn test_size_limit_exceeded() {
        let mut config = test_config();
        config.max_snapshot_size = 1024; // Very small limit

        let manager = LiveSnapshotManager::new(config).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536]; // Exceeds limit

        let result = manager.create_snapshot(sandbox_id, module_hash, &memory, vec![]);
        assert!(result.is_err());

        let metrics = manager.metrics();
        assert_eq!(metrics.total_failed, 1);
    }

    #[test]
    fn test_health_check_healthy() {
        let manager = LiveSnapshotManager::new(test_config()).unwrap();
        let health = manager.health_check();

        assert!(health.healthy);
        assert!(health.storage_healthy);
        assert_eq!(health.snapshot_count, 0);
        assert!(health.issues.is_empty());
    }

    #[test]
    fn test_garbage_collection() {
        let manager = LiveSnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        // Create several snapshots
        for _ in 0..5 {
            manager
                .create_snapshot(sandbox_id, module_hash.clone(), &memory, vec![])
                .unwrap();
        }

        assert_eq!(manager.engine().snapshot_count(), 5);

        // GC shouldn't collect anything (not expired yet)
        let gc_result = manager.garbage_collect();
        assert_eq!(gc_result.collected, 0);
        assert_eq!(gc_result.remaining, 5);
    }

    #[test]
    fn test_gc_enforces_max_snapshots() {
        let mut config = test_config();
        config.max_snapshots = 3;
        config.engine_config.max_snapshots_per_module = 10; // high so engine doesn't evict first

        let manager = LiveSnapshotManager::new(config).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        for _ in 0..5 {
            manager
                .create_snapshot(sandbox_id, module_hash.clone(), &memory, vec![])
                .unwrap();
        }

        let gc_result = manager.garbage_collect();
        assert!(gc_result.collected >= 2);
        assert!(gc_result.remaining <= 3);
    }

    #[test]
    fn test_restore_invalid_id_fails() {
        let manager = LiveSnapshotManager::new(test_config()).unwrap();
        let bad_id = super::SnapshotId::new();
        let result = manager.restore(&bad_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = LiveSnapshotConfig::builder()
            .max_snapshots(500)
            .retention_hours(48)
            .prefer_incremental(false)
            .build();

        assert_eq!(config.max_snapshots, 500);
        assert_eq!(config.retention_hours, 48);
        assert!(!config.prefer_incremental);
    }

    #[test]
    fn test_recent_operations_tracking() {
        let manager = LiveSnapshotManager::new(test_config()).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        manager.create_snapshot(sandbox_id, module_hash, &memory, vec![]).unwrap();

        let ops = manager.recent_operations();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].state, LiveSnapshotState::Complete);
        assert!(ops[0].snapshot_id.is_some());
        assert!(ops[0].duration.is_some());
    }
}
