//! Live module update manager for zero-downtime WASM module swaps.
//!
//! Orchestrates the full lifecycle of updating a running module version
//! without dropping active connections. Combines version registry, health
//! tracking, deployment control, and connection draining.

use super::deployment::{
    DeploymentController, DeploymentEvent, DeploymentState, DeploymentStrategy, RollbackTrigger,
};
use super::health::HealthTracker;
use super::version::{ModuleVersion, VersionId, VersionRegistry, VersionRoute, VersionRouter};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for the live update manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveUpdateConfig {
    /// Maximum time to wait for active connections to drain before force-switching.
    pub drain_timeout: Duration,
    /// How often to poll health metrics during canary advancement.
    pub health_check_interval: Duration,
    /// Deployment strategy for rolling out new versions.
    pub strategy: DeploymentStrategy,
    /// Conditions that trigger automatic rollback.
    pub rollback_trigger: RollbackTrigger,
    /// Maximum number of versions to retain in the registry.
    pub max_retained_versions: usize,
}

impl Default for LiveUpdateConfig {
    fn default() -> Self {
        Self {
            drain_timeout: Duration::from_secs(30),
            health_check_interval: Duration::from_secs(5),
            strategy: DeploymentStrategy::default(),
            rollback_trigger: RollbackTrigger::error_rate(5.0).with_min_requests(10),
            max_retained_versions: 5,
        }
    }
}

/// Tracks active connections per version for graceful draining.
pub struct ConnectionTracker {
    connections: dashmap::DashMap<VersionId, AtomicU64>,
}

impl ConnectionTracker {
    /// Create a new connection tracker.
    pub fn new() -> Self {
        Self {
            connections: dashmap::DashMap::new(),
        }
    }

    /// Acquire a connection slot for a version. Returns a guard that
    /// decrements the count on drop.
    pub fn acquire(&self, version_id: &VersionId) -> ConnectionGuard {
        self.connections
            .entry(version_id.clone())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
        ConnectionGuard {
            version_id: version_id.clone(),
            tracker: self,
        }
    }

    /// Get the number of active connections for a version.
    pub fn active_count(&self, version_id: &VersionId) -> u64 {
        self.connections
            .get(version_id)
            .map(|c| c.value().load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Check if a version has been fully drained (zero active connections).
    pub fn is_drained(&self, version_id: &VersionId) -> bool {
        self.active_count(version_id) == 0
    }

    fn release(&self, version_id: &VersionId) {
        if let Some(counter) = self.connections.get(version_id) {
            let prev = counter.value().fetch_sub(1, Ordering::Relaxed);
            // Guard against underflow (shouldn't happen in normal operation)
            if prev == 0 {
                counter.value().store(0, Ordering::Relaxed);
            }
        }
    }
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that decrements the connection count when dropped.
pub struct ConnectionGuard<'a> {
    version_id: VersionId,
    tracker: &'a ConnectionTracker,
}

impl<'a> Drop for ConnectionGuard<'a> {
    fn drop(&mut self) {
        self.tracker.release(&self.version_id);
    }
}

/// Result of a live update operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateResult {
    /// Update completed successfully, all traffic on new version.
    Completed {
        version: VersionId,
        duration: Duration,
    },
    /// Update was rolled back due to health issues.
    RolledBack {
        version: VersionId,
        reason: String,
    },
    /// Update is still in progress (canary phase).
    InProgress {
        version: VersionId,
        canary_pct: u8,
    },
}

/// Manages zero-downtime live updates of WASM modules.
///
/// Ties together the version registry, router, health tracker, deployment
/// controller, and connection tracker to provide a unified API for live
/// module updates with canary rollouts.
pub struct LiveUpdateManager {
    config: LiveUpdateConfig,
    registry: Arc<VersionRegistry>,
    router: Arc<VersionRouter>,
    health: Arc<HealthTracker>,
    connections: Arc<ConnectionTracker>,
    controller: DeploymentController,
    active_version: parking_lot::RwLock<Option<VersionId>>,
    update_started_at: parking_lot::Mutex<Option<Instant>>,
}

impl LiveUpdateManager {
    /// Create a new live update manager.
    pub fn new(config: LiveUpdateConfig) -> Self {
        let controller = DeploymentController::new(
            config.strategy.clone(),
            config.rollback_trigger.clone(),
        );

        Self {
            config,
            registry: Arc::new(VersionRegistry::new()),
            router: Arc::new(VersionRouter::new()),
            health: Arc::new(HealthTracker::new()),
            connections: Arc::new(ConnectionTracker::new()),
            controller,
            active_version: parking_lot::RwLock::new(None),
            update_started_at: parking_lot::Mutex::new(None),
        }
    }

    /// Deploy the initial version. Must be called before any updates.
    pub fn deploy_initial(&self, version: ModuleVersion) {
        let vid = version.id.clone();
        self.registry.register(version);
        self.router.set_route(VersionRoute::single(vid.clone()));
        *self.active_version.write() = Some(vid);
    }

    /// Start a live update to a new module version.
    ///
    /// Registers the new version and begins the canary rollout.
    /// Returns an error if there's already an update in progress.
    pub fn start_update(&self, new_version: ModuleVersion) -> Result<UpdateResult, String> {
        if matches!(self.controller.state(), DeploymentState::InProgress { .. }) {
            return Err("another update is already in progress".into());
        }

        let active = self
            .active_version
            .read()
            .clone()
            .ok_or("no active version deployed")?;

        let new_vid = new_version.id.clone();
        self.registry.register(new_version);

        *self.update_started_at.lock() = Some(Instant::now());
        self.controller.start_deployment(new_vid.clone());

        // For immediate deployments, the controller completes immediately
        if matches!(self.controller.state(), DeploymentState::Completed { .. }) {
            self.router
                .set_route(VersionRoute::single(new_vid.clone()));
            *self.active_version.write() = Some(new_vid.clone());
            self.gc_old_versions();
            let duration = self
                .update_started_at
                .lock()
                .map(|s| s.elapsed())
                .unwrap_or_default();
            return Ok(UpdateResult::Completed {
                version: new_vid,
                duration,
            });
        }

        // Set up canary route
        let canary_pct = match self.controller.state() {
            DeploymentState::InProgress { canary_pct, .. } => canary_pct,
            _ => 1,
        };
        self.router.set_route(VersionRoute::canary(
            active,
            new_vid.clone(),
            canary_pct,
        ));

        Ok(UpdateResult::InProgress {
            version: new_vid,
            canary_pct,
        })
    }

    /// Advance the canary deployment to the next step.
    ///
    /// Checks health metrics and either advances or rolls back.
    pub fn advance(&self) -> Result<UpdateResult, String> {
        let active = self
            .active_version
            .read()
            .clone()
            .ok_or("no active version")?;

        match self.controller.advance_step(&self.health) {
            Ok(pct) => {
                let state = self.controller.state();
                match state {
                    DeploymentState::Completed { version } => {
                        // Full rollout: switch all traffic and drain old version
                        self.router
                            .set_route(VersionRoute::single(version.clone()));
                        *self.active_version.write() = Some(version.clone());
                        self.gc_old_versions();
                        let duration = self
                            .update_started_at
                            .lock()
                            .map(|s| s.elapsed())
                            .unwrap_or_default();
                        Ok(UpdateResult::Completed { version, duration })
                    }
                    DeploymentState::InProgress {
                        target_version,
                        canary_pct,
                        ..
                    } => {
                        self.router.set_route(VersionRoute::canary(
                            active,
                            target_version.clone(),
                            canary_pct,
                        ));
                        Ok(UpdateResult::InProgress {
                            version: target_version,
                            canary_pct: pct,
                        })
                    }
                    _ => Err("unexpected state after advance".into()),
                }
            }
            Err(e) => {
                // Rollback: restore traffic to active version
                self.router
                    .set_route(VersionRoute::single(active));
                match self.controller.state() {
                    DeploymentState::RolledBack {
                        failed_version,
                        reason,
                    } => Ok(UpdateResult::RolledBack {
                        version: failed_version,
                        reason,
                    }),
                    _ => Err(e),
                }
            }
        }
    }

    /// Force an immediate rollback of the current deployment.
    pub fn force_rollback(&self, reason: impl Into<String>) -> Result<UpdateResult, String> {
        let active = self
            .active_version
            .read()
            .clone()
            .ok_or("no active version")?;

        self.controller.rollback(reason);
        self.router
            .set_route(VersionRoute::single(active));

        match self.controller.state() {
            DeploymentState::RolledBack {
                failed_version,
                reason,
            } => Ok(UpdateResult::RolledBack {
                version: failed_version,
                reason,
            }),
            _ => Err("rollback failed: no deployment in progress".into()),
        }
    }

    /// Resolve which version should handle the next request.
    /// Also acquires a connection slot for the resolved version.
    pub fn resolve_and_acquire(&self) -> Option<(VersionId, ConnectionGuard<'_>)> {
        let vid = self.router.resolve()?;
        let guard = self.connections.acquire(&vid);
        Some((vid, guard))
    }

    /// Record a successful execution for health tracking.
    pub fn record_success(&self, version_id: &VersionId, latency_ms: u64) {
        self.health.record_success(version_id, latency_ms);
    }

    /// Record a failed execution for health tracking.
    pub fn record_failure(&self, version_id: &VersionId, latency_ms: u64) {
        self.health.record_failure(version_id, latency_ms);
    }

    /// Wait for a version to drain all active connections, up to drain_timeout.
    pub fn wait_for_drain(&self, version_id: &VersionId) -> bool {
        let start = Instant::now();
        while start.elapsed() < self.config.drain_timeout {
            if self.connections.is_drained(version_id) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// Get the currently active version ID.
    pub fn active_version(&self) -> Option<VersionId> {
        self.active_version.read().clone()
    }

    /// Get a reference to the version registry.
    pub fn registry(&self) -> &VersionRegistry {
        &self.registry
    }

    /// Get the deployment events log.
    pub fn events(&self) -> Vec<DeploymentEvent> {
        self.controller.events()
    }

    /// Get the current deployment state.
    pub fn deployment_state(&self) -> DeploymentState {
        self.controller.state()
    }

    /// Get the connection tracker for external use.
    pub fn connection_tracker(&self) -> &ConnectionTracker {
        &self.connections
    }

    /// Remove old versions that exceed max_retained_versions.
    fn gc_old_versions(&self) {
        let active = self.active_version.read().clone();
        let versions = self.registry.list();
        if versions.len() > self.config.max_retained_versions {
            let mut sorted = versions;
            sorted.sort_by_key(|v| v.created_at_epoch_ms);
            let to_remove = sorted.len() - self.config.max_retained_versions;
            for v in sorted.into_iter().take(to_remove) {
                // Never remove the active version
                if Some(&v.id) != active.as_ref() {
                    self.registry.remove(&v.id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_live_update_immediate() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Immediate,
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);
        assert_eq!(manager.active_version().unwrap(), VersionId::new("v1"));

        let v2 = ModuleVersion::new("v2", b"wasm-v2");
        let result = manager.start_update(v2).unwrap();
        assert!(matches!(result, UpdateResult::Completed { .. }));
        assert_eq!(manager.active_version().unwrap(), VersionId::new("v2"));
    }

    #[test]
    fn test_live_update_canary_flow() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Canary {
                steps: vec![10, 50, 100],
            },
            rollback_trigger: RollbackTrigger::error_rate(50.0),
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);

        let v2 = ModuleVersion::new("v2", b"wasm-v2");
        let result = manager.start_update(v2).unwrap();
        assert!(matches!(result, UpdateResult::InProgress { canary_pct: 10, .. }));

        // Advance through steps
        let result = manager.advance().unwrap();
        assert!(matches!(result, UpdateResult::InProgress { canary_pct: 50, .. }));

        let result = manager.advance().unwrap();
        assert!(matches!(result, UpdateResult::Completed { .. }));
        assert_eq!(manager.active_version().unwrap(), VersionId::new("v2"));
    }

    #[test]
    fn test_live_update_rollback_on_errors() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Canary {
                steps: vec![10, 50, 100],
            },
            rollback_trigger: RollbackTrigger::error_rate(5.0).with_min_requests(5),
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);

        let v2 = ModuleVersion::new("v2", b"wasm-v2");
        manager.start_update(v2).unwrap();

        // Simulate failures on v2
        let v2_id = VersionId::new("v2");
        for _ in 0..10 {
            manager.record_failure(&v2_id, 100);
        }

        let result = manager.advance().unwrap();
        assert!(matches!(result, UpdateResult::RolledBack { .. }));
        // Active version should remain v1
        assert_eq!(manager.active_version().unwrap(), VersionId::new("v1"));
    }

    #[test]
    fn test_connection_tracking() {
        let tracker = ConnectionTracker::new();
        let vid = VersionId::new("v1");

        assert!(tracker.is_drained(&vid));
        assert_eq!(tracker.active_count(&vid), 0);

        {
            let _g1 = tracker.acquire(&vid);
            let _g2 = tracker.acquire(&vid);
            assert_eq!(tracker.active_count(&vid), 2);
            assert!(!tracker.is_drained(&vid));
        }

        // Guards dropped
        assert!(tracker.is_drained(&vid));
    }

    #[test]
    fn test_resolve_and_acquire() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Immediate,
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);

        let (vid, guard) = manager.resolve_and_acquire().unwrap();
        assert_eq!(vid, VersionId::new("v1"));
        assert_eq!(manager.connection_tracker().active_count(&vid), 1);
        drop(guard);
        assert_eq!(manager.connection_tracker().active_count(&vid), 0);
    }

    #[test]
    fn test_force_rollback() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Canary {
                steps: vec![10, 50, 100],
            },
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);

        let v2 = ModuleVersion::new("v2", b"wasm-v2");
        manager.start_update(v2).unwrap();

        let result = manager.force_rollback("manual").unwrap();
        assert!(matches!(result, UpdateResult::RolledBack { .. }));
    }

    #[test]
    fn test_duplicate_update_rejected() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Canary {
                steps: vec![10, 50, 100],
            },
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);

        let v2 = ModuleVersion::new("v2", b"wasm-v2");
        manager.start_update(v2).unwrap();

        let v3 = ModuleVersion::new("v3", b"wasm-v3");
        let result = manager.start_update(v3);
        assert!(result.is_err());
    }

    #[test]
    fn test_gc_old_versions() {
        let config = LiveUpdateConfig {
            strategy: DeploymentStrategy::Immediate,
            max_retained_versions: 2,
            ..Default::default()
        };
        let manager = LiveUpdateManager::new(config);

        let v1 = ModuleVersion::new("v1", b"wasm-v1");
        manager.deploy_initial(v1);

        for i in 2..=5 {
            let v = ModuleVersion::new(format!("v{i}"), format!("wasm-v{i}").as_bytes());
            manager.start_update(v).unwrap();
        }

        assert!(manager.registry().count() <= 3); // active + max_retained
    }
}
