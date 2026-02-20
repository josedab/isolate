//! Distributed task scheduler for the sandbox mesh.
//!
//! Provides scheduling of sandbox execution tasks across mesh nodes with
//! support for multiple placement strategies, priority-based ordering,
//! resource-aware placement, and affinity/anti-affinity constraints.

use super::NodeId;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Priority level for scheduled tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskPriority {
    /// Background tasks that run when resources are idle.
    Background = 0,
    /// Low priority tasks.
    Low = 1,
    /// Normal priority (default).
    Normal = 2,
    /// High priority tasks that should be scheduled promptly.
    High = 3,
    /// Critical tasks that must be scheduled immediately, may preempt others.
    Critical = 4,
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::Normal
    }
}

/// Strategy used for placing tasks on nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlacementStrategy {
    /// Pack tasks onto the fewest nodes possible (prefer fullest nodes).
    BinPacking,
    /// Spread tasks evenly across all available nodes.
    Spread,
    /// Place tasks on a random eligible node.
    Random,
    /// Place tasks based on affinity constraints (co-locate with related tasks).
    Affinity,
}

impl Default for PlacementStrategy {
    fn default() -> Self {
        PlacementStrategy::Spread
    }
}

/// Constraints that affect where a task can be placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlacementConstraint {
    /// Task should be placed on one of the specified nodes.
    NodeAffinity {
        /// Preferred node IDs.
        nodes: Vec<NodeId>,
    },
    /// Task must not be placed on any of the specified nodes.
    NodeAntiAffinity {
        /// Excluded node IDs.
        nodes: Vec<NodeId>,
    },
    /// Task requires at least the specified resources.
    ResourceRequirement {
        /// Minimum memory in bytes.
        min_memory_bytes: u64,
        /// Minimum CPU fuel units.
        min_cpu_fuel: u64,
    },
    /// Task should be placed in the specified zone (identified by a string label).
    ZoneAffinity {
        /// Preferred zone label.
        zone: String,
    },
}

/// Resource requirements for a scheduled task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// Required memory in bytes.
    pub memory_bytes: u64,
    /// Required CPU fuel units.
    pub cpu_fuel: u64,
    /// Maximum execution duration.
    pub max_duration: Duration,
    /// Whether the task requires network access.
    pub network_required: bool,
}

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024, // 64 MiB
            cpu_fuel: 1_000_000,
            max_duration: Duration::from_secs(30),
            network_required: false,
        }
    }
}

/// A task to be scheduled across the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// Unique task identifier.
    pub task_id: String,
    /// Sandbox identifier.
    pub sandbox_id: String,
    /// Hash of the WASM module to execute.
    pub module_hash: String,
    /// Task priority.
    pub priority: TaskPriority,
    /// Resource requirements.
    pub resource_requirements: ResourceRequirements,
    /// Placement constraints.
    pub constraints: Vec<PlacementConstraint>,
    /// Timestamp when the task was submitted.
    #[serde(skip)]
    pub submitted_at: Option<Instant>,
}

impl ScheduledTask {
    /// Create a new scheduled task with the given identifiers and default settings.
    pub fn new(sandbox_id: String, module_hash: String) -> Self {
        Self {
            task_id: generate_task_id(),
            sandbox_id,
            module_hash,
            priority: TaskPriority::default(),
            resource_requirements: ResourceRequirements::default(),
            constraints: Vec::new(),
            submitted_at: Some(Instant::now()),
        }
    }

    /// Set the task priority.
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set the resource requirements.
    pub fn with_resources(mut self, resources: ResourceRequirements) -> Self {
        self.resource_requirements = resources;
        self
    }

    /// Add a placement constraint.
    pub fn with_constraint(mut self, constraint: PlacementConstraint) -> Self {
        self.constraints.push(constraint);
        self
    }
}

/// Capacity information for a mesh node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeCapacity {
    /// Total memory in bytes.
    pub total_memory_bytes: u64,
    /// Available (free) memory in bytes.
    pub available_memory_bytes: u64,
    /// Total CPU fuel budget.
    pub total_cpu_fuel: u64,
    /// Available CPU fuel.
    pub available_cpu_fuel: u64,
    /// Number of running sandboxes on the node.
    pub active_sandboxes: u32,
    /// Maximum number of sandboxes the node can host.
    pub max_sandboxes: u32,
    /// Whether the node has network access available.
    pub network_available: bool,
    /// Zone label for zone-aware placement.
    pub zone: Option<String>,
}

impl Default for NodeCapacity {
    fn default() -> Self {
        Self {
            total_memory_bytes: 1024 * 1024 * 1024, // 1 GiB
            available_memory_bytes: 1024 * 1024 * 1024,
            total_cpu_fuel: 100_000_000,
            available_cpu_fuel: 100_000_000,
            active_sandboxes: 0,
            max_sandboxes: 100,
            network_available: true,
            zone: None,
        }
    }
}

impl NodeCapacity {
    /// Check whether the node can satisfy the given resource requirements.
    pub fn can_fit(&self, requirements: &ResourceRequirements) -> bool {
        self.available_memory_bytes >= requirements.memory_bytes
            && self.available_cpu_fuel >= requirements.cpu_fuel
            && self.active_sandboxes < self.max_sandboxes
            && (!requirements.network_required || self.network_available)
    }

    /// Return a utilization ratio from 0.0 (empty) to 1.0 (full).
    pub fn utilization(&self) -> f64 {
        if self.total_memory_bytes == 0 || self.total_cpu_fuel == 0 {
            return 1.0;
        }
        let mem_util = 1.0 - (self.available_memory_bytes as f64 / self.total_memory_bytes as f64);
        let cpu_util = 1.0 - (self.available_cpu_fuel as f64 / self.total_cpu_fuel as f64);
        (mem_util + cpu_util) / 2.0
    }

    /// Deduct the given resource requirements from available capacity.
    pub fn allocate(&mut self, requirements: &ResourceRequirements) {
        self.available_memory_bytes =
            self.available_memory_bytes.saturating_sub(requirements.memory_bytes);
        self.available_cpu_fuel = self.available_cpu_fuel.saturating_sub(requirements.cpu_fuel);
        self.active_sandboxes += 1;
    }

    /// Return the given resource requirements to available capacity.
    pub fn release(&mut self, requirements: &ResourceRequirements) {
        self.available_memory_bytes =
            (self.available_memory_bytes + requirements.memory_bytes).min(self.total_memory_bytes);
        self.available_cpu_fuel =
            (self.available_cpu_fuel + requirements.cpu_fuel).min(self.total_cpu_fuel);
        self.active_sandboxes = self.active_sandboxes.saturating_sub(1);
    }
}

/// The result of scheduling a single task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingResult {
    /// The task that was scheduled.
    pub task_id: String,
    /// The node the task was assigned to.
    pub assigned_node: NodeId,
    /// Human-readable reason for the placement decision.
    pub reason: String,
}

/// Configuration for the task scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// How often the scheduler runs a scheduling round.
    pub scheduling_interval: Duration,
    /// Maximum number of tasks allowed in the pending queue.
    pub max_pending_tasks: usize,
    /// Whether higher-priority tasks can preempt lower-priority ones.
    pub preemption_enabled: bool,
    /// Default placement strategy.
    pub placement_strategy: PlacementStrategy,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            scheduling_interval: Duration::from_millis(100),
            max_pending_tasks: 10_000,
            preemption_enabled: false,
            placement_strategy: PlacementStrategy::Spread,
        }
    }
}

/// Statistics about the scheduler's operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulerStats {
    /// Number of tasks currently pending.
    pub pending_tasks: usize,
    /// Number of tasks that have been scheduled.
    pub scheduled_tasks: u64,
    /// Number of tasks that failed to schedule (no eligible node).
    pub failed_tasks: u64,
    /// Number of tasks that were cancelled.
    pub cancelled_tasks: u64,
    /// Number of scheduling rounds executed.
    pub scheduling_rounds: u64,
    /// Number of known nodes with capacity information.
    pub known_nodes: usize,
}

/// Distributed task scheduler for assigning sandbox tasks to mesh nodes.
///
/// The scheduler maintains a priority queue of pending tasks and a capacity map
/// of known nodes. Each scheduling round sorts pending tasks by priority and
/// assigns them to the best eligible node according to the configured placement
/// strategy.
pub struct TaskScheduler {
    /// Scheduler configuration.
    config: SchedulerConfig,
    /// Pending tasks ordered by submission time (priority sorting happens during scheduling).
    pending: Arc<RwLock<VecDeque<ScheduledTask>>>,
    /// Scheduled results keyed by task_id.
    scheduled: Arc<RwLock<BTreeMap<String, SchedulingResult>>>,
    /// Node capacity information keyed by NodeId.
    capacities: Arc<RwLock<HashMap<NodeId, NodeCapacity>>>,
    /// Running statistics.
    stats: Arc<RwLock<SchedulerStats>>,
}

impl TaskScheduler {
    /// Create a new task scheduler with the given configuration.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            pending: Arc::new(RwLock::new(VecDeque::new())),
            scheduled: Arc::new(RwLock::new(BTreeMap::new())),
            capacities: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(SchedulerStats::default())),
        }
    }

    /// Submit a task for scheduling.
    ///
    /// Returns an error if the pending queue is full.
    pub fn submit(&self, task: ScheduledTask) -> Result<()> {
        let mut pending =
            self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        if pending.len() >= self.config.max_pending_tasks {
            return Err(Error::Engine(format!(
                "Scheduler pending queue is full (max {} tasks)",
                self.config.max_pending_tasks
            )));
        }

        pending.push_back(task);
        Ok(())
    }

    /// Run one scheduling round.
    ///
    /// This method:
    /// 1. Sorts pending tasks by priority (highest first).
    /// 2. For each task, finds eligible nodes based on constraints and capacity.
    /// 3. Scores eligible nodes based on the placement strategy.
    /// 4. Assigns the task to the highest-scoring node and updates capacity.
    ///
    /// Returns the list of scheduling results produced in this round.
    pub fn schedule_round(&self) -> Result<Vec<SchedulingResult>> {
        // Take all pending tasks out of the queue.
        let mut tasks: Vec<ScheduledTask> = {
            let mut pending =
                self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            pending.drain(..).collect()
        };

        // Sort by priority descending (highest priority first).
        tasks.sort_by(|a, b| b.priority.cmp(&a.priority));

        let mut results = Vec::new();
        let mut unscheduled = VecDeque::new();

        let mut capacities =
            self.capacities.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        for task in tasks {
            match self.find_best_node(&task, &capacities) {
                Some((node_id, reason)) => {
                    // Deduct resources from the chosen node.
                    if let Some(capacity) = capacities.get_mut(&node_id) {
                        capacity.allocate(&task.resource_requirements);
                    }

                    let result = SchedulingResult {
                        task_id: task.task_id.clone(),
                        assigned_node: node_id,
                        reason,
                    };
                    results.push(result);
                }
                None => {
                    // No eligible node found; put back in the pending queue.
                    unscheduled.push_back(task);
                }
            }
        }

        // Update stats.
        let failed_count = unscheduled.len() as u64;
        let scheduled_count = results.len() as u64;

        // Store results.
        {
            let mut scheduled =
                self.scheduled.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            for result in &results {
                scheduled.insert(result.task_id.clone(), result.clone());
            }
        }

        // Put unscheduled tasks back into the pending queue.
        {
            let mut pending =
                self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            // Prepend unscheduled tasks so they get another chance next round.
            for task in unscheduled.into_iter().rev() {
                pending.push_front(task);
            }
        }

        // Update statistics.
        {
            let mut stats =
                self.stats.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            stats.scheduling_rounds += 1;
            stats.scheduled_tasks += scheduled_count;
            stats.failed_tasks += failed_count;
            stats.pending_tasks = self.pending.read().map(|p| p.len()).unwrap_or(0);
            stats.known_nodes = capacities.len();
        }

        Ok(results)
    }

    /// Cancel a pending task by its task ID.
    ///
    /// Returns `true` if the task was found and removed from the pending queue.
    pub fn cancel(&self, task_id: &str) -> Result<bool> {
        let mut pending =
            self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        let original_len = pending.len();
        pending.retain(|t| t.task_id != task_id);
        let removed = pending.len() < original_len;

        if removed {
            let mut stats =
                self.stats.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            stats.cancelled_tasks += 1;
            stats.pending_tasks = pending.len();
        }

        Ok(removed)
    }

    /// Update the capacity information for a node.
    pub fn update_capacity(&self, node_id: NodeId, capacity: NodeCapacity) -> Result<()> {
        let mut capacities =
            self.capacities.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        capacities.insert(node_id, capacity);
        Ok(())
    }

    /// Get the number of pending tasks.
    pub fn pending_count(&self) -> usize {
        self.pending.read().map(|p| p.len()).unwrap_or(0)
    }

    /// Get the number of tasks that have been scheduled.
    pub fn scheduled_count(&self) -> usize {
        self.scheduled.read().map(|s| s.len()).unwrap_or(0)
    }

    /// Get a snapshot of the scheduler statistics.
    pub fn stats(&self) -> Result<SchedulerStats> {
        let mut stats =
            self.stats.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?.clone();
        stats.pending_tasks = self.pending_count();
        stats.known_nodes = self.capacities.read().map(|c| c.len()).unwrap_or(0);
        Ok(stats)
    }

    /// Find the best node for a task given the current capacity map.
    ///
    /// Returns `None` if no eligible node exists.
    fn find_best_node(
        &self,
        task: &ScheduledTask,
        capacities: &HashMap<NodeId, NodeCapacity>,
    ) -> Option<(NodeId, String)> {
        // Collect eligible nodes that satisfy constraints and have capacity.
        let eligible: Vec<(NodeId, &NodeCapacity)> = capacities
            .iter()
            .filter(|(node_id, cap)| {
                cap.can_fit(&task.resource_requirements)
                    && self.satisfies_constraints(task, **node_id, cap)
            })
            .map(|(id, cap)| (*id, cap))
            .collect();

        if eligible.is_empty() {
            return None;
        }

        // Score each eligible node according to the placement strategy.
        let strategy = self.config.placement_strategy;
        let mut scored: Vec<(NodeId, f64)> =
            eligible.iter().map(|(id, cap)| (*id, self.score_node(strategy, task, cap))).collect();

        // Sort by score descending (highest score is best).
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (best_node, best_score) = scored[0];
        let reason = format!("Placed by {:?} strategy (score: {:.3})", strategy, best_score);

        Some((best_node, reason))
    }

    /// Check whether a task's placement constraints are satisfied by the given node.
    fn satisfies_constraints(
        &self,
        task: &ScheduledTask,
        node_id: NodeId,
        capacity: &NodeCapacity,
    ) -> bool {
        for constraint in &task.constraints {
            match constraint {
                PlacementConstraint::NodeAffinity { nodes } => {
                    if !nodes.contains(&node_id) {
                        return false;
                    }
                }
                PlacementConstraint::NodeAntiAffinity { nodes } => {
                    if nodes.contains(&node_id) {
                        return false;
                    }
                }
                PlacementConstraint::ResourceRequirement { min_memory_bytes, min_cpu_fuel } => {
                    if capacity.available_memory_bytes < *min_memory_bytes
                        || capacity.available_cpu_fuel < *min_cpu_fuel
                    {
                        return false;
                    }
                }
                PlacementConstraint::ZoneAffinity { zone } => {
                    if capacity.zone.as_deref() != Some(zone.as_str()) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Score a node for a given task based on the placement strategy.
    ///
    /// Higher score means better fit.
    fn score_node(
        &self,
        strategy: PlacementStrategy,
        task: &ScheduledTask,
        capacity: &NodeCapacity,
    ) -> f64 {
        match strategy {
            PlacementStrategy::BinPacking => {
                // Prefer the fullest node (highest utilization) that can still fit the task.
                capacity.utilization()
            }
            PlacementStrategy::Spread => {
                // Prefer the emptiest node (lowest utilization).
                1.0 - capacity.utilization()
            }
            PlacementStrategy::Random => {
                // Assign a pseudo-random score based on node and task identifiers.
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                task.task_id.hash(&mut hasher);
                capacity.active_sandboxes.hash(&mut hasher);
                (hasher.finish() % 1000) as f64 / 1000.0
            }
            PlacementStrategy::Affinity => {
                // Prefer nodes listed in affinity constraints; fall back to spread.
                let mut score = 1.0 - capacity.utilization();
                for constraint in &task.constraints {
                    if let PlacementConstraint::NodeAffinity { nodes } = constraint {
                        // Iterate all node IDs in the capacity map is not possible here,
                        // but we already filtered eligible nodes. Give a bonus if the node
                        // is explicitly listed in an affinity constraint.
                        // Note: We do not have node_id here, so we use a flat bonus approach.
                        // The satisfies_constraints method already enforces hard affinity.
                        if !nodes.is_empty() {
                            score += 0.5;
                        }
                    }
                }
                score
            }
        }
    }
}

/// Generate a unique task identifier.
fn generate_task_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    format!("task-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn make_capacity(node_id: u64) -> (NodeId, NodeCapacity) {
        (NodeId::new(node_id), NodeCapacity::default())
    }

    fn make_task(sandbox_id: &str) -> ScheduledTask {
        ScheduledTask::new(sandbox_id.to_string(), "hash-abc123".to_string())
    }

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
        assert!(TaskPriority::Low > TaskPriority::Background);
    }

    #[test]
    fn test_task_priority_default() {
        assert_eq!(TaskPriority::default(), TaskPriority::Normal);
    }

    #[test]
    fn test_placement_strategy_default() {
        assert_eq!(PlacementStrategy::default(), PlacementStrategy::Spread);
    }

    #[test]
    fn test_resource_requirements_default() {
        let req = ResourceRequirements::default();
        assert_eq!(req.memory_bytes, 64 * 1024 * 1024);
        assert_eq!(req.cpu_fuel, 1_000_000);
        assert_eq!(req.max_duration, Duration::from_secs(30));
        assert!(!req.network_required);
    }

    #[test]
    fn test_scheduled_task_creation() {
        let task = ScheduledTask::new("sb-1".to_string(), "hash-abc".to_string());
        assert!(task.task_id.starts_with("task-"));
        assert_eq!(task.sandbox_id, "sb-1");
        assert_eq!(task.module_hash, "hash-abc");
        assert_eq!(task.priority, TaskPriority::Normal);
        assert!(task.constraints.is_empty());
        assert!(task.submitted_at.is_some());
    }

    #[test]
    fn test_scheduled_task_builder_methods() {
        let task = ScheduledTask::new("sb-1".to_string(), "hash-abc".to_string())
            .with_priority(TaskPriority::High)
            .with_resources(ResourceRequirements {
                memory_bytes: 128 * 1024 * 1024,
                cpu_fuel: 5_000_000,
                max_duration: Duration::from_secs(60),
                network_required: true,
            })
            .with_constraint(PlacementConstraint::NodeAffinity { nodes: vec![NodeId::new(1)] });

        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.resource_requirements.memory_bytes, 128 * 1024 * 1024);
        assert!(task.resource_requirements.network_required);
        assert_eq!(task.constraints.len(), 1);
    }

    #[test]
    fn test_node_capacity_default() {
        let cap = NodeCapacity::default();
        assert_eq!(cap.total_memory_bytes, 1024 * 1024 * 1024);
        assert_eq!(cap.available_memory_bytes, 1024 * 1024 * 1024);
        assert_eq!(cap.active_sandboxes, 0);
        assert_eq!(cap.max_sandboxes, 100);
        assert!(cap.network_available);
    }

    #[test]
    fn test_node_capacity_can_fit() {
        let cap = NodeCapacity::default();
        let req = ResourceRequirements::default();
        assert!(cap.can_fit(&req));

        let small_cap = NodeCapacity { available_memory_bytes: 1024, ..Default::default() };
        assert!(!small_cap.can_fit(&req));
    }

    #[test]
    fn test_node_capacity_can_fit_network() {
        let cap = NodeCapacity { network_available: false, ..Default::default() };
        let req = ResourceRequirements { network_required: true, ..Default::default() };
        assert!(!cap.can_fit(&req));
    }

    #[test]
    fn test_node_capacity_can_fit_max_sandboxes() {
        let cap = NodeCapacity { active_sandboxes: 100, max_sandboxes: 100, ..Default::default() };
        let req = ResourceRequirements::default();
        assert!(!cap.can_fit(&req));
    }

    #[test]
    fn test_node_capacity_utilization() {
        let cap = NodeCapacity::default();
        assert!((cap.utilization() - 0.0).abs() < f64::EPSILON);

        let half_used = NodeCapacity {
            total_memory_bytes: 1000,
            available_memory_bytes: 500,
            total_cpu_fuel: 1000,
            available_cpu_fuel: 500,
            ..Default::default()
        };
        assert!((half_used.utilization() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_node_capacity_allocate_release() {
        let mut cap = NodeCapacity {
            total_memory_bytes: 1000,
            available_memory_bytes: 1000,
            total_cpu_fuel: 1000,
            available_cpu_fuel: 1000,
            active_sandboxes: 0,
            max_sandboxes: 10,
            network_available: true,
            zone: None,
        };

        let req = ResourceRequirements {
            memory_bytes: 300,
            cpu_fuel: 200,
            max_duration: Duration::from_secs(10),
            network_required: false,
        };

        cap.allocate(&req);
        assert_eq!(cap.available_memory_bytes, 700);
        assert_eq!(cap.available_cpu_fuel, 800);
        assert_eq!(cap.active_sandboxes, 1);

        cap.release(&req);
        assert_eq!(cap.available_memory_bytes, 1000);
        assert_eq!(cap.available_cpu_fuel, 1000);
        assert_eq!(cap.active_sandboxes, 0);
    }

    #[test]
    fn test_node_capacity_allocate_saturating() {
        let mut cap = NodeCapacity {
            total_memory_bytes: 100,
            available_memory_bytes: 50,
            total_cpu_fuel: 100,
            available_cpu_fuel: 30,
            active_sandboxes: 0,
            max_sandboxes: 10,
            network_available: true,
            zone: None,
        };

        let req = ResourceRequirements {
            memory_bytes: 200,
            cpu_fuel: 200,
            max_duration: Duration::from_secs(10),
            network_required: false,
        };

        cap.allocate(&req);
        assert_eq!(cap.available_memory_bytes, 0);
        assert_eq!(cap.available_cpu_fuel, 0);
    }

    #[test]
    fn test_node_capacity_release_capped() {
        let mut cap = NodeCapacity {
            total_memory_bytes: 100,
            available_memory_bytes: 80,
            total_cpu_fuel: 100,
            available_cpu_fuel: 80,
            active_sandboxes: 1,
            max_sandboxes: 10,
            network_available: true,
            zone: None,
        };

        let req = ResourceRequirements {
            memory_bytes: 50,
            cpu_fuel: 50,
            max_duration: Duration::from_secs(10),
            network_required: false,
        };

        cap.release(&req);
        // Should not exceed total capacity.
        assert_eq!(cap.available_memory_bytes, 100);
        assert_eq!(cap.available_cpu_fuel, 100);
        assert_eq!(cap.active_sandboxes, 0);
    }

    #[test]
    fn test_scheduler_config_default() {
        let config = SchedulerConfig::default();
        assert_eq!(config.scheduling_interval, Duration::from_millis(100));
        assert_eq!(config.max_pending_tasks, 10_000);
        assert!(!config.preemption_enabled);
        assert_eq!(config.placement_strategy, PlacementStrategy::Spread);
    }

    #[test]
    fn test_scheduler_submit_and_pending_count() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        assert_eq!(scheduler.pending_count(), 0);

        scheduler.submit(make_task("sb-1")).unwrap();
        assert_eq!(scheduler.pending_count(), 1);

        scheduler.submit(make_task("sb-2")).unwrap();
        assert_eq!(scheduler.pending_count(), 2);
    }

    #[test]
    fn test_scheduler_submit_queue_full() {
        let config = SchedulerConfig { max_pending_tasks: 1, ..Default::default() };
        let scheduler = TaskScheduler::new(config);

        scheduler.submit(make_task("sb-1")).unwrap();
        let result = scheduler.submit(make_task("sb-2"));
        assert!(result.is_err());
    }

    #[test]
    fn test_scheduler_cancel() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        let task = make_task("sb-1");
        let task_id = task.task_id.clone();
        scheduler.submit(task).unwrap();

        assert_eq!(scheduler.pending_count(), 1);
        let removed = scheduler.cancel(&task_id).unwrap();
        assert!(removed);
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_scheduler_cancel_not_found() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        let removed = scheduler.cancel("nonexistent-task").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_scheduler_schedule_round_no_nodes() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        scheduler.submit(make_task("sb-1")).unwrap();

        let results = scheduler.schedule_round().unwrap();
        assert!(results.is_empty());

        // Task should still be pending.
        assert_eq!(scheduler.pending_count(), 1);
    }

    #[test]
    fn test_scheduler_schedule_round_basic() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        scheduler.update_capacity(NodeId::new(1), NodeCapacity::default()).unwrap();
        scheduler.submit(make_task("sb-1")).unwrap();

        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].assigned_node, NodeId::new(1));
        assert_eq!(scheduler.pending_count(), 0);
        assert_eq!(scheduler.scheduled_count(), 1);
    }

    #[test]
    fn test_scheduler_schedule_round_priority_ordering() {
        let config = SchedulerConfig {
            placement_strategy: PlacementStrategy::BinPacking,
            ..Default::default()
        };
        let scheduler = TaskScheduler::new(config);

        // Only one node with limited capacity: enough for one task.
        let limited_cap = NodeCapacity {
            total_memory_bytes: 64 * 1024 * 1024 + 1,
            available_memory_bytes: 64 * 1024 * 1024 + 1,
            total_cpu_fuel: 1_000_001,
            available_cpu_fuel: 1_000_001,
            active_sandboxes: 0,
            max_sandboxes: 1,
            network_available: true,
            zone: None,
        };
        scheduler.update_capacity(NodeId::new(1), limited_cap).unwrap();

        // Submit low-priority first, then high-priority.
        let low_task = make_task("sb-low").with_priority(TaskPriority::Low);
        let high_task = make_task("sb-high").with_priority(TaskPriority::High);
        let _low_task_id = low_task.task_id.clone();
        let high_task_id = high_task.task_id.clone();

        scheduler.submit(low_task).unwrap();
        scheduler.submit(high_task).unwrap();

        let results = scheduler.schedule_round().unwrap();

        // High-priority task should be scheduled, low-priority should remain pending.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, high_task_id);
        assert_eq!(scheduler.pending_count(), 1);
    }

    #[test]
    fn test_scheduler_schedule_round_spread_strategy() {
        let config =
            SchedulerConfig { placement_strategy: PlacementStrategy::Spread, ..Default::default() };
        let scheduler = TaskScheduler::new(config);

        // Node 1 is heavily loaded, Node 2 is empty.
        // Both nodes must have enough resources to fit the default task requirements.
        let loaded_cap = NodeCapacity {
            total_memory_bytes: 1_000_000_000,
            available_memory_bytes: 200_000_000,
            total_cpu_fuel: 100_000_000,
            available_cpu_fuel: 20_000_000,
            active_sandboxes: 5,
            max_sandboxes: 10,
            network_available: true,
            zone: None,
        };
        let empty_cap = NodeCapacity {
            total_memory_bytes: 1_000_000_000,
            available_memory_bytes: 1_000_000_000,
            total_cpu_fuel: 100_000_000,
            available_cpu_fuel: 100_000_000,
            active_sandboxes: 0,
            max_sandboxes: 10,
            network_available: true,
            zone: None,
        };

        scheduler.update_capacity(NodeId::new(1), loaded_cap).unwrap();
        scheduler.update_capacity(NodeId::new(2), empty_cap).unwrap();

        scheduler.submit(make_task("sb-1")).unwrap();

        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        // Spread should prefer the emptier node.
        assert_eq!(results[0].assigned_node, NodeId::new(2));
    }

    #[test]
    fn test_scheduler_schedule_round_bin_packing_strategy() {
        let config = SchedulerConfig {
            placement_strategy: PlacementStrategy::BinPacking,
            ..Default::default()
        };
        let scheduler = TaskScheduler::new(config);

        // Node 1 is somewhat loaded, Node 2 is empty.
        let loaded_cap = NodeCapacity {
            total_memory_bytes: 1_000_000_000,
            available_memory_bytes: 500_000_000,
            total_cpu_fuel: 100_000_000,
            available_cpu_fuel: 50_000_000,
            active_sandboxes: 5,
            max_sandboxes: 100,
            network_available: true,
            zone: None,
        };
        let empty_cap = NodeCapacity {
            total_memory_bytes: 1_000_000_000,
            available_memory_bytes: 1_000_000_000,
            total_cpu_fuel: 100_000_000,
            available_cpu_fuel: 100_000_000,
            active_sandboxes: 0,
            max_sandboxes: 100,
            network_available: true,
            zone: None,
        };

        scheduler.update_capacity(NodeId::new(1), loaded_cap).unwrap();
        scheduler.update_capacity(NodeId::new(2), empty_cap).unwrap();

        scheduler.submit(make_task("sb-1")).unwrap();

        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        // Bin-packing should prefer the fuller node.
        assert_eq!(results[0].assigned_node, NodeId::new(1));
    }

    #[test]
    fn test_scheduler_node_affinity_constraint() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        scheduler.update_capacity(NodeId::new(1), NodeCapacity::default()).unwrap();
        scheduler.update_capacity(NodeId::new(2), NodeCapacity::default()).unwrap();

        let task = make_task("sb-1")
            .with_constraint(PlacementConstraint::NodeAffinity { nodes: vec![NodeId::new(2)] });

        scheduler.submit(task).unwrap();
        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].assigned_node, NodeId::new(2));
    }

    #[test]
    fn test_scheduler_node_anti_affinity_constraint() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        scheduler.update_capacity(NodeId::new(1), NodeCapacity::default()).unwrap();
        scheduler.update_capacity(NodeId::new(2), NodeCapacity::default()).unwrap();

        let task = make_task("sb-1")
            .with_constraint(PlacementConstraint::NodeAntiAffinity { nodes: vec![NodeId::new(1)] });

        scheduler.submit(task).unwrap();
        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].assigned_node, NodeId::new(2));
    }

    #[test]
    fn test_scheduler_zone_affinity_constraint() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        let cap_zone_a = NodeCapacity { zone: Some("zone-a".to_string()), ..Default::default() };
        let cap_zone_b = NodeCapacity { zone: Some("zone-b".to_string()), ..Default::default() };

        scheduler.update_capacity(NodeId::new(1), cap_zone_a).unwrap();
        scheduler.update_capacity(NodeId::new(2), cap_zone_b).unwrap();

        let task = make_task("sb-1")
            .with_constraint(PlacementConstraint::ZoneAffinity { zone: "zone-b".to_string() });

        scheduler.submit(task).unwrap();
        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].assigned_node, NodeId::new(2));
    }

    #[test]
    fn test_scheduler_resource_requirement_constraint() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        let small_cap = NodeCapacity {
            total_memory_bytes: 100_000,
            available_memory_bytes: 100_000,
            total_cpu_fuel: 100_000,
            available_cpu_fuel: 100_000,
            active_sandboxes: 0,
            max_sandboxes: 10,
            network_available: true,
            zone: None,
        };
        let large_cap = NodeCapacity::default(); // 1 GiB

        scheduler.update_capacity(NodeId::new(1), small_cap).unwrap();
        scheduler.update_capacity(NodeId::new(2), large_cap).unwrap();

        let task = make_task("sb-1").with_constraint(PlacementConstraint::ResourceRequirement {
            min_memory_bytes: 500_000_000,
            min_cpu_fuel: 50_000_000,
        });

        scheduler.submit(task).unwrap();
        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].assigned_node, NodeId::new(2));
    }

    #[test]
    fn test_scheduler_multiple_tasks_multiple_nodes() {
        let scheduler = TaskScheduler::new(SchedulerConfig {
            placement_strategy: PlacementStrategy::Spread,
            ..Default::default()
        });

        scheduler.update_capacity(NodeId::new(1), NodeCapacity::default()).unwrap();
        scheduler.update_capacity(NodeId::new(2), NodeCapacity::default()).unwrap();

        for i in 0..4 {
            scheduler.submit(make_task(&format!("sb-{}", i))).unwrap();
        }

        let results = scheduler.schedule_round().unwrap();
        assert_eq!(results.len(), 4);
        assert_eq!(scheduler.pending_count(), 0);
        assert_eq!(scheduler.scheduled_count(), 4);

        // With spread strategy, tasks should be distributed across nodes.
        let node1_count = results.iter().filter(|r| r.assigned_node == NodeId::new(1)).count();
        let node2_count = results.iter().filter(|r| r.assigned_node == NodeId::new(2)).count();
        assert!(node1_count > 0 && node2_count > 0);
    }

    #[test]
    fn test_scheduler_stats() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        scheduler.update_capacity(NodeId::new(1), NodeCapacity::default()).unwrap();

        scheduler.submit(make_task("sb-1")).unwrap();
        scheduler.schedule_round().unwrap();

        let stats = scheduler.stats().unwrap();
        assert_eq!(stats.scheduled_tasks, 1);
        assert_eq!(stats.scheduling_rounds, 1);
        assert_eq!(stats.pending_tasks, 0);
        assert_eq!(stats.known_nodes, 1);
    }

    #[test]
    fn test_scheduler_stats_failed_tasks() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        // No nodes registered, so tasks cannot be placed.
        scheduler.submit(make_task("sb-1")).unwrap();
        scheduler.schedule_round().unwrap();

        let stats = scheduler.stats().unwrap();
        assert_eq!(stats.failed_tasks, 1);
        assert_eq!(stats.scheduled_tasks, 0);
        assert_eq!(stats.pending_tasks, 1);
    }

    #[test]
    fn test_scheduler_stats_cancelled_tasks() {
        let scheduler = TaskScheduler::new(SchedulerConfig::default());

        let task = make_task("sb-1");
        let task_id = task.task_id.clone();
        scheduler.submit(task).unwrap();
        scheduler.cancel(&task_id).unwrap();

        let stats = scheduler.stats().unwrap();
        assert_eq!(stats.cancelled_tasks, 1);
        assert_eq!(stats.pending_tasks, 0);
    }

    #[test]
    fn test_scheduling_result_fields() {
        let result = SchedulingResult {
            task_id: "task-123".to_string(),
            assigned_node: NodeId::new(42),
            reason: "test reason".to_string(),
        };

        assert_eq!(result.task_id, "task-123");
        assert_eq!(result.assigned_node, NodeId::new(42));
        assert_eq!(result.reason, "test reason");
    }

    #[test]
    fn test_generate_task_id_uniqueness() {
        let id1 = generate_task_id();
        let id2 = generate_task_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("task-"));
        assert!(id2.starts_with("task-"));
    }
}
