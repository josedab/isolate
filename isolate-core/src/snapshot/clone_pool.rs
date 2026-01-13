//! Instant clone pool with LRU eviction and module deduplication.
//!
//! Extends the warm pool with instant-clone capabilities, allowing
//! sub-100μs sandbox creation from pre-warmed snapshots.

use super::{Snapshot, SnapshotEngine, SnapshotId};
use crate::config::ModuleHash;
use crate::error::{Error, Result};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for the clone pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClonePoolConfig {
    /// Maximum total cloneable snapshots in the pool.
    pub max_total: usize,
    /// Maximum snapshots per module hash.
    pub max_per_module: usize,
    /// Pre-warm target per module (number to keep ready).
    pub prewarm_target: usize,
    /// Idle timeout before evicting a snapshot.
    pub idle_timeout: Duration,
    /// Enable LRU eviction when pool is full.
    pub lru_eviction: bool,
    /// Enable module-hash-based deduplication.
    pub deduplication: bool,
}

impl Default for ClonePoolConfig {
    fn default() -> Self {
        Self {
            max_total: 100,
            max_per_module: 20,
            prewarm_target: 3,
            idle_timeout: Duration::from_secs(300),
            lru_eviction: true,
            deduplication: true,
        }
    }
}

/// Statistics for the clone pool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClonePoolStats {
    /// Total snapshots in pool.
    pub total_count: usize,
    /// Unique modules in pool.
    pub unique_modules: usize,
    /// Clone operations performed.
    pub clone_ops: u64,
    /// Cache hits (clone from pool).
    pub hits: u64,
    /// Cache misses.
    pub misses: u64,
    /// Evictions performed.
    pub evictions: u64,
    /// Average clone time in microseconds.
    pub avg_clone_us: f64,
    /// Bytes saved by deduplication.
    pub dedup_bytes_saved: u64,
}

impl ClonePoolStats {
    /// Get the hit rate.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total > 0 {
            self.hits as f64 / total as f64
        } else {
            0.0
        }
    }
}

/// A cloneable entry in the pool.
struct CloneEntry {
    /// The snapshot ID.
    snapshot_id: SnapshotId,
    /// Module hash for deduplication.
    module_hash: ModuleHash,
    /// When this entry was added.
    added_at: Instant,
    /// Last time this entry was accessed.
    last_accessed: Instant,
    /// Number of times this entry was cloned.
    clone_count: u64,
    /// Size in bytes (estimated).
    size_bytes: usize,
}

/// The instant clone pool.
pub struct ClonePool {
    config: ClonePoolConfig,
    snapshot_engine: Arc<SnapshotEngine>,
    /// Module hash -> list of snapshot entries.
    entries: RwLock<HashMap<ModuleHash, VecDeque<CloneEntry>>>,
    /// Deduplication tracking: snapshot memory checksum -> snapshot ID.
    dedup_index: RwLock<HashMap<String, SnapshotId>>,
    /// Stats counters.
    clone_ops: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    total_clone_time_us: AtomicU64,
    dedup_bytes_saved: AtomicU64,
}

impl ClonePool {
    /// Create a new clone pool.
    pub fn new(config: ClonePoolConfig, snapshot_engine: Arc<SnapshotEngine>) -> Self {
        Self {
            config,
            snapshot_engine,
            entries: RwLock::new(HashMap::new()),
            dedup_index: RwLock::new(HashMap::new()),
            clone_ops: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            total_clone_time_us: AtomicU64::new(0),
            dedup_bytes_saved: AtomicU64::new(0),
        }
    }

    /// Add a snapshot to the pool for future cloning.
    pub fn add(&self, snapshot: Snapshot) -> Result<SnapshotId> {
        let module_hash = snapshot.module_hash.clone();
        let memory_checksum = snapshot.memory_checksum.clone();
        let size_bytes = snapshot.memory_size;

        // Check deduplication
        if self.config.deduplication {
            let dedup = self.dedup_index.read();
            if let Some(existing_id) = dedup.get(&memory_checksum) {
                self.dedup_bytes_saved.fetch_add(size_bytes as u64, Ordering::Relaxed);
                tracing::debug!(
                    module_hash = %module_hash,
                    checksum = %memory_checksum,
                    "Snapshot deduplicated"
                );
                return Ok(*existing_id);
            }
        }

        // Evict if necessary
        self.ensure_capacity(&module_hash);

        // Store the snapshot
        let snapshot_id = self.snapshot_engine.store(snapshot)?;

        // Add to entries
        {
            let now = Instant::now();
            let mut entries = self.entries.write();
            let module_entries = entries.entry(module_hash.clone()).or_default();
            module_entries.push_back(CloneEntry {
                snapshot_id,
                module_hash: module_hash.clone(),
                added_at: now,
                last_accessed: now,
                clone_count: 0,
                size_bytes,
            });
        }

        // Add to dedup index
        if self.config.deduplication {
            self.dedup_index.write().insert(memory_checksum, snapshot_id);
        }

        tracing::debug!(
            module_hash = %module_hash,
            snapshot_id = %snapshot_id,
            "Snapshot added to clone pool"
        );

        Ok(snapshot_id)
    }

    /// Clone a snapshot for the given module, returning a fresh copy.
    pub fn clone_for_module(&self, module_hash: &ModuleHash) -> Result<Snapshot> {
        let start = Instant::now();
        self.clone_ops.fetch_add(1, Ordering::Relaxed);

        let snapshot_id = {
            let mut entries = self.entries.write();
            if let Some(module_entries) = entries.get_mut(module_hash) {
                // Find the best entry (least recently cloned for fairness)
                if let Some(entry) = module_entries.front_mut() {
                    entry.last_accessed = Instant::now();
                    entry.clone_count += 1;
                    Some(entry.snapshot_id)
                } else {
                    None
                }
            } else {
                None
            }
        };

        match snapshot_id {
            Some(id) => {
                let snapshot = self.snapshot_engine.load(&id)?;
                let elapsed_us = start.elapsed().as_micros() as u64;
                self.total_clone_time_us.fetch_add(elapsed_us, Ordering::Relaxed);
                self.hits.fetch_add(1, Ordering::Relaxed);

                tracing::debug!(
                    module_hash = %module_hash,
                    clone_us = elapsed_us,
                    "Instant clone from pool"
                );

                Ok(snapshot)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                Err(Error::SnapshotNotFound(format!(
                    "No snapshot available for module {}",
                    module_hash
                )))
            }
        }
    }

    /// Ensure there's capacity for a new entry.
    fn ensure_capacity(&self, module_hash: &ModuleHash) {
        let mut entries = self.entries.write();

        // Check per-module limit
        if let Some(module_entries) = entries.get_mut(module_hash) {
            while module_entries.len() >= self.config.max_per_module {
                if let Some(evicted) = module_entries.pop_front() {
                    let _ = self.snapshot_engine.remove(&evicted.snapshot_id);
                    self.evictions.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        // Check total limit with LRU eviction
        if self.config.lru_eviction {
            let total: usize = entries.values().map(|v| v.len()).sum();
            if total >= self.config.max_total {
                // Find the LRU entry across all modules
                let mut oldest_module = None;
                let mut oldest_time = Instant::now();

                for (hash, module_entries) in entries.iter() {
                    if let Some(front) = module_entries.front() {
                        if front.last_accessed < oldest_time {
                            oldest_time = front.last_accessed;
                            oldest_module = Some(hash.clone());
                        }
                    }
                }

                if let Some(hash) = oldest_module {
                    if let Some(module_entries) = entries.get_mut(&hash) {
                        if let Some(evicted) = module_entries.pop_front() {
                            let _ = self.snapshot_engine.remove(&evicted.snapshot_id);
                            self.evictions.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    /// Evict idle entries past the timeout.
    pub fn evict_idle(&self) {
        let now = Instant::now();
        let mut entries = self.entries.write();
        let mut evicted = 0;

        for module_entries in entries.values_mut() {
            module_entries.retain(|entry| {
                if now.duration_since(entry.last_accessed) >= self.config.idle_timeout {
                    let _ = self.snapshot_engine.remove(&entry.snapshot_id);
                    evicted += 1;
                    false
                } else {
                    true
                }
            });
        }

        entries.retain(|_, v| !v.is_empty());

        if evicted > 0 {
            self.evictions.fetch_add(evicted as u64, Ordering::Relaxed);
            tracing::debug!(evicted, "Evicted idle clone pool entries");
        }
    }

    /// Get the number of modules that need more snapshots (below prewarm target).
    pub fn modules_needing_prewarm(&self) -> Vec<(ModuleHash, usize)> {
        let entries = self.entries.read();
        let mut needs_prewarm = Vec::new();

        for (hash, module_entries) in entries.iter() {
            let count = module_entries.len();
            if count < self.config.prewarm_target {
                needs_prewarm.push((hash.clone(), self.config.prewarm_target - count));
            }
        }

        needs_prewarm
    }

    /// Get pool statistics.
    pub fn stats(&self) -> ClonePoolStats {
        let entries = self.entries.read();
        let total_count: usize = entries.values().map(|v| v.len()).sum();
        let clone_ops = self.clone_ops.load(Ordering::Relaxed);
        let total_clone_time = self.total_clone_time_us.load(Ordering::Relaxed);

        ClonePoolStats {
            total_count,
            unique_modules: entries.len(),
            clone_ops,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            avg_clone_us: if clone_ops > 0 {
                total_clone_time as f64 / clone_ops as f64
            } else {
                0.0
            },
            dedup_bytes_saved: self.dedup_bytes_saved.load(Ordering::Relaxed),
        }
    }

    /// Clear the entire pool.
    pub fn clear(&self) {
        let mut entries = self.entries.write();
        for module_entries in entries.values() {
            for entry in module_entries {
                let _ = self.snapshot_engine.remove(&entry.snapshot_id);
            }
        }
        entries.clear();
        self.dedup_index.write().clear();
    }

    /// Get the total number of snapshots in the pool.
    pub fn size(&self) -> usize {
        self.entries.read().values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxId;

    fn create_test_pool() -> ClonePool {
        let config = ClonePoolConfig {
            max_total: 10,
            max_per_module: 3,
            prewarm_target: 2,
            idle_timeout: Duration::from_secs(60),
            lru_eviction: true,
            deduplication: true,
        };
        let engine = Arc::new(SnapshotEngine::with_defaults().unwrap());
        ClonePool::new(config, engine)
    }

    fn test_snapshot(module_hash: &str) -> Snapshot {
        // Use a hash-length string to avoid Display truncation issues
        let padded = format!("{:0>64}", module_hash);
        let mut snap = Snapshot::new(SandboxId::new(), ModuleHash(padded));
        // Give each snapshot a unique checksum by default
        snap.memory_checksum = uuid::Uuid::new_v4().to_string();
        snap.memory_size = 4096;
        snap
    }

    fn module_hash(name: &str) -> ModuleHash {
        ModuleHash(format!("{:0>64}", name))
    }

    #[test]
    fn test_add_and_clone() {
        let pool = create_test_pool();
        let snap = test_snapshot("mod-1");

        pool.add(snap).unwrap();
        assert_eq!(pool.size(), 1);

        let cloned = pool.clone_for_module(&module_hash("mod-1"));
        assert!(cloned.is_ok());

        let stats = pool.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.clone_ops, 1);
    }

    #[test]
    fn test_clone_miss() {
        let pool = create_test_pool();
        let result = pool.clone_for_module(&module_hash("nonexistent"));
        assert!(result.is_err());

        let stats = pool.stats();
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_per_module_eviction() {
        let pool = create_test_pool();

        // Add more than max_per_module (each with unique checksum)
        for _ in 0..5 {
            pool.add(test_snapshot("mod-1")).unwrap();
        }

        assert!(pool.size() <= 3);
        assert!(pool.stats().evictions >= 2);
    }

    #[test]
    fn test_deduplication() {
        let pool = create_test_pool();

        let mut snap1 = test_snapshot("mod-1");
        snap1.memory_checksum = "shared-checksum-123".to_string();
        snap1.memory_size = 8192;
        let id1 = pool.add(snap1).unwrap();

        // Add another with same checksum
        let mut snap2 = test_snapshot("mod-1");
        snap2.memory_checksum = "shared-checksum-123".to_string();
        snap2.memory_size = 8192;
        let id2 = pool.add(snap2).unwrap();

        // Should return the same ID (deduplicated)
        assert_eq!(id1, id2);
        assert!(pool.stats().dedup_bytes_saved > 0);
    }

    #[test]
    fn test_modules_needing_prewarm() {
        let pool = create_test_pool();

        // Add one snapshot for a module (target is 2)
        pool.add(test_snapshot("mod-1")).unwrap();

        let needs = pool.modules_needing_prewarm();
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].1, 1); // Needs 1 more
    }

    #[test]
    fn test_clear() {
        let pool = create_test_pool();
        pool.add(test_snapshot("mod-1")).unwrap();
        pool.add(test_snapshot("mod-2")).unwrap();

        assert_eq!(pool.size(), 2);
        pool.clear();
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn test_stats() {
        let pool = create_test_pool();
        pool.add(test_snapshot("mod-1")).unwrap();
        pool.add(test_snapshot("mod-2")).unwrap();

        let stats = pool.stats();
        assert_eq!(stats.total_count, 2);
        assert_eq!(stats.unique_modules, 2);
        assert_eq!(stats.hit_rate(), 0.0);
    }
}
