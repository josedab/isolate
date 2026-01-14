//! Health checking system for mesh nodes.
//!
//! Provides periodic health monitoring of cluster nodes with configurable
//! thresholds for failure detection and recovery.

use super::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Health status of a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Node is healthy and responding normally.
    Healthy,
    /// Node is responding but with degraded performance.
    Degraded {
        /// Reason for degradation.
        reason: String,
    },
    /// Node is not reachable.
    Unreachable {
        /// When the node became unreachable.
        #[serde(skip)]
        since: Option<Instant>,
    },
}

impl HealthStatus {
    /// Returns true if the node is healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// Returns true if the node is reachable (healthy or degraded).
    pub fn is_reachable(&self) -> bool {
        !matches!(self, HealthStatus::Unreachable { .. })
    }
}

/// Health information for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealth {
    /// The node being tracked.
    pub node_id: NodeId,
    /// Current health status.
    pub status: HealthStatus,
    /// Last observed latency in milliseconds.
    pub latency_ms: f64,
    /// When the last health check was performed.
    #[serde(skip)]
    pub last_check: Option<Instant>,
    /// Number of consecutive check failures.
    pub consecutive_failures: u32,
    /// How long the node has been tracked.
    pub uptime: Duration,
}

impl NodeHealth {
    /// Create a new healthy node health record.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            status: HealthStatus::Healthy,
            latency_ms: 0.0,
            last_check: Some(Instant::now()),
            consecutive_failures: 0,
            uptime: Duration::ZERO,
        }
    }
}

/// Configuration for health checking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// How often to check node health.
    pub check_interval: Duration,
    /// Timeout for each health check.
    pub timeout: Duration,
    /// Number of consecutive failures before marking a node unreachable.
    pub failure_threshold: u32,
    /// Number of consecutive successes before marking a node recovered.
    pub recovery_threshold: u32,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(2),
            timeout: Duration::from_secs(5),
            failure_threshold: 3,
            recovery_threshold: 2,
        }
    }
}

/// Internal state for tracking a node's health over time.
#[derive(Debug, Clone)]
struct NodeHealthState {
    /// Current health record.
    health: NodeHealth,
    /// Number of consecutive successful checks (used for recovery).
    consecutive_successes: u32,
    /// When the node was first registered.
    registered_at: Instant,
}

/// Health checker that monitors node health via periodic pings.
pub struct HealthChecker {
    /// Health checking configuration.
    config: HealthConfig,
    /// Per-node health state.
    nodes: Arc<RwLock<HashMap<NodeId, NodeHealthState>>>,
    /// Total number of nodes required for quorum (majority).
    total_expected_nodes: Arc<RwLock<usize>>,
}

impl HealthChecker {
    /// Create a new health checker with the given configuration.
    pub fn new(config: HealthConfig) -> Self {
        Self {
            config,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            total_expected_nodes: Arc::new(RwLock::new(0)),
        }
    }

    /// Set the expected total number of nodes (used for quorum calculation).
    pub fn set_expected_nodes(&self, count: usize) {
        if let Ok(mut total) = self.total_expected_nodes.write() {
            *total = count;
        }
    }

    /// Register a node to be health-checked.
    pub fn register_node(&self, node_id: NodeId) {
        if let Ok(mut nodes) = self.nodes.write() {
            let now = Instant::now();
            nodes.insert(
                node_id,
                NodeHealthState {
                    health: NodeHealth {
                        node_id,
                        status: HealthStatus::Healthy,
                        latency_ms: 0.0,
                        last_check: Some(now),
                        consecutive_failures: 0,
                        uptime: Duration::ZERO,
                    },
                    consecutive_successes: 0,
                    registered_at: now,
                },
            );
        }
        // Update expected node count.
        if let Ok(nodes) = self.nodes.read() {
            self.set_expected_nodes(nodes.len());
        }
    }

    /// Unregister a node from health checking.
    pub fn unregister_node(&self, node_id: NodeId) {
        if let Ok(mut nodes) = self.nodes.write() {
            nodes.remove(&node_id);
        }
    }

    /// Get the current health of a node.
    pub fn check_node(&self, node_id: NodeId) -> NodeHealth {
        if let Ok(nodes) = self.nodes.read() {
            if let Some(state) = nodes.get(&node_id) {
                let mut health = state.health.clone();
                health.uptime = state.registered_at.elapsed();
                return health;
            }
        }
        // Return an unreachable status for unknown nodes.
        NodeHealth {
            node_id,
            status: HealthStatus::Unreachable { since: None },
            latency_ms: 0.0,
            last_check: None,
            consecutive_failures: 0,
            uptime: Duration::ZERO,
        }
    }

    /// Get all nodes currently considered healthy.
    pub fn all_healthy_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .read()
            .map(|nodes| {
                nodes
                    .iter()
                    .filter(|(_, state)| state.health.status.is_healthy())
                    .map(|(&id, _)| id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Record a failed health check for a node.
    pub fn mark_failed(&self, node_id: NodeId) {
        if let Ok(mut nodes) = self.nodes.write() {
            if let Some(state) = nodes.get_mut(&node_id) {
                state.health.consecutive_failures += 1;
                state.consecutive_successes = 0;
                state.health.last_check = Some(Instant::now());

                if state.health.consecutive_failures >= self.config.failure_threshold {
                    state.health.status = HealthStatus::Unreachable { since: Some(Instant::now()) };
                } else {
                    state.health.status = HealthStatus::Degraded {
                        reason: format!(
                            "health check failed ({}/{})",
                            state.health.consecutive_failures, self.config.failure_threshold
                        ),
                    };
                }
            }
        }
    }

    /// Record a successful health check for a node, potentially recovering it.
    pub fn mark_recovered(&self, node_id: NodeId) {
        if let Ok(mut nodes) = self.nodes.write() {
            if let Some(state) = nodes.get_mut(&node_id) {
                state.consecutive_successes += 1;
                state.health.last_check = Some(Instant::now());

                if state.consecutive_successes >= self.config.recovery_threshold {
                    state.health.status = HealthStatus::Healthy;
                    state.health.consecutive_failures = 0;
                }
            }
        }
    }

    /// Record a successful ping with latency.
    pub fn record_ping(&self, node_id: NodeId, latency_ms: f64) {
        if let Ok(mut nodes) = self.nodes.write() {
            if let Some(state) = nodes.get_mut(&node_id) {
                state.health.latency_ms = latency_ms;
                state.health.last_check = Some(Instant::now());
                state.consecutive_successes += 1;
                state.health.consecutive_failures = 0;

                if state.consecutive_successes >= self.config.recovery_threshold {
                    state.health.status = HealthStatus::Healthy;
                }
            }
        }
    }

    /// Check if a quorum of nodes is available (majority are healthy).
    pub fn is_quorum_available(&self) -> bool {
        let expected = self.total_expected_nodes.read().map(|n| *n).unwrap_or(0);
        if expected == 0 {
            return false;
        }
        let healthy_count = self.all_healthy_nodes().len();
        healthy_count > expected / 2
    }

    /// Get the health configuration.
    pub fn config(&self) -> &HealthConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_config_defaults() {
        let config = HealthConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(2));
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.recovery_threshold, 2);
    }

    #[test]
    fn test_register_and_check_node() {
        let checker = HealthChecker::new(HealthConfig::default());
        let node = NodeId::new(1);

        checker.register_node(node);
        let health = checker.check_node(node);

        assert!(health.status.is_healthy());
        assert_eq!(health.node_id, node);
        assert_eq!(health.consecutive_failures, 0);
    }

    #[test]
    fn test_mark_failed_transitions_to_degraded_then_unreachable() {
        let checker = HealthChecker::new(HealthConfig::default());
        let node = NodeId::new(1);
        checker.register_node(node);

        // First failure: degraded
        checker.mark_failed(node);
        let health = checker.check_node(node);
        assert!(matches!(health.status, HealthStatus::Degraded { .. }));
        assert_eq!(health.consecutive_failures, 1);

        // Second failure: still degraded
        checker.mark_failed(node);
        let health = checker.check_node(node);
        assert!(matches!(health.status, HealthStatus::Degraded { .. }));

        // Third failure (threshold=3): unreachable
        checker.mark_failed(node);
        let health = checker.check_node(node);
        assert!(matches!(health.status, HealthStatus::Unreachable { .. }));
        assert_eq!(health.consecutive_failures, 3);
    }

    #[test]
    fn test_mark_recovered_transitions_to_healthy() {
        let config = HealthConfig { recovery_threshold: 2, ..HealthConfig::default() };
        let checker = HealthChecker::new(config);
        let node = NodeId::new(1);
        checker.register_node(node);

        // Make node degraded first
        checker.mark_failed(node);
        assert!(!checker.check_node(node).status.is_healthy());

        // First recovery: not yet healthy
        checker.mark_recovered(node);
        // Second recovery: should be healthy
        checker.mark_recovered(node);
        assert!(checker.check_node(node).status.is_healthy());
    }

    #[test]
    fn test_all_healthy_nodes() {
        let checker = HealthChecker::new(HealthConfig::default());
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);
        let node3 = NodeId::new(3);

        checker.register_node(node1);
        checker.register_node(node2);
        checker.register_node(node3);

        assert_eq!(checker.all_healthy_nodes().len(), 3);

        // Fail node2 past threshold
        for _ in 0..3 {
            checker.mark_failed(node2);
        }

        let healthy = checker.all_healthy_nodes();
        assert_eq!(healthy.len(), 2);
        assert!(!healthy.contains(&node2));
    }

    #[test]
    fn test_quorum_available() {
        let checker = HealthChecker::new(HealthConfig::default());
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);
        let node3 = NodeId::new(3);

        // No nodes: no quorum
        assert!(!checker.is_quorum_available());

        checker.register_node(node1);
        checker.register_node(node2);
        checker.register_node(node3);

        // All healthy: quorum available (3/3 > 3/2)
        assert!(checker.is_quorum_available());

        // Fail one node: still have quorum (2/3 > 1)
        for _ in 0..3 {
            checker.mark_failed(node1);
        }
        assert!(checker.is_quorum_available());

        // Fail another: no quorum (1/3 <= 1)
        for _ in 0..3 {
            checker.mark_failed(node2);
        }
        assert!(!checker.is_quorum_available());
    }

    #[test]
    fn test_unknown_node_returns_unreachable() {
        let checker = HealthChecker::new(HealthConfig::default());
        let health = checker.check_node(NodeId::new(999));
        assert!(matches!(health.status, HealthStatus::Unreachable { .. }));
    }

    #[test]
    fn test_record_ping_updates_latency() {
        let checker = HealthChecker::new(HealthConfig::default());
        let node = NodeId::new(1);
        checker.register_node(node);

        checker.record_ping(node, 42.5);
        let health = checker.check_node(node);
        assert!((health.latency_ms - 42.5).abs() < f64::EPSILON);
        assert!(health.status.is_healthy());
    }

    #[test]
    fn test_unregister_node() {
        let checker = HealthChecker::new(HealthConfig::default());
        let node = NodeId::new(1);

        checker.register_node(node);
        assert_eq!(checker.all_healthy_nodes().len(), 1);

        checker.unregister_node(node);
        assert_eq!(checker.all_healthy_nodes().len(), 0);
    }
}
