//! # Hot Reload & Zero-Downtime Updates
//!
//! Module versioning with canary deployments and automatic rollback.
//! Enables updating running sandbox pools without dropping requests.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐     ┌────────────────┐     ┌──────────────┐
//! │ ModuleVersion│────▶│ VersionRouter  │────▶│ HealthTracker│
//! │   Registry   │     │  (weighted)    │     │  (per-ver)   │
//! └──────────────┘     └────────────────┘     └──────────────┘
//!                            │
//!                            ▼
//!                      ┌────────────────┐
//!                      │ DeploymentCtrl │
//!                      │  (canary/roll) │
//!                      └────────────────┘
//! ```

#![allow(missing_docs)]
mod deployment;
mod health;
mod live_update;
mod version;

pub use deployment::{
    DeploymentController, DeploymentEvent, DeploymentState, DeploymentStrategy, RollbackTrigger,
};
pub use health::{HealthTracker, VersionHealth};
pub use live_update::{
    ConnectionGuard, ConnectionTracker, LiveUpdateConfig, LiveUpdateManager, UpdateResult,
};
pub use version::{ModuleVersion, VersionId, VersionRegistry, VersionRoute, VersionRouter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_canary_deployment() {
        let registry = VersionRegistry::new();

        // Deploy v1 at 100%
        let v1 = ModuleVersion::new("v1", b"wasm-v1-bytes");
        registry.register(v1.clone());

        let router = VersionRouter::new();
        router.set_route(VersionRoute::single(v1.id.clone()));
        assert_eq!(router.resolve().unwrap(), v1.id);

        // Start canary for v2
        let v2 = ModuleVersion::new("v2", b"wasm-v2-bytes");
        registry.register(v2.clone());

        router.set_route(VersionRoute::canary(v1.id.clone(), v2.id.clone(), 10));
        let resolved = router.resolve().unwrap();
        assert!(resolved == v1.id || resolved == v2.id);
    }

    #[test]
    fn test_deployment_controller_lifecycle() {
        let ctrl = DeploymentController::new(
            DeploymentStrategy::Canary { steps: vec![1, 10, 50, 100] },
            RollbackTrigger::error_rate(5.0),
        );

        assert!(matches!(ctrl.state(), DeploymentState::Idle));
        ctrl.start_deployment(VersionId::new("v2"));
        assert!(matches!(ctrl.state(), DeploymentState::InProgress { .. }));
    }
}
