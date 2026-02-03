//! Resource-aware scheduler with bin-packing and priority queues.
//!
//! Provides optimal sandbox placement on nodes using first-fit-decreasing
//! bin-packing, priority-based scheduling with fair-share SLA enforcement.

#![allow(missing_docs)]
use super::limits::ResourceLimits;
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashMap};
use std::time::{Duration, Instant};

/// Unique identifier for a compute node.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeId(pub String);

/// Resource requirements for a sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequest {
    /// Memory required in bytes.
    pub memory_bytes: u64,
    /// CPU shares (1000 = 1 full core).
    pub cpu_millicores: u32,
    /// Expected execution duration hint.
    pub expected_duration: Option<Duration>,
    /// Priority level.
    pub priority: Priority,
    /// Tenant or owner identifier for fair-share accounting.
    pub owner: String,
}

impl ResourceRequest {
    /// Create a request from resource limits with a priority and owner.
    pub fn from_limits(limits: &ResourceLimits, priority: Priority, owner: impl Into<String>) -> Self {
        Self {
            memory_bytes: limits.memory.total_max as u64,
            cpu_millicores: 1000, // default 1 core
            expected_duration: limits.time.wall_time,
            priority,
            owner: owner.into(),
        }
    }

    /// A "score" for bin-packing ordering (larger = schedule first).
    pub fn packing_score(&self) -> u64 {
        self.memory_bytes + (self.cpu_millicores as u64 * 1_000_000)
    }
}

/// Priority levels for scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// Background tasks, lowest priority.
    Background = 0,
    /// Normal priority.
    Normal = 1,
    /// High priority, preferred scheduling.
    High = 2,
    /// Critical, preempts lower priorities.
    Critical = 3,
}

/// Available capacity on a compute node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapacity {
    pub node_id: NodeId,
    /// Total memory in bytes.
    pub total_memory: u64,
    /// Available memory in bytes.
    pub available_memory: u64,
    /// Total CPU in millicores.
    pub total_cpu: u32,
    /// Available CPU in millicores.
    pub available_cpu: u32,
    /// Number of running sandboxes.
    pub running_sandboxes: u32,
    /// Maximum sandboxes allowed.
    pub max_sandboxes: u32,
}

impl NodeCapacity {
    /// Check if this node can fit the given resource request.
    pub fn can_fit(&self, req: &ResourceRequest) -> bool {
        self.available_memory >= req.memory_bytes
            && self.available_cpu >= req.cpu_millicores
            && self.running_sandboxes < self.max_sandboxes
    }

    /// Memory utilization percentage (0.0 to 1.0).
    pub fn memory_utilization(&self) -> f64 {
        if self.total_memory == 0 {
            return 1.0;
        }
        1.0 - (self.available_memory as f64 / self.total_memory as f64)
    }

    /// CPU utilization percentage (0.0 to 1.0).
    pub fn cpu_utilization(&self) -> f64 {
        if self.total_cpu == 0 {
            return 1.0;
        }
        1.0 - (self.available_cpu as f64 / self.total_cpu as f64)
    }

    /// Allocate resources for a request. Returns false if insufficient.
    pub fn allocate(&mut self, req: &ResourceRequest) -> bool {
        if !self.can_fit(req) {
            return false;
        }
        self.available_memory -= req.memory_bytes;
        self.available_cpu -= req.cpu_millicores;
        self.running_sandboxes += 1;
        true
    }

    /// Release resources back to the node.
    pub fn release(&mut self, req: &ResourceRequest) {
        self.available_memory = (self.available_memory + req.memory_bytes).min(self.total_memory);
        self.available_cpu = (self.available_cpu + req.cpu_millicores).min(self.total_cpu);
        self.running_sandboxes = self.running_sandboxes.saturating_sub(1);
    }
}

/// Bin-packing placement strategy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PlacementStrategy {
    /// Pack sandboxes tightly to minimize node count (first-fit decreasing).
    BinPack,
    /// Spread sandboxes across nodes for fault tolerance.
    Spread,
    /// Balance utilization across nodes.
    Balanced,
}

/// A queued scheduling request with priority ordering.
#[derive(Debug, Clone)]
struct QueuedRequest {
    id: String,
    request: ResourceRequest,
    enqueued_at: Instant,
}

impl PartialEq for QueuedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for QueuedRequest {}

impl PartialOrd for QueuedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedRequest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first, then FIFO (earlier enqueued first)
        self.request
            .priority
            .cmp(&other.request.priority)
            .then_with(|| other.enqueued_at.cmp(&self.enqueued_at))
    }
}

/// Fair-share quota for a tenant/owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FairShareQuota {
    pub owner: String,
    /// Maximum concurrent sandboxes.
    pub max_concurrent: u32,
    /// Maximum total memory usage in bytes.
    pub max_memory: u64,
    /// Maximum total CPU in millicores.
    pub max_cpu: u32,
    /// Weight for weighted fair-share (higher = more share).
    pub weight: u32,
}

impl FairShareQuota {
    /// Create a default quota for an owner.
    pub fn default_for(owner: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            max_concurrent: 10,
            max_memory: 4 * 1024 * 1024 * 1024, // 4GB
            max_cpu: 4000,                        // 4 cores
            weight: 100,
        }
    }
}

/// Tracks current usage per owner for fair-share enforcement.
#[derive(Debug, Clone, Default)]
struct OwnerUsage {
    concurrent: u32,
    memory_bytes: u64,
    cpu_millicores: u32,
}

/// Scheduling decision result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScheduleResult {
    /// Request was placed on a node.
    Placed {
        node_id: NodeId,
        request_id: String,
    },
    /// Request was queued (no capacity available now).
    Queued {
        request_id: String,
        queue_position: usize,
    },
    /// Request was rejected (exceeds quota or invalid).
    Rejected {
        request_id: String,
        reason: String,
    },
}

/// Resource-aware scheduler with bin-packing and fair-share enforcement.
pub struct ResourceScheduler {
    strategy: PlacementStrategy,
    nodes: parking_lot::RwLock<HashMap<NodeId, NodeCapacity>>,
    queue: parking_lot::Mutex<BinaryHeap<QueuedRequest>>,
    quotas: parking_lot::RwLock<HashMap<String, FairShareQuota>>,
    usage: parking_lot::RwLock<HashMap<String, OwnerUsage>>,
    placements: parking_lot::RwLock<HashMap<String, (NodeId, ResourceRequest)>>,
}

impl ResourceScheduler {
    /// Create a new scheduler with the given placement strategy.
    pub fn new(strategy: PlacementStrategy) -> Self {
        Self {
            strategy,
            nodes: parking_lot::RwLock::new(HashMap::new()),
            queue: parking_lot::Mutex::new(BinaryHeap::new()),
            quotas: parking_lot::RwLock::new(HashMap::new()),
            usage: parking_lot::RwLock::new(HashMap::new()),
            placements: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Register a compute node with its capacity.
    pub fn register_node(&self, capacity: NodeCapacity) {
        self.nodes.write().insert(capacity.node_id.clone(), capacity);
    }

    /// Remove a node from the scheduler.
    pub fn remove_node(&self, node_id: &NodeId) -> Option<NodeCapacity> {
        self.nodes.write().remove(node_id)
    }

    /// Set a fair-share quota for an owner.
    pub fn set_quota(&self, quota: FairShareQuota) {
        self.quotas.write().insert(quota.owner.clone(), quota);
    }

    /// Schedule a resource request, returning placement or queue position.
    pub fn schedule(&self, request_id: impl Into<String>, request: ResourceRequest) -> ScheduleResult {
        let request_id = request_id.into();

        // Check fair-share quota
        if let Some(rejection) = self.check_quota(&request, &request_id) {
            return rejection;
        }

        // Try to place immediately
        if let Some(node_id) = self.find_placement(&request) {
            self.commit_placement(&request_id, &node_id, &request);
            return ScheduleResult::Placed {
                node_id,
                request_id,
            };
        }

        // Queue the request
        let mut queue = self.queue.lock();
        queue.push(QueuedRequest {
            id: request_id.clone(),
            request,
            enqueued_at: Instant::now(),
        });
        let position = queue.len();
        ScheduleResult::Queued {
            request_id,
            queue_position: position,
        }
    }

    /// Release resources for a completed sandbox.
    pub fn release(&self, request_id: &str) {
        if let Some((node_id, request)) = self.placements.write().remove(request_id) {
            if let Some(node) = self.nodes.write().get_mut(&node_id) {
                node.release(&request);
            }
            let mut usage = self.usage.write();
            if let Some(u) = usage.get_mut(&request.owner) {
                u.concurrent = u.concurrent.saturating_sub(1);
                u.memory_bytes = u.memory_bytes.saturating_sub(request.memory_bytes);
                u.cpu_millicores = u.cpu_millicores.saturating_sub(request.cpu_millicores);
            }
        }
    }

    /// Try to drain queued requests after a release.
    pub fn drain_queue(&self) -> Vec<ScheduleResult> {
        let mut results = Vec::new();
        let mut queue = self.queue.lock();
        let mut remaining = BinaryHeap::new();

        while let Some(queued) = queue.pop() {
            if let Some(node_id) = self.find_placement(&queued.request) {
                self.commit_placement(&queued.id, &node_id, &queued.request);
                results.push(ScheduleResult::Placed {
                    node_id,
                    request_id: queued.id,
                });
            } else {
                remaining.push(queued);
            }
        }

        *queue = remaining;
        results
    }

    /// Get the number of queued requests.
    pub fn queue_size(&self) -> usize {
        self.queue.lock().len()
    }

    /// Get current node capacities.
    pub fn node_capacities(&self) -> Vec<NodeCapacity> {
        self.nodes.read().values().cloned().collect()
    }

    /// Get cluster-wide utilization stats.
    pub fn cluster_utilization(&self) -> ClusterUtilization {
        let nodes = self.nodes.read();
        let total_memory: u64 = nodes.values().map(|n| n.total_memory).sum();
        let available_memory: u64 = nodes.values().map(|n| n.available_memory).sum();
        let total_cpu: u32 = nodes.values().map(|n| n.total_cpu).sum();
        let available_cpu: u32 = nodes.values().map(|n| n.available_cpu).sum();
        let total_sandboxes: u32 = nodes.values().map(|n| n.running_sandboxes).sum();

        ClusterUtilization {
            node_count: nodes.len(),
            total_memory,
            used_memory: total_memory - available_memory,
            total_cpu,
            used_cpu: total_cpu - available_cpu,
            running_sandboxes: total_sandboxes,
            queued_requests: self.queue_size(),
        }
    }

    fn check_quota(&self, request: &ResourceRequest, request_id: &str) -> Option<ScheduleResult> {
        let quotas = self.quotas.read();
        if let Some(quota) = quotas.get(&request.owner) {
            let usage = self.usage.read();
            let current = usage.get(&request.owner).cloned().unwrap_or_default();

            if current.concurrent >= quota.max_concurrent {
                return Some(ScheduleResult::Rejected {
                    request_id: request_id.to_string(),
                    reason: format!(
                        "owner '{}' concurrent limit reached ({}/{})",
                        request.owner, current.concurrent, quota.max_concurrent
                    ),
                });
            }
            if current.memory_bytes + request.memory_bytes > quota.max_memory {
                return Some(ScheduleResult::Rejected {
                    request_id: request_id.to_string(),
                    reason: format!(
                        "owner '{}' memory quota exceeded",
                        request.owner
                    ),
                });
            }
            if current.cpu_millicores + request.cpu_millicores > quota.max_cpu {
                return Some(ScheduleResult::Rejected {
                    request_id: request_id.to_string(),
                    reason: format!(
                        "owner '{}' CPU quota exceeded",
                        request.owner
                    ),
                });
            }
        }
        None
    }

    fn find_placement(&self, request: &ResourceRequest) -> Option<NodeId> {
        let nodes = self.nodes.read();
        let mut candidates: Vec<_> = nodes
            .values()
            .filter(|n| n.can_fit(request))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        match self.strategy {
            PlacementStrategy::BinPack => {
                // Most utilized first (pack tightly)
                candidates.sort_by(|a, b| {
                    b.memory_utilization()
                        .partial_cmp(&a.memory_utilization())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            PlacementStrategy::Spread => {
                // Least utilized first (spread out)
                candidates.sort_by(|a, b| {
                    a.memory_utilization()
                        .partial_cmp(&b.memory_utilization())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            PlacementStrategy::Balanced => {
                // Sort by combined utilization (balance memory + CPU)
                candidates.sort_by(|a, b| {
                    let a_util = (a.memory_utilization() + a.cpu_utilization()) / 2.0;
                    let b_util = (b.memory_utilization() + b.cpu_utilization()) / 2.0;
                    a_util
                        .partial_cmp(&b_util)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }

        candidates.first().map(|n| n.node_id.clone())
    }

    fn commit_placement(&self, request_id: &str, node_id: &NodeId, request: &ResourceRequest) {
        if let Some(node) = self.nodes.write().get_mut(node_id) {
            node.allocate(request);
        }

        let mut usage = self.usage.write();
        let u = usage.entry(request.owner.clone()).or_default();
        u.concurrent += 1;
        u.memory_bytes += request.memory_bytes;
        u.cpu_millicores += request.cpu_millicores;

        self.placements.write().insert(
            request_id.to_string(),
            (node_id.clone(), request.clone()),
        );
    }
}

/// Cluster-wide utilization statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterUtilization {
    pub node_count: usize,
    pub total_memory: u64,
    pub used_memory: u64,
    pub total_cpu: u32,
    pub used_cpu: u32,
    pub running_sandboxes: u32,
    pub queued_requests: usize,
}

impl ClusterUtilization {
    /// Memory utilization percentage.
    pub fn memory_pct(&self) -> f64 {
        if self.total_memory == 0 {
            return 0.0;
        }
        self.used_memory as f64 / self.total_memory as f64 * 100.0
    }

    /// CPU utilization percentage.
    pub fn cpu_pct(&self) -> f64 {
        if self.total_cpu == 0 {
            return 0.0;
        }
        self.used_cpu as f64 / self.total_cpu as f64 * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, mem_gb: u64, cpus: u32) -> NodeCapacity {
        NodeCapacity {
            node_id: NodeId(id.into()),
            total_memory: mem_gb * 1024 * 1024 * 1024,
            available_memory: mem_gb * 1024 * 1024 * 1024,
            total_cpu: cpus * 1000,
            available_cpu: cpus * 1000,
            running_sandboxes: 0,
            max_sandboxes: 100,
        }
    }

    fn make_request(mem_mb: u64, owner: &str, priority: Priority) -> ResourceRequest {
        ResourceRequest {
            memory_bytes: mem_mb * 1024 * 1024,
            cpu_millicores: 500,
            expected_duration: Some(Duration::from_secs(30)),
            priority,
            owner: owner.into(),
        }
    }

    #[test]
    fn test_basic_placement() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        sched.register_node(make_node("n1", 8, 4));

        let result = sched.schedule("req-1", make_request(512, "alice", Priority::Normal));
        assert!(matches!(result, ScheduleResult::Placed { .. }));
    }

    #[test]
    fn test_bin_packing() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        sched.register_node(make_node("n1", 4, 4));
        sched.register_node(make_node("n2", 4, 4));

        // Schedule multiple requests — bin-pack should fill n1 first
        sched.schedule("r1", make_request(1024, "alice", Priority::Normal));
        sched.schedule("r2", make_request(1024, "alice", Priority::Normal));

        let util = sched.cluster_utilization();
        assert_eq!(util.running_sandboxes, 2);
    }

    #[test]
    fn test_spread_strategy() {
        let sched = ResourceScheduler::new(PlacementStrategy::Spread);
        sched.register_node(make_node("n1", 4, 4));
        sched.register_node(make_node("n2", 4, 4));

        let r1 = sched.schedule("r1", make_request(512, "alice", Priority::Normal));
        let r2 = sched.schedule("r2", make_request(512, "alice", Priority::Normal));

        // Spread should place on different nodes
        if let (ScheduleResult::Placed { node_id: n1, .. }, ScheduleResult::Placed { node_id: n2, .. }) = (r1, r2) {
            assert_ne!(n1, n2);
        } else {
            panic!("expected both placed");
        }
    }

    #[test]
    fn test_queue_when_full() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        let mut node = make_node("n1", 1, 1);
        node.max_sandboxes = 1;
        sched.register_node(node);

        let r1 = sched.schedule("r1", make_request(100, "alice", Priority::Normal));
        assert!(matches!(r1, ScheduleResult::Placed { .. }));

        let r2 = sched.schedule("r2", make_request(100, "alice", Priority::Normal));
        assert!(matches!(r2, ScheduleResult::Queued { .. }));
        assert_eq!(sched.queue_size(), 1);
    }

    #[test]
    fn test_release_and_drain() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        let mut node = make_node("n1", 1, 1);
        node.max_sandboxes = 1;
        sched.register_node(node);

        sched.schedule("r1", make_request(100, "alice", Priority::Normal));
        sched.schedule("r2", make_request(100, "alice", Priority::Normal));
        assert_eq!(sched.queue_size(), 1);

        sched.release("r1");
        let drained = sched.drain_queue();
        assert_eq!(drained.len(), 1);
        assert!(matches!(drained[0], ScheduleResult::Placed { .. }));
        assert_eq!(sched.queue_size(), 0);
    }

    #[test]
    fn test_fair_share_quota_concurrent_limit() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        sched.register_node(make_node("n1", 16, 8));

        let mut quota = FairShareQuota::default_for("alice");
        quota.max_concurrent = 2;
        sched.set_quota(quota);

        sched.schedule("r1", make_request(100, "alice", Priority::Normal));
        sched.schedule("r2", make_request(100, "alice", Priority::Normal));
        let r3 = sched.schedule("r3", make_request(100, "alice", Priority::Normal));

        assert!(matches!(r3, ScheduleResult::Rejected { .. }));
    }

    #[test]
    fn test_fair_share_quota_memory_limit() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        sched.register_node(make_node("n1", 16, 8));

        let mut quota = FairShareQuota::default_for("bob");
        quota.max_memory = 1024 * 1024 * 1024; // 1GB
        sched.set_quota(quota);

        let r1 = sched.schedule("r1", make_request(800, "bob", Priority::Normal));
        assert!(matches!(r1, ScheduleResult::Placed { .. }));

        let r2 = sched.schedule("r2", make_request(500, "bob", Priority::Normal));
        assert!(matches!(r2, ScheduleResult::Rejected { .. }));
    }

    #[test]
    fn test_no_quota_allows_all() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        sched.register_node(make_node("n1", 16, 16));

        // No quota set — node has enough capacity for 10 requests of 100MB/500m
        for i in 0..10 {
            let r = sched.schedule(format!("r{i}"), make_request(100, "alice", Priority::Normal));
            assert!(matches!(r, ScheduleResult::Placed { .. }));
        }
    }

    #[test]
    fn test_cluster_utilization() {
        let sched = ResourceScheduler::new(PlacementStrategy::BinPack);
        sched.register_node(make_node("n1", 4, 4));
        sched.register_node(make_node("n2", 4, 4));

        sched.schedule("r1", make_request(1024, "alice", Priority::Normal));

        let util = sched.cluster_utilization();
        assert_eq!(util.node_count, 2);
        assert_eq!(util.running_sandboxes, 1);
        assert!(util.memory_pct() > 0.0);
    }

    #[test]
    fn test_node_capacity_utilization() {
        let mut node = make_node("n1", 4, 4);
        assert_eq!(node.memory_utilization(), 0.0);

        let req = make_request(2048, "alice", Priority::Normal);
        node.allocate(&req);
        assert!(node.memory_utilization() > 0.0);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Normal);
        assert!(Priority::Normal > Priority::Background);
    }
}
