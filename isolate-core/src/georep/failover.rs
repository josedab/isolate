use super::region::{RegionId, RegionRegistry, RegionStatus};
use serde::{Deserialize, Serialize};

/// Policy for automatic failover behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverPolicy {
    /// Number of consecutive failures before triggering failover.
    pub failure_threshold: u32,
    /// Whether to allow automatic failover (vs manual only).
    pub auto_failover: bool,
    /// Whether to fail back automatically when primary recovers.
    pub auto_failback: bool,
}

impl Default for FailoverPolicy {
    fn default() -> Self {
        Self { failure_threshold: 3, auto_failover: true, auto_failback: false }
    }
}

/// Event emitted during failover operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailoverEvent {
    FailoverTriggered { from: RegionId, to: RegionId, reason: String },
    FailbackCompleted { from: RegionId, to: RegionId },
    ManualOverride { new_primary: RegionId },
}

/// Controller for managing failover between regions.
pub struct FailoverController {
    policy: FailoverPolicy,
    current_primary: parking_lot::RwLock<Option<RegionId>>,
    events: parking_lot::Mutex<Vec<FailoverEvent>>,
}

impl FailoverController {
    pub fn new(policy: FailoverPolicy) -> Self {
        Self {
            policy,
            current_primary: parking_lot::RwLock::new(None),
            events: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// Set the initial primary region.
    pub fn set_primary(&self, region: RegionId) {
        *self.current_primary.write() = Some(region);
    }

    /// Get the current primary region.
    pub fn primary(&self) -> Option<RegionId> {
        self.current_primary.read().clone()
    }

    /// Evaluate whether failover is needed based on region health.
    pub fn evaluate(&self, registry: &RegionRegistry) -> Option<FailoverEvent> {
        if !self.policy.auto_failover {
            return None;
        }

        let primary = self.current_primary.read().clone()?;
        let health = registry.get_health(&primary)?;

        if health.consecutive_failures >= self.policy.failure_threshold {
            // Find best healthy replica to fail over to
            let candidates = registry.list_healthy();
            let new_primary = candidates
                .iter()
                .find(|r| r.id != primary && matches!(r.status, RegionStatus::Healthy))?;

            let event = FailoverEvent::FailoverTriggered {
                from: primary,
                to: new_primary.id.clone(),
                reason: format!(
                    "{} consecutive failures exceeded threshold {}",
                    health.consecutive_failures, self.policy.failure_threshold
                ),
            };

            *self.current_primary.write() = Some(new_primary.id.clone());
            self.events.lock().push(event.clone());
            Some(event)
        } else {
            None
        }
    }

    /// Manually force failover to a specific region.
    pub fn force_failover(&self, new_primary: RegionId) {
        *self.current_primary.write() = Some(new_primary.clone());
        self.events.lock().push(FailoverEvent::ManualOverride { new_primary });
    }

    /// Get all failover events.
    pub fn events(&self) -> Vec<FailoverEvent> {
        self.events.lock().clone()
    }
}

impl Default for FailoverController {
    fn default() -> Self {
        Self::new(FailoverPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::georep::region::RegionInfo;

    fn setup_registry() -> RegionRegistry {
        let reg = RegionRegistry::new();
        reg.register(RegionInfo::new("us-east-1", true));
        reg.register(RegionInfo::new("eu-west-1", false));
        reg.register(RegionInfo::new("ap-southeast-1", false));
        reg
    }

    #[test]
    fn test_normal_operation_no_failover() {
        let reg = setup_registry();
        let ctrl = FailoverController::new(FailoverPolicy::default());
        ctrl.set_primary(RegionId::new("us-east-1"));

        reg.record_heartbeat(&RegionId::new("us-east-1"), 10);
        assert!(ctrl.evaluate(&reg).is_none());
    }

    #[test]
    fn test_auto_failover() {
        let reg = setup_registry();
        let ctrl = FailoverController::new(FailoverPolicy {
            failure_threshold: 3,
            auto_failover: true,
            auto_failback: false,
        });
        ctrl.set_primary(RegionId::new("us-east-1"));

        // Simulate failures
        let id = RegionId::new("us-east-1");
        for _ in 0..3 {
            reg.record_failure(&id);
        }

        let event = ctrl.evaluate(&reg).unwrap();
        assert!(matches!(event, FailoverEvent::FailoverTriggered { .. }));

        // Primary should have changed
        let new_primary = ctrl.primary().unwrap();
        assert_ne!(new_primary.as_str(), "us-east-1");
    }

    #[test]
    fn test_manual_failover() {
        let ctrl = FailoverController::new(FailoverPolicy::default());
        ctrl.set_primary(RegionId::new("us-east-1"));
        ctrl.force_failover(RegionId::new("eu-west-1"));

        assert_eq!(ctrl.primary().unwrap().as_str(), "eu-west-1");
        assert_eq!(ctrl.events().len(), 1);
    }

    #[test]
    fn test_disabled_auto_failover() {
        let reg = setup_registry();
        let ctrl =
            FailoverController::new(FailoverPolicy { auto_failover: false, ..Default::default() });
        ctrl.set_primary(RegionId::new("us-east-1"));

        let id = RegionId::new("us-east-1");
        for _ in 0..5 {
            reg.record_failure(&id);
        }

        // Auto failover disabled, should return None
        assert!(ctrl.evaluate(&reg).is_none());
    }

    #[test]
    fn test_no_primary_set() {
        let reg = setup_registry();
        let ctrl = FailoverController::new(FailoverPolicy::default());
        assert!(ctrl.evaluate(&reg).is_none());
    }
}
