//! Warm pool for pre-initialized sandboxes.

use super::{Snapshot, SnapshotEngine, SnapshotId};
use crate::config::ModuleHash;
use crate::error::{Error, Result};

use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Configuration for the warm pool.
#[derive(Debug, Clone)]
pub struct WarmPoolConfig {
    /// Maximum number of warm instances per module.
    pub max_per_module: usize,
    /// Total maximum warm instances across all modules.
    pub max_total: usize,
    /// Time to keep warm instances before eviction.
    pub ttl: Duration,
    /// Enable automatic pre-warming.
    pub auto_prewarm: bool,
    /// Number of instances to pre-warm per module.
    pub prewarm_count: usize,
}

impl Default for WarmPoolConfig {
    fn default() -> Self {
        Self {
            max_per_module: 10,
            max_total: 100,
            ttl: Duration::from_secs(300), // 5 minutes
            auto_prewarm: false,
            prewarm_count: 3,
        }
    }
}

/// Statistics about the warm pool.
#[derive(Debug, Clone, Default)]
pub struct WarmPoolStats {
    /// Total number of warm instances.
    pub warm_count: usize,
    /// Number of modules with warm instances.
    pub module_count: usize,
    /// Number of hits (successful gets).
    pub hits: u64,
    /// Number of misses.
    pub misses: u64,
    /// Number of evictions.
    pub evictions: u64,
}

impl WarmPoolStats {
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

/// A pool of warm (pre-initialized) sandboxes for fast startup.
pub struct WarmPool {
    config: WarmPoolConfig,
    snapshot_engine: Arc<SnapshotEngine>,
    pool: DashMap<ModuleHash, Vec<PoolEntry>>,
    total_count: AtomicUsize,
    hits: AtomicUsize,
    misses: AtomicUsize,
    evictions: AtomicUsize,
    semaphore: Arc<Semaphore>,
}

#[derive(Debug)]
struct PoolEntry {
    snapshot_id: SnapshotId,
    created_at: std::time::Instant,
}

impl WarmPool {
    /// Create a new warm pool.
    pub fn new(config: WarmPoolConfig, snapshot_engine: Arc<SnapshotEngine>) -> Self {
        let max_total = config.max_total;
        Self {
            config,
            snapshot_engine,
            pool: DashMap::new(),
            total_count: AtomicUsize::new(0),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            evictions: AtomicUsize::new(0),
            semaphore: Arc::new(Semaphore::new(max_total)),
        }
    }

    /// Get a warm instance for a module, if available.
    pub fn get(&self, module_hash: &ModuleHash) -> Option<Snapshot> {
        if let Some(mut entries) = self.pool.get_mut(module_hash) {
            // Find a non-expired entry
            let now = std::time::Instant::now();
            while let Some(entry) = entries.pop() {
                if now.duration_since(entry.created_at) < self.config.ttl {
                    // Entry is still valid
                    self.total_count.fetch_sub(1, Ordering::Relaxed);
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    self.semaphore.add_permits(1);

                    // Try to load the snapshot
                    if let Ok(snapshot) = self.snapshot_engine.load(&entry.snapshot_id) {
                        tracing::debug!(
                            module_hash = %module_hash,
                            snapshot_id = %entry.snapshot_id,
                            "Warm pool hit"
                        );
                        return Some(snapshot);
                    }
                } else {
                    // Entry expired
                    self.evictions.fetch_add(1, Ordering::Relaxed);
                    self.total_count.fetch_sub(1, Ordering::Relaxed);
                    self.semaphore.add_permits(1);
                }
            }
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(module_hash = %module_hash, "Warm pool miss");
        None
    }

    /// Return a snapshot to the pool.
    pub fn put(&self, snapshot: Snapshot) -> Result<()> {
        let module_hash = snapshot.module_hash.clone();

        // Check if we have room
        if self.total_count.load(Ordering::Relaxed) >= self.config.max_total {
            // Try to evict something
            self.evict_one();
        }

        // Check per-module limit
        let current = self.pool.get(&module_hash).map(|e| e.len()).unwrap_or(0);

        if current >= self.config.max_per_module {
            // Don't add, already at limit
            return Ok(());
        }

        // Try to acquire a permit
        match self.semaphore.try_acquire() {
            Ok(permit) => {
                permit.forget(); // We'll add_permits when removing

                // Store the snapshot
                let snapshot_id = self.snapshot_engine.store(snapshot)?;

                // Add to pool
                self.pool
                    .entry(module_hash.clone())
                    .or_default()
                    .push(PoolEntry { snapshot_id, created_at: std::time::Instant::now() });

                self.total_count.fetch_add(1, Ordering::Relaxed);

                tracing::debug!(
                    module_hash = %module_hash,
                    snapshot_id = %snapshot_id,
                    "Snapshot added to warm pool"
                );

                Ok(())
            }
            Err(_) => {
                // Pool is full
                Err(Error::PoolExhausted)
            }
        }
    }

    /// Evict one entry from the pool (oldest first).
    fn evict_one(&self) {
        for mut entry in self.pool.iter_mut() {
            if let Some(pool_entry) = entry.value_mut().pop() {
                self.total_count.fetch_sub(1, Ordering::Relaxed);
                self.evictions.fetch_add(1, Ordering::Relaxed);
                self.semaphore.add_permits(1);

                // Remove the snapshot
                let _ = self.snapshot_engine.remove(&pool_entry.snapshot_id);

                tracing::debug!(
                    module_hash = %entry.key(),
                    snapshot_id = %pool_entry.snapshot_id,
                    "Evicted from warm pool"
                );
                return;
            }
        }
    }

    /// Evict all expired entries.
    pub fn evict_expired(&self) {
        let now = std::time::Instant::now();
        let mut evicted = 0;

        for mut entry in self.pool.iter_mut() {
            entry.value_mut().retain(|e| {
                if now.duration_since(e.created_at) >= self.config.ttl {
                    self.total_count.fetch_sub(1, Ordering::Relaxed);
                    self.semaphore.add_permits(1);
                    let _ = self.snapshot_engine.remove(&e.snapshot_id);
                    evicted += 1;
                    false
                } else {
                    true
                }
            });
        }

        // Remove empty entries
        self.pool.retain(|_, v| !v.is_empty());

        if evicted > 0 {
            self.evictions.fetch_add(evicted, Ordering::Relaxed);
            tracing::debug!(evicted = evicted, "Evicted expired entries from warm pool");
        }
    }

    /// Get the current size of the pool.
    pub fn size(&self) -> usize {
        self.total_count.load(Ordering::Relaxed)
    }

    /// Get the number of modules in the pool.
    pub fn module_count(&self) -> usize {
        self.pool.len()
    }

    /// Get pool statistics.
    pub fn stats(&self) -> WarmPoolStats {
        WarmPoolStats {
            warm_count: self.total_count.load(Ordering::Relaxed),
            module_count: self.pool.len(),
            hits: self.hits.load(Ordering::Relaxed) as u64,
            misses: self.misses.load(Ordering::Relaxed) as u64,
            evictions: self.evictions.load(Ordering::Relaxed) as u64,
        }
    }

    /// Clear the entire pool.
    pub fn clear(&self) {
        for entry in self.pool.iter() {
            for pool_entry in entry.value() {
                let _ = self.snapshot_engine.remove(&pool_entry.snapshot_id);
            }
        }
        self.pool.clear();
        self.total_count.store(0, Ordering::Relaxed);
    }

    /// Apply warming recommendations from an [`AccessTracker`].
    ///
    /// Pre-warms modules that are classified as "hot" and evicts modules
    /// that the tracker considers cold. Requires a callback to create
    /// snapshots for modules that need warming.
    pub fn apply_warming_recommendation(
        &self,
        recommendation: &super::auto_warm::WarmingRecommendation,
    ) {
        // Evict cold modules
        for module_hash in &recommendation.modules_to_evict {
            if let Some((_, entries)) = self.pool.remove(module_hash) {
                let count = entries.len();
                for entry in &entries {
                    let _ = self.snapshot_engine.remove(&entry.snapshot_id);
                    self.semaphore.add_permits(1);
                }
                self.total_count.fetch_sub(count, Ordering::Relaxed);
                self.evictions.fetch_add(count, Ordering::Relaxed);
                tracing::info!(
                    module_hash = %module_hash,
                    count = count,
                    "Evicted cold module from warm pool"
                );
            }
        }

        tracing::info!(
            to_warm = recommendation.modules_to_warm.len(),
            to_evict = recommendation.modules_to_evict.len(),
            hot_count = recommendation.hot_count,
            "Applied warming recommendation"
        );
    }

    /// Record a module access and check if auto-warming should trigger.
    ///
    /// Returns true if the module is now considered hot and should be pre-warmed.
    pub fn record_access_and_check(
        &self,
        tracker: &super::auto_warm::AccessTracker,
        module_hash: &ModuleHash,
        cold_start_ms: f64,
    ) -> bool {
        tracker.record_access(module_hash, cold_start_ms);
        let was_miss = !self.pool.contains_key(module_hash);
        was_miss && tracker.is_hot(module_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxId;
    use crate::snapshot::Snapshot;

    fn create_test_pool() -> WarmPool {
        let config = WarmPoolConfig {
            max_per_module: 3,
            max_total: 10,
            ttl: Duration::from_secs(60),
            ..Default::default()
        };
        let engine = Arc::new(SnapshotEngine::with_defaults().unwrap());
        WarmPool::new(config, engine)
    }

    #[test]
    fn test_warm_pool_put_get() {
        let pool = create_test_pool();
        let module_hash = ModuleHash("test123".to_string());
        let sandbox_id = SandboxId::new();

        // Put a snapshot
        let snapshot = Snapshot::new(sandbox_id, module_hash.clone());
        pool.put(snapshot).unwrap();

        assert_eq!(pool.size(), 1);

        // Get it back
        let retrieved = pool.get(&module_hash);
        assert!(retrieved.is_some());
        assert_eq!(pool.size(), 0);

        // Should miss now
        let miss = pool.get(&module_hash);
        assert!(miss.is_none());
    }

    #[test]
    fn test_warm_pool_stats() {
        let pool = create_test_pool();
        let module_hash = ModuleHash("test123".to_string());
        let sandbox_id = SandboxId::new();

        // Put and get
        pool.put(Snapshot::new(sandbox_id, module_hash.clone())).unwrap();
        pool.get(&module_hash);
        pool.get(&module_hash); // Miss

        let stats = pool.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate(), 0.5);
    }

    #[test]
    fn test_warm_pool_per_module_limit() {
        let pool = create_test_pool();
        let module_hash = ModuleHash("test123".to_string());
        let sandbox_id = SandboxId::new();

        // Try to add more than max_per_module
        for _ in 0..5 {
            pool.put(Snapshot::new(sandbox_id, module_hash.clone())).ok();
        }

        // Should be capped at max_per_module
        assert!(pool.size() <= 3);
    }

    #[test]
    fn test_warm_pool_clear() {
        let pool = create_test_pool();
        let module_hash = ModuleHash("test123".to_string());
        let sandbox_id = SandboxId::new();

        pool.put(Snapshot::new(sandbox_id, module_hash.clone())).unwrap();
        pool.put(Snapshot::new(sandbox_id, module_hash.clone())).unwrap();

        assert_eq!(pool.size(), 2);

        pool.clear();
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn test_warm_pool_apply_warming_recommendation() {
        let pool = create_test_pool();
        let hot = ModuleHash("hot_module".to_string());
        let cold = ModuleHash("cold_module".to_string());

        // Add a snapshot for the cold module
        let snapshot = Snapshot::new(SandboxId::new(), cold.clone());
        pool.put(snapshot).unwrap();
        assert_eq!(pool.size(), 1);

        // Create a recommendation to evict the cold module
        let recommendation = super::super::auto_warm::WarmingRecommendation {
            modules_to_warm: vec![hot],
            modules_to_evict: vec![cold.clone()],
            total_tracked: 2,
            hot_count: 1,
        };

        pool.apply_warming_recommendation(&recommendation);
        assert_eq!(pool.size(), 0);
        assert!(pool.get(&cold).is_none());
    }

    #[test]
    fn test_warm_pool_record_access_and_check() {
        let pool = create_test_pool();
        let tracker = super::super::auto_warm::AccessTracker::new(
            super::super::auto_warm::AutoWarmConfig {
                hot_threshold: 3,
                ..Default::default()
            },
        );
        let module_hash = ModuleHash("test_mod".to_string());

        // First few accesses: not hot yet
        assert!(!pool.record_access_and_check(&tracker, &module_hash, 5.0));
        assert!(!pool.record_access_and_check(&tracker, &module_hash, 4.0));

        // Third access: becomes hot, pool doesn't have it → should suggest warming
        assert!(pool.record_access_and_check(&tracker, &module_hash, 3.0));
    }
}
