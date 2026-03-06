//! Background pool manager for auto-prewarming.
//!
//! Periodically evaluates demand and warms or evicts sandbox instances
//! in the [`WarmPool`](super::warm::WarmPool).

use super::prewarm::{PreWarmAction, PreWarmEngine};
use super::warm::WarmPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Manages a [`WarmPool`] with background auto-prewarming.
pub struct PoolManager {
    warm_pool: Arc<Mutex<WarmPool>>,
    prewarm_engine: Arc<Mutex<PreWarmEngine>>,
}

impl PoolManager {
    /// Create a new pool manager wrapping the given pool and prewarm engine.
    pub fn new(warm_pool: WarmPool, prewarm_engine: PreWarmEngine) -> Self {
        Self {
            warm_pool: Arc::new(Mutex::new(warm_pool)),
            prewarm_engine: Arc::new(Mutex::new(prewarm_engine)),
        }
    }

    /// Get a shared reference to the warm pool.
    pub fn pool(&self) -> Arc<Mutex<WarmPool>> {
        self.warm_pool.clone()
    }

    /// Record a request for demand tracking.
    pub fn record_request(&self, module_name: &str) {
        if let Ok(mut engine) = self.prewarm_engine.lock() {
            engine.record_request(module_name);
        }
    }

    /// Start the background auto-prewarm loop.
    ///
    /// Spawns a tokio task that periodically evaluates demand and executes
    /// warm-up or eviction decisions. Returns immediately.
    pub fn start_background_task(&self, interval: Duration) {
        let pool = self.warm_pool.clone();
        let engine = self.prewarm_engine.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                let decisions = {
                    let mut eng = match engine.lock() {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if !eng.should_evaluate() {
                        continue;
                    }
                    // Collect current warm instance counts from the pool
                    let current_counts: HashMap<String, usize> = match pool.lock() {
                        Ok(p) => p
                            .list_modules()
                            .iter()
                            .map(|name| (name.to_string(), p.warm_count(name)))
                            .collect(),
                        Err(_) => continue,
                    };
                    eng.evaluate(&current_counts)
                };

                let mut p = match pool.lock() {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                for decision in decisions {
                    match decision.action {
                        PreWarmAction::ScaleUp(count) => {
                            let _ = p.warm_up(&decision.module_name, count);
                            tracing::debug!(
                                module = %decision.module_name,
                                count,
                                "Auto-warmed instances"
                            );
                        }
                        PreWarmAction::ScaleDown(_) => {
                            p.evict_idle();
                        }
                        PreWarmAction::NoChange => {}
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::prewarm::PreWarmConfig;
    use crate::pool::warm::WarmPoolConfig;

    #[test]
    fn test_pool_manager_creation() {
        let pool = WarmPool::new(WarmPoolConfig::default());
        let engine = PreWarmEngine::new(PreWarmConfig::default());
        let manager = PoolManager::new(pool, engine);
        assert!(manager.pool().lock().is_ok());
    }

    #[test]
    fn test_record_request() {
        let pool = WarmPool::new(WarmPoolConfig::default());
        let engine = PreWarmEngine::new(PreWarmConfig::default());
        let manager = PoolManager::new(pool, engine);
        manager.record_request("test-module");
    }
}
