//! Checkout/return pool for pre-initialized sandbox snapshots.
//!
//! Provides a thread-safe pool of pre-warmed snapshot slots organized
//! by module hash. Slots are checked out via RAII guards that
//! automatically return the slot on drop.

use super::SnapshotId;
use crate::config::ModuleHash;
use crate::error::{Error, Result};

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

/// Configuration for the checkout pool.
#[derive(Debug, Clone)]
pub struct CheckoutPoolConfig {
    /// Maximum slots per module hash.
    pub max_per_module: usize,
    /// Total maximum slots across all modules.
    pub max_total: usize,
}

impl Default for CheckoutPoolConfig {
    fn default() -> Self {
        Self { max_per_module: 10, max_total: 100 }
    }
}

/// A single slot in the pool.
#[derive(Debug, Clone)]
pub struct PoolSlot {
    /// Snapshot identifier.
    pub snapshot_id: SnapshotId,
    /// Module this slot belongs to.
    pub module_hash: ModuleHash,
    /// When the slot was created.
    pub created_at: Instant,
    /// Number of times this slot has been checked out.
    pub checkout_count: u64,
}

/// Statistics about the checkout pool.
#[derive(Debug, Clone, Default)]
pub struct CheckoutPoolStats {
    /// Total slots across all modules.
    pub total_slots: usize,
    /// Number of distinct modules.
    pub module_count: usize,
    /// Successful checkouts.
    pub checkouts: u64,
    /// Successful returns.
    pub returns: u64,
    /// Slots currently checked out.
    pub checked_out: u64,
}

/// RAII guard that returns a slot to the pool on drop.
pub struct CheckoutHandle {
    slot: Option<PoolSlot>,
    pool: Arc<CheckoutPoolInner>,
}

impl std::fmt::Debug for CheckoutHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckoutHandle").field("slot", &self.slot).finish()
    }
}

impl CheckoutHandle {
    /// Get a reference to the held slot.
    pub fn slot(&self) -> &PoolSlot {
        self.slot.as_ref().expect("slot taken after drop")
    }
}

impl Drop for CheckoutHandle {
    fn drop(&mut self) {
        if let Some(mut slot) = self.slot.take() {
            slot.checkout_count += 1;
            let module_hash = slot.module_hash.clone();

            // Return to available slots.
            self.pool
                .available
                .entry(module_hash)
                .or_default()
                .push(slot);
            self.pool.checked_out.fetch_sub(1, Ordering::Relaxed);
            self.pool.returns.fetch_add(1, Ordering::Relaxed);
            self.pool.semaphore.add_permits(1);
        }
    }
}

/// Shared inner state for the pool.
struct CheckoutPoolInner {
    config: CheckoutPoolConfig,
    /// Module hash -> available (not checked-out) slots.
    available: DashMap<ModuleHash, Vec<PoolSlot>>,
    semaphore: Semaphore,
    checkouts: AtomicU64,
    returns: AtomicU64,
    checked_out: AtomicU64,
}

/// A thread-safe checkout/return pool for pre-warmed snapshot slots.
pub struct CheckoutPool {
    inner: Arc<CheckoutPoolInner>,
}

impl CheckoutPool {
    /// Create a new checkout pool.
    pub fn new(config: CheckoutPoolConfig) -> Self {
        let max_total = config.max_total;
        Self {
            inner: Arc::new(CheckoutPoolInner {
                config,
                available: DashMap::new(),
                semaphore: Semaphore::new(max_total),
                checkouts: AtomicU64::new(0),
                returns: AtomicU64::new(0),
                checked_out: AtomicU64::new(0),
            }),
        }
    }

    /// Pre-warm the pool by adding `count` slots for the given module.
    pub fn prewarm(&self, module_hash: ModuleHash, count: usize) -> Result<usize> {
        let inner = &self.inner;
        let current = inner.available.get(&module_hash).map(|v| v.len()).unwrap_or(0);
        let checked = inner
            .available
            .iter()
            .map(|e| e.value().len())
            .sum::<usize>()
            + inner.checked_out.load(Ordering::Relaxed) as usize;

        let mut added = 0;
        for _ in 0..count {
            let per_module =
                inner.available.get(&module_hash).map(|v| v.len()).unwrap_or(0);
            if per_module >= inner.config.max_per_module {
                break;
            }
            if checked + added >= inner.config.max_total {
                break;
            }

            match inner.semaphore.try_acquire() {
                Ok(permit) => {
                    permit.forget();
                    let slot = PoolSlot {
                        snapshot_id: SnapshotId::new(),
                        module_hash: module_hash.clone(),
                        created_at: Instant::now(),
                        checkout_count: 0,
                    };
                    inner.available.entry(module_hash.clone()).or_default().push(slot);
                    added += 1;
                }
                Err(_) => break,
            }
        }

        if added == 0 && count > 0 && current >= inner.config.max_per_module {
            return Err(Error::PoolExhausted);
        }

        tracing::debug!(
            module_hash = %module_hash,
            added = added,
            "Prewarmed checkout pool"
        );
        Ok(added)
    }

    /// Checkout a slot for the given module hash.
    ///
    /// Returns an RAII [`CheckoutHandle`] that automatically returns
    /// the slot when dropped.
    pub fn checkout(&self, module_hash: &ModuleHash) -> Result<CheckoutHandle> {
        let inner = &self.inner;

        let slot = inner
            .available
            .get_mut(module_hash)
            .and_then(|mut slots| slots.pop());

        match slot {
            Some(s) => {
                inner.checkouts.fetch_add(1, Ordering::Relaxed);
                inner.checked_out.fetch_add(1, Ordering::Relaxed);
                Ok(CheckoutHandle { slot: Some(s), pool: Arc::clone(&self.inner) })
            }
            None => Err(Error::PoolExhausted),
        }
    }

    /// Explicitly return a slot to the pool.
    ///
    /// This is equivalent to dropping the handle, but allows the
    /// caller to confirm the return happened.
    pub fn return_slot(&self, handle: CheckoutHandle) {
        drop(handle);
    }

    /// Get pool statistics.
    pub fn stats(&self) -> CheckoutPoolStats {
        let inner = &self.inner;
        let total_slots: usize = inner.available.iter().map(|e| e.value().len()).sum();
        CheckoutPoolStats {
            total_slots,
            module_count: inner.available.len(),
            checkouts: inner.checkouts.load(Ordering::Relaxed),
            returns: inner.returns.load(Ordering::Relaxed),
            checked_out: inner.checked_out.load(Ordering::Relaxed),
        }
    }

    /// Drain all slots for a given module, returning the count removed.
    pub fn drain(&self, module_hash: &ModuleHash) -> usize {
        let inner = &self.inner;
        match inner.available.remove(module_hash) {
            Some((_, slots)) => {
                let count = slots.len();
                // Release the semaphore permits for the drained slots.
                inner.semaphore.add_permits(count);
                tracing::debug!(
                    module_hash = %module_hash,
                    count = count,
                    "Drained checkout pool slots"
                );
                count
            }
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_hash(name: &str) -> ModuleHash {
        ModuleHash(format!("{:0>64}", name))
    }

    #[test]
    fn test_prewarm_and_checkout() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        let added = pool.prewarm(hash.clone(), 3).unwrap();
        assert_eq!(added, 3);

        let stats = pool.stats();
        assert_eq!(stats.total_slots, 3);
        assert_eq!(stats.module_count, 1);

        let handle = pool.checkout(&hash).unwrap();
        assert_eq!(handle.slot().module_hash, hash);
        assert_eq!(pool.stats().total_slots, 2);
        assert_eq!(pool.stats().checked_out, 1);
    }

    #[test]
    fn test_checkout_miss() {
        let pool = CheckoutPool::new(Default::default());
        let result = pool.checkout(&module_hash("nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_drop_returns_slot() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        pool.prewarm(hash.clone(), 1).unwrap();

        {
            let _handle = pool.checkout(&hash).unwrap();
            assert_eq!(pool.stats().total_slots, 0);
            assert_eq!(pool.stats().checked_out, 1);
        }

        // After drop, slot is returned.
        assert_eq!(pool.stats().total_slots, 1);
        assert_eq!(pool.stats().checked_out, 0);
        assert_eq!(pool.stats().returns, 1);
    }

    #[test]
    fn test_explicit_return() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        pool.prewarm(hash.clone(), 1).unwrap();

        let handle = pool.checkout(&hash).unwrap();
        pool.return_slot(handle);

        assert_eq!(pool.stats().total_slots, 1);
        assert_eq!(pool.stats().returns, 1);
    }

    #[test]
    fn test_checkout_count_increments() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        pool.prewarm(hash.clone(), 1).unwrap();

        // First checkout/return cycle.
        let handle = pool.checkout(&hash).unwrap();
        assert_eq!(handle.slot().checkout_count, 0);
        drop(handle);

        // Second checkout – count should be 1.
        let handle = pool.checkout(&hash).unwrap();
        assert_eq!(handle.slot().checkout_count, 1);
        drop(handle);
    }

    #[test]
    fn test_per_module_limit() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 3,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        let added = pool.prewarm(hash.clone(), 10).unwrap();
        assert_eq!(added, 3);
    }

    #[test]
    fn test_total_limit() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 10,
            max_total: 5,
        });

        let h1 = module_hash("mod-1");
        let h2 = module_hash("mod-2");

        let a1 = pool.prewarm(h1.clone(), 3).unwrap();
        let a2 = pool.prewarm(h2.clone(), 3).unwrap();

        assert_eq!(a1 + a2, 5);
    }

    #[test]
    fn test_drain() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 10,
            max_total: 20,
        });

        let h1 = module_hash("mod-1");
        let h2 = module_hash("mod-2");

        pool.prewarm(h1.clone(), 3).unwrap();
        pool.prewarm(h2.clone(), 2).unwrap();

        let drained = pool.drain(&h1);
        assert_eq!(drained, 3);
        assert_eq!(pool.stats().total_slots, 2);
        assert_eq!(pool.stats().module_count, 1);
    }

    #[test]
    fn test_drain_nonexistent() {
        let pool = CheckoutPool::new(Default::default());
        assert_eq!(pool.drain(&module_hash("ghost")), 0);
    }

    #[test]
    fn test_stats_initial() {
        let pool = CheckoutPool::new(Default::default());
        let stats = pool.stats();
        assert_eq!(stats.total_slots, 0);
        assert_eq!(stats.module_count, 0);
        assert_eq!(stats.checkouts, 0);
        assert_eq!(stats.returns, 0);
        assert_eq!(stats.checked_out, 0);
    }

    #[test]
    fn test_multiple_modules() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let h1 = module_hash("mod-1");
        let h2 = module_hash("mod-2");
        let h3 = module_hash("mod-3");

        pool.prewarm(h1.clone(), 2).unwrap();
        pool.prewarm(h2.clone(), 3).unwrap();
        pool.prewarm(h3.clone(), 1).unwrap();

        assert_eq!(pool.stats().total_slots, 6);
        assert_eq!(pool.stats().module_count, 3);

        let _h1 = pool.checkout(&h1).unwrap();
        let _h2 = pool.checkout(&h2).unwrap();
        assert_eq!(pool.stats().checked_out, 2);
    }

    #[test]
    fn test_prewarm_returns_error_when_full_per_module() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 2,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        pool.prewarm(hash.clone(), 2).unwrap();

        let result = pool.prewarm(hash.clone(), 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_slot_has_unique_snapshot_id() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        pool.prewarm(hash.clone(), 3).unwrap();

        let h1 = pool.checkout(&hash).unwrap();
        let h2 = pool.checkout(&hash).unwrap();
        let h3 = pool.checkout(&hash).unwrap();

        assert_ne!(h1.slot().snapshot_id, h2.slot().snapshot_id);
        assert_ne!(h2.slot().snapshot_id, h3.slot().snapshot_id);
    }

    #[test]
    fn test_drain_then_prewarm() {
        let pool = CheckoutPool::new(CheckoutPoolConfig {
            max_per_module: 5,
            max_total: 20,
        });

        let hash = module_hash("mod-1");
        pool.prewarm(hash.clone(), 3).unwrap();
        pool.drain(&hash);
        assert_eq!(pool.stats().total_slots, 0);

        // Re-prewarm after drain.
        let added = pool.prewarm(hash.clone(), 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(pool.stats().total_slots, 2);
    }
}
