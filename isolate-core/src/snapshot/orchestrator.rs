//! Snapshot orchestrator for end-to-end snapshot lifecycle management.
//!
//! Coordinates capture, storage, indexing, and restoration of sandbox snapshots,
//! with automatic garbage collection and warm pool integration.
//!
//! ```rust,ignore
//! use isolate_core::snapshot::orchestrator::{
//!     SnapshotOrchestrator, OrchestratorConfig, SnapshotRef,
//! };
//!
//! let config = OrchestratorConfig::default();
//! let mut orchestrator = SnapshotOrchestrator::new(config);
//!
//! // Capture a snapshot
//! let snap_ref = orchestrator.capture("sandbox-1", "module-hash-abc", &state_bytes)?;
//!
//! // Restore from snapshot
//! let restored = orchestrator.restore(&snap_ref)?;
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

/// Orchestrator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Maximum snapshots to keep per module.
    pub max_snapshots_per_module: usize,
    /// Maximum total snapshot storage bytes.
    pub max_total_bytes: usize,
    /// Snapshot TTL (time to live).
    pub snapshot_ttl: Duration,
    /// Enable compression for stored snapshots.
    pub enable_compression: bool,
    /// Enable incremental snapshots.
    pub enable_incremental: bool,
    /// Garbage collection interval.
    pub gc_interval: Duration,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_snapshots_per_module: 5,
            max_total_bytes: 512 * 1024 * 1024, // 512 MB
            snapshot_ttl: Duration::from_secs(3600),
            enable_compression: true,
            enable_incremental: true,
            gc_interval: Duration::from_secs(60),
        }
    }
}

/// Reference to a stored snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotRef {
    /// Unique snapshot ID.
    pub id: String,
    /// Module hash this snapshot belongs to.
    pub module_hash: String,
    /// Snapshot version (incremental sequence).
    pub version: u64,
}

impl SnapshotRef {
    fn new(module_hash: &str, version: u64) -> Self {
        Self {
            id: format!("snap-{}-v{}", &module_hash[..12.min(module_hash.len())], version),
            module_hash: module_hash.to_string(),
            version,
        }
    }
}

impl std::fmt::Display for SnapshotRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@v{}", self.module_hash, self.version)
    }
}

/// Metadata about a stored snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// Snapshot reference.
    pub snapshot_ref: SnapshotRef,
    /// Snapshot size in bytes.
    pub size_bytes: usize,
    /// Whether it's compressed.
    pub compressed: bool,
    /// Whether it's incremental (delta from parent).
    pub incremental: bool,
    /// Parent snapshot (for incremental).
    pub parent: Option<SnapshotRef>,
    /// Creation timestamp.
    pub created_at: SystemTime,
    /// Number of times this snapshot was restored.
    pub restore_count: u64,
    /// Source sandbox ID.
    pub source_sandbox_id: String,
    /// Checksum for integrity verification.
    pub checksum: String,
}

/// Orchestrator error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    /// Snapshot not found.
    NotFound(String),
    /// Storage capacity exceeded.
    StorageFull { used: usize, max: usize },
    /// Integrity check failed.
    IntegrityError(String),
    /// State too large.
    StateTooLarge { size: usize, max: usize },
    /// Module has too many snapshots.
    TooManySnapshots { module: String, count: usize, max: usize },
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Snapshot not found: {}", id),
            Self::StorageFull { used, max } => {
                write!(f, "Storage full: {} / {} bytes", used, max)
            }
            Self::IntegrityError(msg) => write!(f, "Integrity error: {}", msg),
            Self::StateTooLarge { size, max } => {
                write!(f, "State too large: {} bytes (max: {})", size, max)
            }
            Self::TooManySnapshots { module, count, max } => {
                write!(f, "Module '{}' has {} snapshots (max: {})", module, count, max)
            }
        }
    }
}

/// Stored snapshot data (bytes + metadata).
struct StoredSnapshot {
    meta: SnapshotMeta,
    data: Vec<u8>,
}

/// Restored snapshot data.
#[derive(Debug, Clone)]
pub struct RestoredState {
    /// The raw state bytes.
    pub state: Vec<u8>,
    /// Snapshot reference this was restored from.
    pub from_snapshot: SnapshotRef,
    /// Restore duration.
    pub restore_duration: Duration,
    /// Whether decompression was needed.
    pub was_compressed: bool,
}

/// Snapshot lifecycle orchestrator.
pub struct SnapshotOrchestrator {
    config: OrchestratorConfig,
    /// Stored snapshots indexed by ref.
    snapshots: HashMap<SnapshotRef, StoredSnapshot>,
    /// Module -> list of snapshot refs (ordered by version).
    module_index: HashMap<String, Vec<SnapshotRef>>,
    /// Version counters per module.
    version_counters: HashMap<String, u64>,
    /// Total storage used.
    total_bytes: usize,
    /// Last GC run.
    last_gc: Option<Instant>,
    /// Statistics.
    stats: OrchestratorStats,
}

/// Orchestrator statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestratorStats {
    /// Total snapshots captured.
    pub captures: u64,
    /// Total snapshots restored.
    pub restores: u64,
    /// Total GC runs.
    pub gc_runs: u64,
    /// Total bytes reclaimed by GC.
    pub bytes_reclaimed: u64,
    /// Total snapshots evicted.
    pub evictions: u64,
}

impl SnapshotOrchestrator {
    /// Create a new orchestrator.
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            config,
            snapshots: HashMap::new(),
            module_index: HashMap::new(),
            version_counters: HashMap::new(),
            total_bytes: 0,
            last_gc: None,
            stats: OrchestratorStats::default(),
        }
    }

    /// Capture a snapshot of sandbox state.
    pub fn capture(
        &mut self,
        sandbox_id: &str,
        module_hash: &str,
        state: &[u8],
    ) -> Result<SnapshotRef, OrchestratorError> {
        // Check storage capacity
        if self.total_bytes + state.len() > self.config.max_total_bytes {
            // Try GC first
            self.gc();
            if self.total_bytes + state.len() > self.config.max_total_bytes {
                return Err(OrchestratorError::StorageFull {
                    used: self.total_bytes,
                    max: self.config.max_total_bytes,
                });
            }
        }

        // Check per-module limit
        let module_snaps = self.module_index.entry(module_hash.to_string()).or_default();
        if module_snaps.len() >= self.config.max_snapshots_per_module {
            // Evict oldest
            if let Some(oldest) = module_snaps.first().cloned() {
                self.evict(&oldest);
            }
        }

        // Compute checksum
        let checksum = compute_checksum(state);

        // Get next version
        let version = {
            let counter = self.version_counters.entry(module_hash.to_string()).or_insert(0);
            *counter += 1;
            *counter
        };

        let snap_ref = SnapshotRef::new(module_hash, version);

        // Determine if incremental
        let module_snaps = self.module_index.entry(module_hash.to_string()).or_default();
        let parent = if self.config.enable_incremental && !module_snaps.is_empty() {
            module_snaps.last().cloned()
        } else {
            None
        };

        // Optionally compress
        let stored_data =
            if self.config.enable_compression { simple_compress(state) } else { state.to_vec() };

        let meta = SnapshotMeta {
            snapshot_ref: snap_ref.clone(),
            size_bytes: stored_data.len(),
            compressed: self.config.enable_compression,
            incremental: parent.is_some(),
            parent,
            created_at: SystemTime::now(),
            restore_count: 0,
            source_sandbox_id: sandbox_id.to_string(),
            checksum,
        };

        self.total_bytes += stored_data.len();
        self.snapshots.insert(snap_ref.clone(), StoredSnapshot { meta, data: stored_data });

        let module_snaps = self.module_index.entry(module_hash.to_string()).or_default();
        module_snaps.push(snap_ref.clone());

        self.stats.captures += 1;

        Ok(snap_ref)
    }

    /// Restore from a snapshot.
    pub fn restore(&mut self, snap_ref: &SnapshotRef) -> Result<RestoredState, OrchestratorError> {
        let start = Instant::now();

        let stored = self
            .snapshots
            .get_mut(snap_ref)
            .ok_or_else(|| OrchestratorError::NotFound(snap_ref.id.clone()))?;

        stored.meta.restore_count += 1;

        let state = if stored.meta.compressed {
            simple_decompress(&stored.data)
        } else {
            stored.data.clone()
        };

        // Verify integrity
        let checksum = compute_checksum(&state);
        if checksum != stored.meta.checksum {
            return Err(OrchestratorError::IntegrityError(format!(
                "Checksum mismatch: expected {}, got {}",
                stored.meta.checksum, checksum
            )));
        }

        self.stats.restores += 1;

        Ok(RestoredState {
            state,
            from_snapshot: snap_ref.clone(),
            restore_duration: start.elapsed(),
            was_compressed: stored.meta.compressed,
        })
    }

    /// Get latest snapshot for a module.
    pub fn latest(&self, module_hash: &str) -> Option<&SnapshotRef> {
        self.module_index.get(module_hash).and_then(|refs| refs.last())
    }

    /// List all snapshots for a module.
    pub fn list_module_snapshots(&self, module_hash: &str) -> Vec<&SnapshotMeta> {
        self.module_index
            .get(module_hash)
            .map(|refs| {
                refs.iter().filter_map(|r| self.snapshots.get(r).map(|s| &s.meta)).collect()
            })
            .unwrap_or_default()
    }

    /// Get snapshot metadata.
    pub fn get_meta(&self, snap_ref: &SnapshotRef) -> Option<&SnapshotMeta> {
        self.snapshots.get(snap_ref).map(|s| &s.meta)
    }

    /// Evict a specific snapshot.
    fn evict(&mut self, snap_ref: &SnapshotRef) {
        if let Some(stored) = self.snapshots.remove(snap_ref) {
            self.total_bytes = self.total_bytes.saturating_sub(stored.meta.size_bytes);
            self.stats.evictions += 1;
            self.stats.bytes_reclaimed += stored.meta.size_bytes as u64;

            // Remove from module index
            if let Some(refs) = self.module_index.get_mut(&snap_ref.module_hash) {
                refs.retain(|r| r != snap_ref);
            }
        }
    }

    /// Run garbage collection.
    pub fn gc(&mut self) -> u64 {
        let now = SystemTime::now();
        let mut reclaimed = 0u64;

        // Find expired snapshots
        let expired: Vec<SnapshotRef> = self
            .snapshots
            .iter()
            .filter(|(_, stored)| {
                now.duration_since(stored.meta.created_at).unwrap_or(Duration::ZERO)
                    > self.config.snapshot_ttl
            })
            .map(|(r, _)| r.clone())
            .collect();

        for snap_ref in expired {
            if let Some(stored) = self.snapshots.remove(&snap_ref) {
                reclaimed += stored.meta.size_bytes as u64;
                self.total_bytes = self.total_bytes.saturating_sub(stored.meta.size_bytes);

                if let Some(refs) = self.module_index.get_mut(&snap_ref.module_hash) {
                    refs.retain(|r| r != &snap_ref);
                }
            }
        }

        self.stats.gc_runs += 1;
        self.stats.bytes_reclaimed += reclaimed;
        self.last_gc = Some(Instant::now());

        reclaimed
    }

    /// Delete all snapshots for a module.
    pub fn delete_module(&mut self, module_hash: &str) -> usize {
        let refs = self.module_index.remove(module_hash).unwrap_or_default();
        let count = refs.len();

        for snap_ref in &refs {
            if let Some(stored) = self.snapshots.remove(snap_ref) {
                self.total_bytes = self.total_bytes.saturating_sub(stored.meta.size_bytes);
            }
        }

        count
    }

    /// Get orchestrator statistics.
    pub fn stats(&self) -> &OrchestratorStats {
        &self.stats
    }

    /// Get storage utilization.
    pub fn utilization(&self) -> f64 {
        if self.config.max_total_bytes > 0 {
            self.total_bytes as f64 / self.config.max_total_bytes as f64
        } else {
            0.0
        }
    }

    /// Total number of stored snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Total bytes used.
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

/// Simple RLE compression.
fn simple_compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];
        let mut count = 1u8;

        while (i + count as usize) < data.len() && data[i + count as usize] == byte && (count < 255)
        {
            count += 1;
        }

        if count >= 3 || byte == 0xFF {
            result.push(0xFF); // escape
            result.push(count);
            result.push(byte);
        } else {
            for _ in 0..count {
                result.push(byte);
            }
        }

        i += count as usize;
    }

    result
}

/// Simple RLE decompression.
fn simple_decompress(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        if data[i] == 0xFF && i + 2 < data.len() {
            let count = data[i + 1] as usize;
            let byte = data[i + 2];
            result.extend(std::iter::repeat(byte).take(count));
            i += 3;
        } else {
            result.push(data[i]);
            i += 1;
        }
    }

    result
}

/// Compute SHA-256 checksum.
fn compute_checksum(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> OrchestratorConfig {
        OrchestratorConfig {
            max_snapshots_per_module: 3,
            max_total_bytes: 1024 * 1024,
            snapshot_ttl: Duration::from_secs(3600),
            enable_compression: false, // Disable for deterministic tests
            enable_incremental: true,
            gc_interval: Duration::from_secs(60),
        }
    }

    #[test]
    fn test_capture_and_restore() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        let state = b"sandbox state data here";

        let snap_ref = orch.capture("sb-1", "module-abc", state).unwrap();
        assert_eq!(snap_ref.module_hash, "module-abc");
        assert_eq!(snap_ref.version, 1);

        let restored = orch.restore(&snap_ref).unwrap();
        assert_eq!(restored.state, state);
    }

    #[test]
    fn test_versioning() {
        let mut orch = SnapshotOrchestrator::new(test_config());

        let r1 = orch.capture("sb-1", "mod-a", b"state1").unwrap();
        let r2 = orch.capture("sb-1", "mod-a", b"state2").unwrap();

        assert_eq!(r1.version, 1);
        assert_eq!(r2.version, 2);
    }

    #[test]
    fn test_latest() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        orch.capture("sb-1", "mod-a", b"state1").unwrap();
        orch.capture("sb-1", "mod-a", b"state2").unwrap();

        let latest = orch.latest("mod-a").unwrap();
        assert_eq!(latest.version, 2);
    }

    #[test]
    fn test_per_module_eviction() {
        let mut orch = SnapshotOrchestrator::new(test_config()); // max 3 per module
        orch.capture("sb-1", "mod-a", b"state1").unwrap();
        orch.capture("sb-1", "mod-a", b"state2").unwrap();
        orch.capture("sb-1", "mod-a", b"state3").unwrap();
        orch.capture("sb-1", "mod-a", b"state4").unwrap(); // Should evict v1

        let snaps = orch.list_module_snapshots("mod-a");
        assert_eq!(snaps.len(), 3);
    }

    #[test]
    fn test_not_found() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        let fake = SnapshotRef::new("nonexistent", 99);
        assert!(matches!(orch.restore(&fake), Err(OrchestratorError::NotFound(_))));
    }

    #[test]
    fn test_storage_full() {
        let config = OrchestratorConfig {
            max_total_bytes: 10, // Very small
            ..test_config()
        };
        let mut orch = SnapshotOrchestrator::new(config);

        let result = orch.capture("sb-1", "mod-a", &[0u8; 100]);
        assert!(matches!(result, Err(OrchestratorError::StorageFull { .. })));
    }

    #[test]
    fn test_delete_module() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        orch.capture("sb-1", "mod-a", b"state1").unwrap();
        orch.capture("sb-1", "mod-a", b"state2").unwrap();

        let deleted = orch.delete_module("mod-a");
        assert_eq!(deleted, 2);
        assert_eq!(orch.snapshot_count(), 0);
    }

    #[test]
    fn test_stats() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        let r = orch.capture("sb-1", "mod-a", b"state").unwrap();
        orch.restore(&r).unwrap();

        let stats = orch.stats();
        assert_eq!(stats.captures, 1);
        assert_eq!(stats.restores, 1);
    }

    #[test]
    fn test_utilization() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        assert!((orch.utilization() - 0.0).abs() < f64::EPSILON);

        orch.capture("sb-1", "mod-a", b"state data").unwrap();
        assert!(orch.utilization() > 0.0);
    }

    #[test]
    fn test_compression() {
        let mut config = test_config();
        config.enable_compression = true;
        let mut orch = SnapshotOrchestrator::new(config);

        let state = vec![0u8; 1000]; // Highly compressible
        let snap_ref = orch.capture("sb-1", "mod-a", &state).unwrap();

        let meta = orch.get_meta(&snap_ref).unwrap();
        assert!(meta.compressed);

        let restored = orch.restore(&snap_ref).unwrap();
        assert_eq!(restored.state, state);
    }

    #[test]
    fn test_snapshot_ref_display() {
        let snap = SnapshotRef::new("abcdef123456", 3);
        assert!(snap.to_string().contains("v3"));
    }

    #[test]
    fn test_orchestrator_error_display() {
        let err = OrchestratorError::NotFound("snap-1".to_string());
        assert!(err.to_string().contains("not found"));

        let err = OrchestratorError::StorageFull { used: 100, max: 50 };
        assert!(err.to_string().contains("full"));
    }

    #[test]
    fn test_simple_compress_decompress() {
        let original = b"aaabbbcccddd";
        let compressed = simple_compress(original);
        let decompressed = simple_decompress(&compressed);
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_empty() {
        let compressed = simple_compress(b"");
        assert!(compressed.is_empty());
        let decompressed = simple_decompress(&compressed);
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_gc() {
        let mut orch = SnapshotOrchestrator::new(test_config());
        orch.capture("sb-1", "mod-a", b"state").unwrap();

        let reclaimed = orch.gc();
        // Nothing expired yet (TTL is 3600s)
        assert_eq!(reclaimed, 0);
        assert_eq!(orch.stats().gc_runs, 1);
    }

    #[test]
    fn test_incremental_parent() {
        let config = OrchestratorConfig { enable_incremental: true, ..test_config() };
        let mut orch = SnapshotOrchestrator::new(config);

        let r1 = orch.capture("sb-1", "mod-a", b"state1").unwrap();
        let r2 = orch.capture("sb-1", "mod-a", b"state2").unwrap();

        let meta2 = orch.get_meta(&r2).unwrap();
        assert!(meta2.incremental);
        assert_eq!(meta2.parent.as_ref().unwrap(), &r1);
    }
}
