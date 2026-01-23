//! Kubernetes operator for managing Isolate sandboxes.
//!
//! This module provides the operator logic for reconciling Sandbox and
//! SandboxPool custom resources, managing sandbox lifecycle, and
//! integrating with Kubernetes APIs.

use super::{
    Condition, SandboxCrd, SandboxPhase, SandboxPoolCrd, SandboxPoolStatus,
    SandboxSpec, SandboxStatus,
};
use super::scheduler::{NodeResources, ResourceRequest, SandboxScheduler, SchedulingStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Operator configuration.
#[derive(Debug, Clone)]
pub struct OperatorConfig {
    /// Namespace to watch (None = all namespaces).
    pub namespace: Option<String>,
    /// Reconcile interval.
    pub reconcile_interval: Duration,
    /// Health check interval.
    pub health_check_interval: Duration,
    /// Maximum retries for failed operations.
    pub max_retries: u32,
    /// Backoff duration for retries.
    pub retry_backoff: Duration,
    /// Enable leader election.
    pub leader_election: bool,
    /// Leader election lease duration.
    pub lease_duration: Duration,
    /// Scheduling strategy.
    pub scheduling_strategy: SchedulingStrategy,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            namespace: None,
            reconcile_interval: Duration::from_secs(30),
            health_check_interval: Duration::from_secs(10),
            max_retries: 3,
            retry_backoff: Duration::from_secs(5),
            leader_election: true,
            lease_duration: Duration::from_secs(15),
            scheduling_strategy: SchedulingStrategy::LeastLoaded,
        }
    }
}

/// Operator state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorState {
    /// Operator is starting up.
    Starting,
    /// Operator is running and processing events.
    Running,
    /// Operator is the leader (if leader election is enabled).
    Leading,
    /// Operator is a standby (waiting for leadership).
    Standby,
    /// Operator is shutting down.
    ShuttingDown,
    /// Operator has stopped.
    Stopped,
}

/// Event emitted by the operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorEvent {
    /// Event type.
    pub event_type: EventType,
    /// Resource kind.
    pub kind: String,
    /// Resource name.
    pub name: String,
    /// Resource namespace.
    pub namespace: String,
    /// Event reason.
    pub reason: String,
    /// Event message.
    pub message: String,
    /// Timestamp.
    pub timestamp: String,
}

/// Event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    Normal,
    Warning,
}

/// Reconcile action returned by the operator.
#[derive(Debug, Clone)]
pub enum ReconcileAction {
    /// No action needed.
    None,
    /// Requeue after the specified duration.
    RequeueAfter(Duration),
    /// Requeue immediately.
    RequeueNow,
    /// Update the resource status.
    UpdateStatus(Box<SandboxStatus>),
    /// Create a new resource.
    Create(String),
    /// Delete the resource.
    Delete,
    /// Scale the resource.
    Scale { current: u32, desired: u32 },
}

/// The Isolate operator for Kubernetes.
pub struct IsolateOperator {
    config: OperatorConfig,
    state: OperatorState,
    scheduler: SandboxScheduler,
    sandboxes: HashMap<String, SandboxCrd>,
    pools: HashMap<String, SandboxPoolCrd>,
    events: Vec<OperatorEvent>,
    last_reconcile: Option<Instant>,
    reconcile_count: u64,
    error_count: u64,
}

impl IsolateOperator {
    /// Create a new operator.
    pub fn new(config: OperatorConfig) -> Self {
        Self {
            scheduler: SandboxScheduler::new(config.scheduling_strategy.clone()),
            config,
            state: OperatorState::Starting,
            sandboxes: HashMap::new(),
            pools: HashMap::new(),
            events: Vec::new(),
            last_reconcile: None,
            reconcile_count: 0,
            error_count: 0,
        }
    }

    /// Start the operator.
    pub fn start(&mut self) {
        self.state = OperatorState::Running;
        tracing::info!("Isolate operator started");
    }

    /// Stop the operator.
    pub fn stop(&mut self) {
        self.state = OperatorState::ShuttingDown;
        tracing::info!("Isolate operator stopping");
        self.state = OperatorState::Stopped;
    }

    /// Get operator state.
    pub fn state(&self) -> OperatorState {
        self.state
    }

    /// Update node resources for scheduling.
    pub fn update_nodes(&mut self, nodes: Vec<NodeResources>) {
        self.scheduler.update_nodes(nodes);
    }

    /// Reconcile a Sandbox resource.
    pub fn reconcile_sandbox(&mut self, sandbox: &mut SandboxCrd) -> ReconcileAction {
        self.reconcile_count += 1;
        self.last_reconcile = Some(Instant::now());

        let key = format!(
            "{}/{}",
            sandbox.metadata.namespace.as_deref().unwrap_or("default"),
            sandbox.metadata.name
        );

        // Initialize status if not present
        let status = sandbox.status.get_or_insert_with(SandboxStatus::default);

        let action = match status.phase {
            SandboxPhase::Pending => self.handle_pending(sandbox),
            SandboxPhase::Initializing => self.handle_initializing(sandbox),
            SandboxPhase::Running => self.handle_running(sandbox),
            SandboxPhase::Suspended => self.handle_suspended(sandbox),
            SandboxPhase::Failed => self.handle_failed(sandbox),
            SandboxPhase::Terminated => ReconcileAction::None,
        };

        // Store the sandbox
        self.sandboxes.insert(key, sandbox.clone());

        action
    }

    /// Reconcile a SandboxPool resource.
    pub fn reconcile_pool(&mut self, pool: &mut SandboxPoolCrd) -> ReconcileAction {
        self.reconcile_count += 1;
        self.last_reconcile = Some(Instant::now());

        let key = format!(
            "{}/{}",
            pool.metadata.namespace.as_deref().unwrap_or("default"),
            pool.metadata.name
        );

        // Initialize status if not present
        let status = pool.status.get_or_insert_with(SandboxPoolStatus::default);

        // Calculate desired state
        let desired_total = pool.spec.size;
        let desired_warm = pool.spec.warm_pool_size;
        let current_total = status.total;

        // Check if scaling is needed
        if current_total < desired_total {
            // Scale up
            self.emit_event(
                EventType::Normal,
                "SandboxPool",
                &pool.metadata.name,
                pool.metadata.namespace.as_deref().unwrap_or("default"),
                "ScalingUp",
                &format!("Scaling pool from {} to {} sandboxes", current_total, desired_total),
            );

            status.total = desired_total;
            status.ready = desired_total.min(status.ready + (desired_total - current_total));

            self.pools.insert(key, pool.clone());

            return ReconcileAction::Scale {
                current: current_total,
                desired: desired_total,
            };
        }

        if current_total > desired_total {
            // Scale down
            self.emit_event(
                EventType::Normal,
                "SandboxPool",
                &pool.metadata.name,
                pool.metadata.namespace.as_deref().unwrap_or("default"),
                "ScalingDown",
                &format!("Scaling pool from {} to {} sandboxes", current_total, desired_total),
            );

            status.total = desired_total;
            status.ready = status.ready.min(desired_total);

            self.pools.insert(key, pool.clone());

            return ReconcileAction::Scale {
                current: current_total,
                desired: desired_total,
            };
        }

        // Ensure warm pool is maintained
        if status.warm < desired_warm && status.in_use + status.warm < status.total {
            status.warm = desired_warm.min(status.total - status.in_use);
        }

        self.pools.insert(key, pool.clone());
        ReconcileAction::RequeueAfter(self.config.reconcile_interval)
    }

    fn handle_pending(&mut self, sandbox: &mut SandboxCrd) -> ReconcileAction {
        let status = sandbox.status.as_mut().unwrap();

        // Validate the spec
        if let Err(reason) = self.validate_spec(&sandbox.spec) {
            status.phase = SandboxPhase::Failed;
            status.message = Some(reason.clone());

            self.emit_event(
                EventType::Warning,
                "Sandbox",
                &sandbox.metadata.name,
                sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                "ValidationFailed",
                &reason,
            );

            self.error_count += 1;
            return ReconcileAction::UpdateStatus(Box::new(status.clone()));
        }

        // Try to schedule
        let request = self.build_resource_request(&sandbox.spec);

        if let Some(node_name) = self.scheduler.schedule(&request, None, &[]) {
            status.phase = SandboxPhase::Initializing;
            status.conditions.push(Condition {
                condition_type: "Scheduled".to_string(),
                status: "True".to_string(),
                last_transition_time: chrono_now(),
                reason: Some("SchedulingSucceeded".to_string()),
                message: Some(format!("Scheduled to node {}", node_name)),
            });

            self.emit_event(
                EventType::Normal,
                "Sandbox",
                &sandbox.metadata.name,
                sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                "Scheduled",
                &format!("Sandbox scheduled to node {}", node_name),
            );

            ReconcileAction::RequeueNow
        } else {
            // No node available
            status.message = Some("No schedulable node found".to_string());

            self.emit_event(
                EventType::Warning,
                "Sandbox",
                &sandbox.metadata.name,
                sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                "SchedulingFailed",
                "No node with sufficient resources available",
            );

            ReconcileAction::RequeueAfter(self.config.retry_backoff)
        }
    }

    fn handle_initializing(&mut self, sandbox: &mut SandboxCrd) -> ReconcileAction {
        let status = sandbox.status.as_mut().unwrap();

        // Simulate initialization (in a real operator, this would create pods/resources)
        status.phase = SandboxPhase::Running;
        status.ready_replicas = sandbox.spec.replicas;
        status.available_replicas = sandbox.spec.replicas;

        status.conditions.push(Condition {
            condition_type: "Ready".to_string(),
            status: "True".to_string(),
            last_transition_time: chrono_now(),
            reason: Some("SandboxReady".to_string()),
            message: Some("Sandbox is ready to accept requests".to_string()),
        });

        self.emit_event(
            EventType::Normal,
            "Sandbox",
            &sandbox.metadata.name,
            sandbox.metadata.namespace.as_deref().unwrap_or("default"),
            "Ready",
            "Sandbox is now ready",
        );

        ReconcileAction::UpdateStatus(Box::new(status.clone()))
    }

    fn handle_running(&mut self, sandbox: &mut SandboxCrd) -> ReconcileAction {
        // Get status info for scaling check first
        let (ready_replicas, desired_replicas) = {
            let status = sandbox.status.as_ref().unwrap();
            (status.ready_replicas, sandbox.spec.replicas)
        };

        // Check if scaling is needed
        if ready_replicas != desired_replicas {
            let status = sandbox.status.as_mut().unwrap();
            if ready_replicas < desired_replicas {
                self.emit_event(
                    EventType::Normal,
                    "Sandbox",
                    &sandbox.metadata.name,
                    sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                    "ScalingUp",
                    &format!(
                        "Scaling from {} to {} replicas",
                        ready_replicas, desired_replicas
                    ),
                );
            } else {
                self.emit_event(
                    EventType::Normal,
                    "Sandbox",
                    &sandbox.metadata.name,
                    sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                    "ScalingDown",
                    &format!(
                        "Scaling from {} to {} replicas",
                        ready_replicas, desired_replicas
                    ),
                );
            }

            status.ready_replicas = desired_replicas;
            status.available_replicas = desired_replicas;

            return ReconcileAction::Scale {
                current: ready_replicas,
                desired: desired_replicas,
            };
        }

        // Perform health check - get health check info before mutable borrow
        let health_check_clone = sandbox.spec.health_check.clone();
        if let Some(health_check) = &health_check_clone {
            let healthy = self.check_health(sandbox, health_check);
            if !healthy {
                let status = sandbox.status.as_mut().unwrap();
                status.conditions.push(Condition {
                    condition_type: "Healthy".to_string(),
                    status: "False".to_string(),
                    last_transition_time: chrono_now(),
                    reason: Some("HealthCheckFailed".to_string()),
                    message: Some("Health check failed".to_string()),
                });

                self.emit_event(
                    EventType::Warning,
                    "Sandbox",
                    &sandbox.metadata.name,
                    sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                    "Unhealthy",
                    "Sandbox failed health check",
                );
            }
        }

        ReconcileAction::RequeueAfter(self.config.health_check_interval)
    }

    fn handle_suspended(&mut self, _sandbox: &mut SandboxCrd) -> ReconcileAction {
        // Nothing to do for suspended sandboxes except monitor
        ReconcileAction::RequeueAfter(self.config.reconcile_interval)
    }

    fn handle_failed(&mut self, sandbox: &mut SandboxCrd) -> ReconcileAction {
        let status = sandbox.status.as_mut().unwrap();

        // Check if we should retry
        let retry_count = status
            .conditions
            .iter()
            .filter(|c| c.condition_type == "RetryAttempted")
            .count() as u32;

        if retry_count < self.config.max_retries {
            status.phase = SandboxPhase::Pending;
            status.conditions.push(Condition {
                condition_type: "RetryAttempted".to_string(),
                status: "True".to_string(),
                last_transition_time: chrono_now(),
                reason: Some(format!("RetryAttempt{}", retry_count + 1)),
                message: Some(format!("Retry attempt {} of {}", retry_count + 1, self.config.max_retries)),
            });

            self.emit_event(
                EventType::Normal,
                "Sandbox",
                &sandbox.metadata.name,
                sandbox.metadata.namespace.as_deref().unwrap_or("default"),
                "Retrying",
                &format!("Retry attempt {} of {}", retry_count + 1, self.config.max_retries),
            );

            return ReconcileAction::RequeueAfter(self.config.retry_backoff);
        }

        // Max retries exceeded
        ReconcileAction::None
    }

    fn validate_spec(&self, spec: &SandboxSpec) -> Result<(), String> {
        // Check module source
        if spec.module.image.is_none()
            && spec.module.config_map.is_none()
            && spec.module.inline.is_none()
            && spec.module.url.is_none()
        {
            return Err("No module source specified".to_string());
        }

        // Check replicas
        if spec.replicas == 0 {
            return Err("Replicas must be at least 1".to_string());
        }

        // Check scaling config
        if let Some(scaling) = &spec.scaling {
            if scaling.min_replicas > scaling.max_replicas {
                return Err("min_replicas cannot be greater than max_replicas".to_string());
            }
            if scaling.min_replicas == 0 {
                return Err("min_replicas must be at least 1".to_string());
            }
        }

        Ok(())
    }

    fn build_resource_request(&self, spec: &SandboxSpec) -> ResourceRequest {
        let mut request = ResourceRequest::new();

        if let Some(mem) = &spec.resources.limits.memory {
            if let Some(bytes) = ResourceRequest::parse_memory(mem) {
                request.memory_bytes = Some(bytes);
            }
        }

        if let Some(cpu) = &spec.resources.limits.cpu {
            if let Some(millicores) = ResourceRequest::parse_cpu(cpu) {
                request.cpu_millicores = Some(millicores);
            }
        }

        request.fuel = spec.resources.limits.fuel;

        request
    }

    fn check_health(&self, sandbox: &SandboxCrd, _health_check: &super::HealthCheck) -> bool {
        // In a real implementation, this would:
        // 1. Call the health check function if specified
        // 2. Make an HTTP request if http_get is specified
        // For now, we simulate success
        sandbox.status.as_ref().map_or(false, |s| s.ready_replicas > 0)
    }

    fn emit_event(
        &mut self,
        event_type: EventType,
        kind: &str,
        name: &str,
        namespace: &str,
        reason: &str,
        message: &str,
    ) {
        self.events.push(OperatorEvent {
            event_type,
            kind: kind.to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            reason: reason.to_string(),
            message: message.to_string(),
            timestamp: chrono_now(),
        });

        // Keep only recent events
        if self.events.len() > 1000 {
            self.events.drain(0..500);
        }
    }

    /// Get recent events.
    pub fn events(&self) -> &[OperatorEvent] {
        &self.events
    }

    /// Get operator metrics.
    pub fn metrics(&self) -> OperatorMetrics {
        OperatorMetrics {
            state: self.state,
            sandbox_count: self.sandboxes.len(),
            pool_count: self.pools.len(),
            node_count: self.scheduler.node_count(),
            reconcile_count: self.reconcile_count,
            error_count: self.error_count,
            last_reconcile: self.last_reconcile,
        }
    }
}

/// Operator metrics.
#[derive(Debug, Clone)]
pub struct OperatorMetrics {
    /// Current operator state.
    pub state: OperatorState,
    /// Number of managed sandboxes.
    pub sandbox_count: usize,
    /// Number of managed pools.
    pub pool_count: usize,
    /// Number of nodes in the scheduler.
    pub node_count: usize,
    /// Total reconcile operations.
    pub reconcile_count: u64,
    /// Total errors encountered.
    pub error_count: u64,
    /// Last reconcile time.
    pub last_reconcile: Option<Instant>,
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{}Z", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::k8s::{ModuleSource, ObjectMeta, PullPolicy, ResourceRequirements};

    fn create_test_sandbox(name: &str) -> SandboxCrd {
        SandboxCrd::new(
            name,
            "default",
            SandboxSpec {
                module: ModuleSource {
                    image: Some("ghcr.io/test/module:v1".to_string()),
                    config_map: None,
                    inline: None,
                    url: None,
                    pull_policy: PullPolicy::IfNotPresent,
                },
                resources: ResourceRequirements::default(),
                capabilities: vec![],
                env: vec![],
                secrets: vec![],
                timeout_seconds: Some(30),
                replicas: 1,
                scaling: None,
                health_check: None,
                service: None,
            },
        )
    }

    fn create_test_node(name: &str) -> NodeResources {
        NodeResources {
            name: name.to_string(),
            memory_total: 16 * 1024 * 1024 * 1024,
            memory_available: 8 * 1024 * 1024 * 1024,
            cpu_total: 8000,
            cpu_available: 4000,
            sandbox_count: 0,
            max_sandboxes: 100,
            labels: HashMap::new(),
            taints: Vec::new(),
            ready: true,
            schedulable: true,
        }
    }

    #[test]
    fn test_operator_creation() {
        let operator = IsolateOperator::new(OperatorConfig::default());
        assert_eq!(operator.state(), OperatorState::Starting);
    }

    #[test]
    fn test_operator_start_stop() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());

        operator.start();
        assert_eq!(operator.state(), OperatorState::Running);

        operator.stop();
        assert_eq!(operator.state(), OperatorState::Stopped);
    }

    #[test]
    fn test_reconcile_sandbox_pending() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());
        operator.start();

        // Add a node for scheduling
        operator.update_nodes(vec![create_test_node("node1")]);

        let mut sandbox = create_test_sandbox("test-sandbox");
        let action = operator.reconcile_sandbox(&mut sandbox);

        // Should transition to Initializing
        assert!(matches!(action, ReconcileAction::RequeueNow));
        assert_eq!(sandbox.status.unwrap().phase, SandboxPhase::Initializing);
    }

    #[test]
    fn test_reconcile_sandbox_no_node() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());
        operator.start();

        // No nodes available
        let mut sandbox = create_test_sandbox("test-sandbox");
        let action = operator.reconcile_sandbox(&mut sandbox);

        // Should requeue with backoff
        assert!(matches!(action, ReconcileAction::RequeueAfter(_)));
        assert_eq!(sandbox.status.unwrap().phase, SandboxPhase::Pending);
    }

    #[test]
    fn test_reconcile_sandbox_invalid_spec() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());
        operator.start();

        let mut sandbox = SandboxCrd::new(
            "test-sandbox",
            "default",
            SandboxSpec {
                module: ModuleSource {
                    image: None,
                    config_map: None,
                    inline: None,
                    url: None,
                    pull_policy: PullPolicy::IfNotPresent,
                },
                resources: ResourceRequirements::default(),
                capabilities: vec![],
                env: vec![],
                secrets: vec![],
                timeout_seconds: None,
                replicas: 1,
                scaling: None,
                health_check: None,
                service: None,
            },
        );

        let action = operator.reconcile_sandbox(&mut sandbox);

        // Should fail validation
        assert!(matches!(action, ReconcileAction::UpdateStatus(_)));
        assert_eq!(sandbox.status.unwrap().phase, SandboxPhase::Failed);
    }

    #[test]
    fn test_reconcile_pool_scaling() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());
        operator.start();

        let mut pool = SandboxPoolCrd {
            api_version: "isolate.io/v1alpha1".to_string(),
            kind: "SandboxPool".to_string(),
            metadata: ObjectMeta {
                name: "test-pool".to_string(),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: super::super::SandboxPoolSpec {
                template: create_test_sandbox("template").spec,
                size: 10,
                warm_pool_size: 5,
                max_idle_seconds: 300,
                preload: false,
            },
            status: Some(SandboxPoolStatus {
                total: 5,
                ready: 5,
                in_use: 2,
                warm: 3,
            }),
        };

        let action = operator.reconcile_pool(&mut pool);

        // Should scale up
        assert!(matches!(action, ReconcileAction::Scale { current: 5, desired: 10 }));
    }

    #[test]
    fn test_operator_metrics() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());
        operator.start();
        operator.update_nodes(vec![create_test_node("node1")]);

        let mut sandbox = create_test_sandbox("test-sandbox");
        operator.reconcile_sandbox(&mut sandbox);

        let metrics = operator.metrics();
        assert_eq!(metrics.state, OperatorState::Running);
        assert_eq!(metrics.sandbox_count, 1);
        assert_eq!(metrics.node_count, 1);
        assert!(metrics.reconcile_count > 0);
    }

    #[test]
    fn test_operator_events() {
        let mut operator = IsolateOperator::new(OperatorConfig::default());
        operator.start();
        operator.update_nodes(vec![create_test_node("node1")]);

        let mut sandbox = create_test_sandbox("test-sandbox");
        operator.reconcile_sandbox(&mut sandbox);

        let events = operator.events();
        assert!(!events.is_empty());
        assert!(events.iter().any(|e| e.reason == "Scheduled"));
    }
}
