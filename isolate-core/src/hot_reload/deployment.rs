use super::health::HealthTracker;
use super::version::VersionId;
use serde::{Deserialize, Serialize};

/// Strategy for rolling out a new module version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentStrategy {
    /// Immediate: switch 100% traffic instantly.
    Immediate,
    /// Canary: gradually increase traffic percentage through steps.
    Canary { steps: Vec<u8> },
    /// Blue-green: maintain two parallel environments.
    BlueGreen,
}

impl Default for DeploymentStrategy {
    fn default() -> Self {
        Self::Canary {
            steps: vec![1, 5, 10, 25, 50, 100],
        }
    }
}

/// Conditions that trigger an automatic rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackTrigger {
    pub max_error_rate_pct: f64,
    pub max_latency_ms: Option<u64>,
    pub min_requests_before_eval: u64,
}

impl RollbackTrigger {
    pub fn error_rate(pct: f64) -> Self {
        Self {
            max_error_rate_pct: pct,
            max_latency_ms: None,
            min_requests_before_eval: 10,
        }
    }

    pub fn with_latency(mut self, max_ms: u64) -> Self {
        self.max_latency_ms = Some(max_ms);
        self
    }

    pub fn with_min_requests(mut self, min: u64) -> Self {
        self.min_requests_before_eval = min;
        self
    }
}

/// Current state of a deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentState {
    Idle,
    InProgress {
        target_version: VersionId,
        current_step: usize,
        canary_pct: u8,
    },
    RolledBack {
        failed_version: VersionId,
        reason: String,
    },
    Completed {
        version: VersionId,
    },
}

/// Event emitted during deployment lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentEvent {
    Started { version: VersionId },
    StepAdvanced { version: VersionId, pct: u8 },
    Completed { version: VersionId },
    RolledBack { version: VersionId, reason: String },
}

/// Controller managing the deployment lifecycle of module versions.
pub struct DeploymentController {
    strategy: DeploymentStrategy,
    trigger: RollbackTrigger,
    state: parking_lot::RwLock<DeploymentState>,
    events: parking_lot::Mutex<Vec<DeploymentEvent>>,
}

impl DeploymentController {
    pub fn new(strategy: DeploymentStrategy, trigger: RollbackTrigger) -> Self {
        Self {
            strategy,
            trigger,
            state: parking_lot::RwLock::new(DeploymentState::Idle),
            events: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub fn state(&self) -> DeploymentState {
        self.state.read().clone()
    }

    /// Start deploying a new version.
    pub fn start_deployment(&self, version: VersionId) {
        let initial_pct = match &self.strategy {
            DeploymentStrategy::Immediate => 100,
            DeploymentStrategy::Canary { steps } => steps.first().copied().unwrap_or(100),
            DeploymentStrategy::BlueGreen => 0,
        };

        *self.state.write() = DeploymentState::InProgress {
            target_version: version.clone(),
            current_step: 0,
            canary_pct: initial_pct,
        };

        self.emit(DeploymentEvent::Started {
            version: version.clone(),
        });

        if initial_pct == 100 {
            *self.state.write() = DeploymentState::Completed {
                version: version.clone(),
            };
            self.emit(DeploymentEvent::Completed { version });
        }
    }

    /// Advance to the next canary step, checking health first.
    pub fn advance_step(&self, health: &HealthTracker) -> Result<u8, String> {
        let mut state = self.state.write();
        match &*state {
            DeploymentState::InProgress {
                target_version,
                current_step,
                ..
            } => {
                // Check health before advancing
                if let Some(health_data) = health.get_health(target_version) {
                    if health_data.total_requests >= self.trigger.min_requests_before_eval
                        && health_data.error_rate() > self.trigger.max_error_rate_pct
                    {
                        let reason = format!(
                            "error rate {:.1}% exceeds threshold {:.1}%",
                            health_data.error_rate(),
                            self.trigger.max_error_rate_pct
                        );
                        let version = target_version.clone();
                        *state = DeploymentState::RolledBack {
                            failed_version: version.clone(),
                            reason: reason.clone(),
                        };
                        drop(state);
                        self.emit(DeploymentEvent::RolledBack { version, reason });
                        return Err("rollback triggered".into());
                    }

                    if let Some(max_lat) = self.trigger.max_latency_ms {
                        if health_data.avg_latency_ms() > max_lat as f64 {
                            let reason = format!(
                                "avg latency {:.0}ms exceeds max {}ms",
                                health_data.avg_latency_ms(),
                                max_lat
                            );
                            let version = target_version.clone();
                            *state = DeploymentState::RolledBack {
                                failed_version: version.clone(),
                                reason: reason.clone(),
                            };
                            drop(state);
                            self.emit(DeploymentEvent::RolledBack { version, reason });
                            return Err("rollback triggered".into());
                        }
                    }
                }

                let next_step = current_step + 1;
                let steps = match &self.strategy {
                    DeploymentStrategy::Canary { steps } => steps.clone(),
                    _ => vec![100],
                };

                let next_pct = if next_step < steps.len() {
                    steps[next_step]
                } else {
                    100
                };

                let version = target_version.clone();

                if next_pct >= 100 {
                    *state = DeploymentState::Completed {
                        version: version.clone(),
                    };
                    drop(state);
                    self.emit(DeploymentEvent::Completed { version });
                    Ok(100)
                } else {
                    *state = DeploymentState::InProgress {
                        target_version: version.clone(),
                        current_step: next_step,
                        canary_pct: next_pct,
                    };
                    drop(state);
                    self.emit(DeploymentEvent::StepAdvanced {
                        version,
                        pct: next_pct,
                    });
                    Ok(next_pct)
                }
            }
            _ => Err("no deployment in progress".into()),
        }
    }

    /// Force rollback to previous version.
    pub fn rollback(&self, reason: impl Into<String>) {
        let mut state = self.state.write();
        if let DeploymentState::InProgress {
            target_version, ..
        } = &*state
        {
            let version = target_version.clone();
            let reason = reason.into();
            *state = DeploymentState::RolledBack {
                failed_version: version.clone(),
                reason: reason.clone(),
            };
            drop(state);
            self.emit(DeploymentEvent::RolledBack { version, reason });
        }
    }

    /// Get all deployment events.
    pub fn events(&self) -> Vec<DeploymentEvent> {
        self.events.lock().clone()
    }

    fn emit(&self, event: DeploymentEvent) {
        self.events.lock().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_immediate_deployment() {
        let ctrl =
            DeploymentController::new(DeploymentStrategy::Immediate, RollbackTrigger::error_rate(5.0));

        ctrl.start_deployment(VersionId::new("v2"));
        assert!(matches!(ctrl.state(), DeploymentState::Completed { .. }));
    }

    #[test]
    fn test_canary_deployment_steps() {
        let ctrl = DeploymentController::new(
            DeploymentStrategy::Canary {
                steps: vec![1, 10, 50, 100],
            },
            RollbackTrigger::error_rate(50.0),
        );

        ctrl.start_deployment(VersionId::new("v2"));
        let health = HealthTracker::new();

        // Advance through steps
        let pct = ctrl.advance_step(&health).unwrap();
        assert_eq!(pct, 10);

        let pct = ctrl.advance_step(&health).unwrap();
        assert_eq!(pct, 50);

        let pct = ctrl.advance_step(&health).unwrap();
        assert_eq!(pct, 100);
        assert!(matches!(ctrl.state(), DeploymentState::Completed { .. }));
    }

    #[test]
    fn test_canary_rollback_on_errors() {
        let ctrl = DeploymentController::new(
            DeploymentStrategy::Canary {
                steps: vec![1, 10, 50, 100],
            },
            RollbackTrigger::error_rate(5.0).with_min_requests(5),
        );

        ctrl.start_deployment(VersionId::new("v2"));

        let health = HealthTracker::new();
        let vid = VersionId::new("v2");
        // Simulate high error rate
        for _ in 0..10 {
            health.record_failure(&vid, 100);
        }

        let result = ctrl.advance_step(&health);
        assert!(result.is_err());
        assert!(matches!(ctrl.state(), DeploymentState::RolledBack { .. }));
    }

    #[test]
    fn test_manual_rollback() {
        let ctrl = DeploymentController::new(
            DeploymentStrategy::Canary {
                steps: vec![1, 50, 100],
            },
            RollbackTrigger::error_rate(5.0),
        );

        ctrl.start_deployment(VersionId::new("v2"));
        ctrl.rollback("manual intervention");

        let DeploymentState::RolledBack { reason, .. } = ctrl.state() else {
            unreachable!("expected RolledBack");
        };
        assert_eq!(reason, "manual intervention");
    }

    #[test]
    fn test_deployment_events() {
        let ctrl = DeploymentController::new(DeploymentStrategy::Immediate, RollbackTrigger::error_rate(5.0));

        ctrl.start_deployment(VersionId::new("v2"));
        let events = ctrl.events();
        assert_eq!(events.len(), 2); // Started + Completed
    }

    #[test]
    fn test_latency_rollback() {
        let ctrl = DeploymentController::new(
            DeploymentStrategy::Canary {
                steps: vec![10, 50, 100],
            },
            RollbackTrigger::error_rate(50.0)
                .with_latency(100)
                .with_min_requests(3),
        );

        ctrl.start_deployment(VersionId::new("v2"));

        let health = HealthTracker::new();
        let vid = VersionId::new("v2");
        for _ in 0..5 {
            health.record_success(&vid, 200); // avg = 200ms > 100ms threshold
        }

        let result = ctrl.advance_step(&health);
        assert!(result.is_err());
    }
}
