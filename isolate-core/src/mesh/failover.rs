//! Failover coordination for mesh nodes.
//!
//! Detects node failures and coordinates sandbox reassignment to maintain
//! availability across the cluster.

use super::NodeId;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Events emitted during the failover process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailoverEvent {
    /// A node has failed and its sandboxes need reassignment.
    NodeFailed {
        /// The failed node.
        node_id: NodeId,
        /// Sandboxes that were running on the failed node.
        affected_sandboxes: Vec<String>,
    },
    /// A sandbox has been reassigned to a different node.
    SandboxReassigned {
        /// The sandbox that was moved.
        sandbox_id: String,
        /// The node it was moved from.
        from_node: NodeId,
        /// The node it was moved to.
        to_node: NodeId,
    },
    /// Failover for a node has completed.
    FailoverComplete {
        /// The failed node that was handled.
        node_id: NodeId,
        /// Number of sandboxes successfully reassigned.
        reassigned: usize,
        /// Number of sandboxes that could not be reassigned.
        failed: usize,
    },
    /// Quorum has been lost; cluster cannot safely operate.
    QuorumLost,
}

/// Policy controlling how failovers are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailoverPolicy {
    /// Automatically reassign sandboxes on failure.
    Automatic,
    /// Require manual intervention to reassign sandboxes.
    Manual,
    /// Only drain sandboxes from the failed node, do not reassign.
    DrainOnly,
}

impl Default for FailoverPolicy {
    fn default() -> Self {
        FailoverPolicy::Automatic
    }
}

/// State of an in-progress failover.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverState {
    /// The failed node being handled.
    pub failed_node: NodeId,
    /// Sandboxes that still need reassignment.
    pub pending_sandboxes: Vec<String>,
    /// Sandboxes that were successfully reassigned.
    pub reassigned_sandboxes: Vec<String>,
    /// Sandboxes that failed to be reassigned.
    pub failed_sandboxes: Vec<String>,
    /// When the failover was initiated.
    #[serde(skip)]
    pub started_at: Option<Instant>,
    /// Whether the failover has been completed.
    pub completed: bool,
}

impl FailoverState {
    /// Create a new failover state for a failed node.
    fn new(failed_node: NodeId, sandboxes: Vec<String>) -> Self {
        Self {
            failed_node,
            pending_sandboxes: sandboxes,
            reassigned_sandboxes: Vec::new(),
            failed_sandboxes: Vec::new(),
            started_at: Some(Instant::now()),
            completed: false,
        }
    }

    /// Returns true if the failover is still in progress.
    pub fn is_active(&self) -> bool {
        !self.completed
    }
}

/// Coordinates failover when nodes fail in the mesh.
pub struct FailoverCoordinator {
    /// Failover policy.
    policy: FailoverPolicy,
    /// Active failover states keyed by failed node.
    active_failovers: Arc<RwLock<HashMap<NodeId, FailoverState>>>,
    /// Available target nodes for reassignment.
    available_nodes: Arc<RwLock<Vec<NodeId>>>,
    /// Mapping of sandbox to its current node assignment.
    sandbox_assignments: Arc<RwLock<HashMap<String, NodeId>>>,
}

impl FailoverCoordinator {
    /// Create a new failover coordinator with the given policy.
    pub fn new(policy: FailoverPolicy) -> Self {
        Self {
            policy,
            active_failovers: Arc::new(RwLock::new(HashMap::new())),
            available_nodes: Arc::new(RwLock::new(Vec::new())),
            sandbox_assignments: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set the list of available nodes for failover targets.
    pub fn set_available_nodes(&self, nodes: Vec<NodeId>) {
        if let Ok(mut available) = self.available_nodes.write() {
            *available = nodes;
        }
    }

    /// Register a sandbox assignment.
    pub fn register_sandbox(&self, sandbox_id: String, node_id: NodeId) {
        if let Ok(mut assignments) = self.sandbox_assignments.write() {
            assignments.insert(sandbox_id, node_id);
        }
    }

    /// Initiate failover for a failed node.
    ///
    /// Identifies affected sandboxes and, if the policy allows, reassigns them
    /// to available healthy nodes.
    pub fn initiate_failover(&self, failed_node: NodeId) -> Result<FailoverEvent> {
        // Check if there's already an active failover for this node.
        {
            let failovers = self
                .active_failovers
                .read()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            if let Some(state) = failovers.get(&failed_node) {
                if state.is_active() {
                    return Err(Error::Engine(format!(
                        "Failover already in progress for node {}",
                        failed_node
                    )));
                }
            }
        }

        // Find sandboxes on the failed node.
        let affected_sandboxes: Vec<String> = self
            .sandbox_assignments
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?
            .iter()
            .filter(|(_, &node)| node == failed_node)
            .map(|(id, _)| id.clone())
            .collect();

        let mut failover_state = FailoverState::new(failed_node, affected_sandboxes.clone());

        match self.policy {
            FailoverPolicy::Automatic => {
                let available = self
                    .available_nodes
                    .read()
                    .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

                let targets: Vec<NodeId> =
                    available.iter().filter(|&&n| n != failed_node).copied().collect();

                if targets.is_empty() {
                    // No nodes available; mark all as failed.
                    failover_state.failed_sandboxes = failover_state.pending_sandboxes.clone();
                    failover_state.pending_sandboxes.clear();
                    failover_state.completed = true;
                } else {
                    // Round-robin reassignment across available targets.
                    let mut assignments = self
                        .sandbox_assignments
                        .write()
                        .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

                    let pending: Vec<String> = failover_state.pending_sandboxes.drain(..).collect();
                    for (i, sandbox_id) in pending.iter().enumerate() {
                        let target = targets[i % targets.len()];
                        assignments.insert(sandbox_id.clone(), target);
                        failover_state.reassigned_sandboxes.push(sandbox_id.clone());
                    }
                    failover_state.completed = true;
                }
            }
            FailoverPolicy::Manual => {
                // Leave sandboxes in pending state for manual intervention.
            }
            FailoverPolicy::DrainOnly => {
                // Remove sandbox assignments but don't reassign.
                let mut assignments = self
                    .sandbox_assignments
                    .write()
                    .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
                for sandbox_id in &failover_state.pending_sandboxes {
                    assignments.remove(sandbox_id);
                }
                failover_state.pending_sandboxes.clear();
                failover_state.completed = true;
            }
        }

        let reassigned = failover_state.reassigned_sandboxes.len();
        let failed_count = failover_state.failed_sandboxes.len();

        // Store the failover state.
        {
            let mut failovers = self
                .active_failovers
                .write()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            failovers.insert(failed_node, failover_state);
        }

        if affected_sandboxes.is_empty() {
            Ok(FailoverEvent::FailoverComplete { node_id: failed_node, reassigned: 0, failed: 0 })
        } else {
            Ok(FailoverEvent::FailoverComplete {
                node_id: failed_node,
                reassigned,
                failed: failed_count,
            })
        }
    }

    /// Cancel an in-progress failover.
    pub fn cancel_failover(&self, node_id: NodeId) -> Result<()> {
        let mut failovers = self
            .active_failovers
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        match failovers.get_mut(&node_id) {
            Some(state) if state.is_active() => {
                state.completed = true;
                Ok(())
            }
            Some(_) => {
                Err(Error::Engine(format!("Failover for node {} is already completed", node_id)))
            }
            None => Err(Error::Engine(format!("No failover found for node {}", node_id))),
        }
    }

    /// Get the current status of all failovers.
    pub fn failover_status(&self) -> Vec<FailoverState> {
        self.active_failovers.read().map(|f| f.values().cloned().collect()).unwrap_or_default()
    }

    /// Get the failover policy.
    pub fn policy(&self) -> FailoverPolicy {
        self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_coordinator(policy: FailoverPolicy) -> FailoverCoordinator {
        let coord = FailoverCoordinator::new(policy);
        coord.set_available_nodes(vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]);
        coord.register_sandbox("sb-1".to_string(), NodeId::new(1));
        coord.register_sandbox("sb-2".to_string(), NodeId::new(1));
        coord.register_sandbox("sb-3".to_string(), NodeId::new(2));
        coord
    }

    #[test]
    fn test_automatic_failover_reassigns_sandboxes() {
        let coord = setup_coordinator(FailoverPolicy::Automatic);

        let event = coord.initiate_failover(NodeId::new(1)).unwrap();
        let FailoverEvent::FailoverComplete { node_id, reassigned, failed } = event else {
            unreachable!("Expected FailoverComplete event");
        };
        assert_eq!(node_id, NodeId::new(1));
        assert_eq!(reassigned, 2);
        assert_eq!(failed, 0);
    }

    #[test]
    fn test_manual_failover_leaves_pending() {
        let coord = setup_coordinator(FailoverPolicy::Manual);

        let _event = coord.initiate_failover(NodeId::new(1)).unwrap();
        let statuses = coord.failover_status();

        assert_eq!(statuses.len(), 1);
        let state = &statuses[0];
        assert_eq!(state.failed_node, NodeId::new(1));
        // Manual policy: sandboxes remain pending.
        assert!(!state.pending_sandboxes.is_empty());
        assert!(!state.completed);
    }

    #[test]
    fn test_drain_only_removes_assignments() {
        let coord = setup_coordinator(FailoverPolicy::DrainOnly);

        let event = coord.initiate_failover(NodeId::new(1)).unwrap();
        let FailoverEvent::FailoverComplete { reassigned, failed, .. } = event else {
            unreachable!("Expected FailoverComplete event");
        };
        assert_eq!(reassigned, 0);
        assert_eq!(failed, 0);
    }

    #[test]
    fn test_duplicate_failover_returns_error() {
        let coord = setup_coordinator(FailoverPolicy::Manual);

        coord.initiate_failover(NodeId::new(1)).unwrap();
        // Second failover for same node should fail.
        let result = coord.initiate_failover(NodeId::new(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_failover() {
        let coord = setup_coordinator(FailoverPolicy::Manual);

        coord.initiate_failover(NodeId::new(1)).unwrap();
        coord.cancel_failover(NodeId::new(1)).unwrap();

        let statuses = coord.failover_status();
        assert!(statuses.iter().all(|s| s.completed));
    }

    #[test]
    fn test_cancel_nonexistent_failover_returns_error() {
        let coord = FailoverCoordinator::new(FailoverPolicy::Automatic);
        let result = coord.cancel_failover(NodeId::new(99));
        assert!(result.is_err());
    }

    #[test]
    fn test_failover_with_no_affected_sandboxes() {
        let coord = setup_coordinator(FailoverPolicy::Automatic);

        // Node 3 has no sandboxes.
        let event = coord.initiate_failover(NodeId::new(3)).unwrap();
        let FailoverEvent::FailoverComplete { reassigned, failed, .. } = event else {
            unreachable!("Expected FailoverComplete event");
        };
        assert_eq!(reassigned, 0);
        assert_eq!(failed, 0);
    }

    #[test]
    fn test_failover_with_no_available_targets() {
        let coord = FailoverCoordinator::new(FailoverPolicy::Automatic);
        coord.set_available_nodes(vec![NodeId::new(1)]);
        coord.register_sandbox("sb-1".to_string(), NodeId::new(1));

        // Only node is the failed node itself; no targets.
        let event = coord.initiate_failover(NodeId::new(1)).unwrap();
        let FailoverEvent::FailoverComplete { failed, .. } = event else {
            unreachable!("Expected FailoverComplete event");
        };
        assert_eq!(failed, 1);
    }
}
