//! Raft consensus and leader election for the distributed mesh.
//!
//! Provides Raft-based leader election, split-brain protection,
//! and work-stealing scheduling for the mesh cluster.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use super::NodeId;

/// Raft node state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

/// A Raft log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Term when entry was created.
    pub term: u64,
    /// Log index.
    pub index: u64,
    /// Command to apply.
    pub command: RaftCommand,
}

/// Commands replicated via Raft log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RaftCommand {
    /// Add a node to the cluster.
    AddNode { node_id: NodeId },
    /// Remove a node from the cluster.
    RemoveNode { node_id: NodeId },
    /// Assign a sandbox to a node.
    AssignSandbox { sandbox_id: String, node_id: NodeId },
    /// Migrate a sandbox between nodes.
    MigrateSandbox { sandbox_id: String, from: NodeId, to: NodeId },
    /// Update cluster configuration.
    UpdateConfig { key: String, value: String },
    /// No-op (used for leader commit confirmation).
    Noop,
}

/// Raft consensus state for a single node.
pub struct RaftNode {
    /// This node's ID.
    node_id: NodeId,
    /// Current term.
    current_term: u64,
    /// Current role.
    role: RaftRole,
    /// Who we voted for in the current term.
    voted_for: Option<NodeId>,
    /// Current leader (if known).
    leader_id: Option<NodeId>,
    /// Log entries.
    log: Vec<LogEntry>,
    /// Index of last committed entry.
    commit_index: u64,
    /// Index of last applied entry.
    last_applied: u64,
    /// Known cluster members.
    cluster_members: HashSet<NodeId>,
    /// Votes received (when candidate).
    votes_received: HashSet<NodeId>,
    /// Next index to send to each follower (when leader).
    next_index: HashMap<NodeId, u64>,
    /// Highest replicated index for each follower (when leader).
    match_index: HashMap<NodeId, u64>,
    /// Last heartbeat received.
    last_heartbeat: Instant,
    /// Election timeout.
    election_timeout: Duration,
}

impl RaftNode {
    /// Create a new Raft node.
    pub fn new(node_id: NodeId, election_timeout: Duration) -> Self {
        let mut members = HashSet::new();
        members.insert(node_id);

        Self {
            node_id,
            current_term: 0,
            role: RaftRole::Follower,
            voted_for: None,
            leader_id: None,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            cluster_members: members,
            votes_received: HashSet::new(),
            next_index: HashMap::new(),
            match_index: HashMap::new(),
            last_heartbeat: Instant::now(),
            election_timeout,
        }
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get current term.
    pub fn current_term(&self) -> u64 {
        self.current_term
    }

    /// Get current role.
    pub fn role(&self) -> RaftRole {
        self.role
    }

    /// Get the current leader.
    pub fn leader_id(&self) -> Option<NodeId> {
        self.leader_id
    }

    /// Check if this node is the leader.
    pub fn is_leader(&self) -> bool {
        self.role == RaftRole::Leader
    }

    /// Add a member to the cluster.
    pub fn add_member(&mut self, node_id: NodeId) {
        self.cluster_members.insert(node_id);
    }

    /// Remove a member from the cluster.
    pub fn remove_member(&mut self, node_id: &NodeId) {
        self.cluster_members.remove(node_id);
    }

    /// Get cluster members.
    pub fn cluster_members(&self) -> &HashSet<NodeId> {
        &self.cluster_members
    }

    /// Get the quorum size.
    pub fn quorum_size(&self) -> usize {
        self.cluster_members.len() / 2 + 1
    }

    /// Check if election timeout has elapsed.
    pub fn election_timeout_elapsed(&self) -> bool {
        self.last_heartbeat.elapsed() >= self.election_timeout
    }

    /// Start an election (become candidate).
    pub fn start_election(&mut self) -> VoteRequest {
        self.current_term += 1;
        self.role = RaftRole::Candidate;
        self.voted_for = Some(self.node_id);
        self.votes_received.clear();
        self.votes_received.insert(self.node_id);
        self.leader_id = None;

        // Single-node cluster: immediately become leader
        if self.votes_received.len() >= self.quorum_size() {
            self.become_leader();
        }

        let last_log_index = self.log.last().map(|e| e.index).unwrap_or(0);
        let last_log_term = self.log.last().map(|e| e.term).unwrap_or(0);

        VoteRequest {
            term: self.current_term,
            candidate_id: self.node_id,
            last_log_index,
            last_log_term,
        }
    }

    /// Handle a vote request.
    pub fn handle_vote_request(&mut self, request: &VoteRequest) -> VoteResponse {
        // If request term is higher, step down
        if request.term > self.current_term {
            self.step_down(request.term);
        }

        let vote_granted = if request.term < self.current_term {
            false
        } else if self.voted_for.is_some() && self.voted_for != Some(request.candidate_id) {
            false
        } else {
            // Check log is at least as up-to-date
            let our_last_index = self.log.last().map(|e| e.index).unwrap_or(0);
            let our_last_term = self.log.last().map(|e| e.term).unwrap_or(0);

            if request.last_log_term > our_last_term {
                true
            } else if request.last_log_term == our_last_term {
                request.last_log_index >= our_last_index
            } else {
                false
            }
        };

        if vote_granted {
            self.voted_for = Some(request.candidate_id);
            self.last_heartbeat = Instant::now();
        }

        VoteResponse { term: self.current_term, vote_granted }
    }

    /// Handle a vote response.
    pub fn handle_vote_response(&mut self, from: NodeId, response: &VoteResponse) {
        if response.term > self.current_term {
            self.step_down(response.term);
            return;
        }

        if self.role != RaftRole::Candidate || response.term != self.current_term {
            return;
        }

        if response.vote_granted {
            self.votes_received.insert(from);

            if self.votes_received.len() >= self.quorum_size() {
                self.become_leader();
            }
        }
    }

    /// Become the leader.
    fn become_leader(&mut self) {
        self.role = RaftRole::Leader;
        self.leader_id = Some(self.node_id);

        let next_idx = self.log.last().map(|e| e.index + 1).unwrap_or(1);
        for &member in &self.cluster_members {
            if member != self.node_id {
                self.next_index.insert(member, next_idx);
                self.match_index.insert(member, 0);
            }
        }
    }

    /// Step down to follower.
    fn step_down(&mut self, new_term: u64) {
        self.current_term = new_term;
        self.role = RaftRole::Follower;
        self.voted_for = None;
        self.votes_received.clear();
    }

    /// Receive a heartbeat from the leader.
    pub fn receive_heartbeat(&mut self, leader_id: NodeId, term: u64) -> bool {
        if term < self.current_term {
            return false;
        }

        if term > self.current_term {
            self.step_down(term);
        }

        self.leader_id = Some(leader_id);
        self.last_heartbeat = Instant::now();
        self.role = RaftRole::Follower;
        true
    }

    /// Append a command to the log (leader only).
    pub fn append_command(&mut self, command: RaftCommand) -> Option<u64> {
        if self.role != RaftRole::Leader {
            return None;
        }

        let index = self.log.last().map(|e| e.index + 1).unwrap_or(1);
        self.log.push(LogEntry { term: self.current_term, index, command });

        Some(index)
    }

    /// Get uncommitted log entries.
    pub fn uncommitted_entries(&self) -> &[LogEntry] {
        let start = self.commit_index as usize;
        if start < self.log.len() {
            &self.log[start..]
        } else {
            &[]
        }
    }

    /// Advance the commit index (when quorum replicates).
    pub fn advance_commit(&mut self, new_commit_index: u64) {
        if new_commit_index > self.commit_index {
            self.commit_index = new_commit_index.min(self.log.last().map(|e| e.index).unwrap_or(0));
        }
    }

    /// Get log entries that need to be applied.
    pub fn entries_to_apply(&mut self) -> Vec<LogEntry> {
        let mut entries = Vec::new();
        while self.last_applied < self.commit_index {
            self.last_applied += 1;
            if let Some(entry) = self.log.iter().find(|e| e.index == self.last_applied) {
                entries.push(entry.clone());
            }
        }
        entries
    }

    /// Get raft state summary.
    pub fn state(&self) -> RaftState {
        RaftState {
            node_id: self.node_id,
            current_term: self.current_term,
            role: self.role,
            leader_id: self.leader_id,
            log_length: self.log.len(),
            commit_index: self.commit_index,
            last_applied: self.last_applied,
            cluster_size: self.cluster_members.len(),
        }
    }
}

/// Raft state summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftState {
    pub node_id: NodeId,
    pub current_term: u64,
    pub role: RaftRole,
    pub leader_id: Option<NodeId>,
    pub log_length: usize,
    pub commit_index: u64,
    pub last_applied: u64,
    pub cluster_size: usize,
}

/// Vote request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteRequest {
    pub term: u64,
    pub candidate_id: NodeId,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

/// Vote response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}

/// Split-brain detector.
pub struct SplitBrainDetector {
    /// Minimum cluster size for quorum.
    min_quorum: usize,
    /// Nodes we can reach.
    reachable: HashSet<NodeId>,
    /// Total known nodes.
    total_nodes: usize,
    /// Partition detection history.
    partition_events: VecDeque<PartitionEvent>,
    /// Max history to keep.
    max_history: usize,
}

/// A partition event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionEvent {
    /// When the partition was detected.
    pub detected_at_ms: u64,
    /// Nodes in our partition.
    pub our_partition_size: usize,
    /// Total known nodes.
    pub total_nodes: usize,
    /// Whether we have quorum.
    pub has_quorum: bool,
    /// Action taken.
    pub action: PartitionAction,
}

/// Action taken on partition detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PartitionAction {
    /// Continue operating (we have quorum).
    Continue,
    /// Entered read-only mode (minority partition).
    ReadOnly,
    /// Fenced off (cannot reach quorum).
    Fenced,
}

impl SplitBrainDetector {
    /// Create a new split-brain detector.
    pub fn new(total_nodes: usize) -> Self {
        Self {
            min_quorum: total_nodes / 2 + 1,
            reachable: HashSet::new(),
            total_nodes,
            partition_events: VecDeque::new(),
            max_history: 100,
        }
    }

    /// Update reachable nodes.
    pub fn update_reachable(&mut self, nodes: HashSet<NodeId>) {
        self.reachable = nodes;
    }

    /// Add a reachable node.
    pub fn mark_reachable(&mut self, node_id: NodeId) {
        self.reachable.insert(node_id);
    }

    /// Remove an unreachable node.
    pub fn mark_unreachable(&mut self, node_id: &NodeId) {
        self.reachable.remove(node_id);
    }

    /// Check if we have quorum.
    pub fn has_quorum(&self) -> bool {
        self.reachable.len() >= self.min_quorum
    }

    /// Check for split-brain and return recommended action.
    pub fn check(&mut self) -> PartitionAction {
        let action = if self.reachable.len() >= self.min_quorum {
            PartitionAction::Continue
        } else if self.reachable.is_empty() {
            PartitionAction::Fenced
        } else {
            PartitionAction::ReadOnly
        };

        let event = PartitionEvent {
            detected_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            our_partition_size: self.reachable.len(),
            total_nodes: self.total_nodes,
            has_quorum: self.has_quorum(),
            action: action.clone(),
        };

        self.partition_events.push_back(event);
        if self.partition_events.len() > self.max_history {
            self.partition_events.pop_front();
        }

        action
    }

    /// Get partition event history.
    pub fn history(&self) -> &VecDeque<PartitionEvent> {
        &self.partition_events
    }

    /// Update total node count.
    pub fn set_total_nodes(&mut self, count: usize) {
        self.total_nodes = count;
        self.min_quorum = count / 2 + 1;
    }
}

/// Work-stealing task queue for distributed scheduling.
pub struct WorkStealingQueue {
    /// Local task queues per node.
    queues: HashMap<NodeId, VecDeque<StealableTask>>,
    /// Steal threshold - steal when local queue drops below this.
    steal_threshold: usize,
    /// Maximum tasks to steal at once.
    max_steal_batch: usize,
    /// Total tasks stolen.
    total_stolen: u64,
}

/// A task that can be stolen by another node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealableTask {
    /// Task identifier.
    pub id: String,
    /// Task priority (higher = more important).
    pub priority: u32,
    /// Estimated cost (fuel units).
    pub estimated_cost: u64,
    /// Whether this task is stealable.
    pub stealable: bool,
    /// Node affinity (prefer this node).
    pub affinity: Option<NodeId>,
}

impl WorkStealingQueue {
    /// Create a new work-stealing queue.
    pub fn new(steal_threshold: usize, max_steal_batch: usize) -> Self {
        Self { queues: HashMap::new(), steal_threshold, max_steal_batch, total_stolen: 0 }
    }

    /// Add a task to a node's queue.
    pub fn push(&mut self, node_id: NodeId, task: StealableTask) {
        self.queues.entry(node_id).or_default().push_back(task);
    }

    /// Pop a task from a node's queue.
    pub fn pop(&mut self, node_id: &NodeId) -> Option<StealableTask> {
        self.queues.get_mut(node_id)?.pop_front()
    }

    /// Get queue length for a node.
    pub fn queue_len(&self, node_id: &NodeId) -> usize {
        self.queues.get(node_id).map(|q| q.len()).unwrap_or(0)
    }

    /// Check if a node should steal work.
    pub fn should_steal(&self, node_id: &NodeId) -> bool {
        self.queue_len(node_id) < self.steal_threshold
    }

    /// Find the best node to steal from (longest queue).
    pub fn find_steal_target(&self, requester: &NodeId) -> Option<NodeId> {
        self.queues
            .iter()
            .filter(|(id, queue)| *id != requester && queue.len() > self.steal_threshold)
            .max_by_key(|(_, queue)| queue.len())
            .map(|(id, _)| *id)
    }

    /// Steal tasks from one node for another.
    pub fn steal(&mut self, from: &NodeId, to: &NodeId) -> Vec<StealableTask> {
        let source_queue = match self.queues.get_mut(from) {
            Some(q) => q,
            None => return Vec::new(),
        };

        let steal_count = (source_queue.len() / 2).min(self.max_steal_batch);
        let mut stolen = Vec::new();

        // Steal from the back (lower priority tasks)
        for _ in 0..steal_count {
            if let Some(task) = source_queue.pop_back() {
                if task.stealable && task.affinity.as_ref() != Some(from) {
                    stolen.push(task);
                } else {
                    // Put it back if not stealable or has affinity
                    source_queue.push_back(task);
                    break;
                }
            }
        }

        self.total_stolen += stolen.len() as u64;

        // Add stolen tasks to the requester's queue
        let dest_queue = self.queues.entry(*to).or_default();
        for task in &stolen {
            dest_queue.push_back(task.clone());
        }

        stolen
    }

    /// Get total tasks stolen.
    pub fn total_stolen(&self) -> u64 {
        self.total_stolen
    }

    /// Get total tasks across all queues.
    pub fn total_tasks(&self) -> usize {
        self.queues.values().map(|q| q.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Raft Tests ===

    #[test]
    fn test_raft_node_creation() {
        let node = RaftNode::new(NodeId::new(1), Duration::from_millis(150));
        assert_eq!(node.role(), RaftRole::Follower);
        assert_eq!(node.current_term(), 0);
        assert!(!node.is_leader());
    }

    #[test]
    fn test_raft_start_election() {
        let mut node = RaftNode::new(NodeId::new(1), Duration::from_millis(150));
        node.add_member(NodeId::new(2));
        node.add_member(NodeId::new(3));

        let vote_req = node.start_election();
        assert_eq!(node.role(), RaftRole::Candidate);
        assert_eq!(node.current_term(), 1);
        assert_eq!(vote_req.term, 1);
        assert_eq!(vote_req.candidate_id, NodeId::new(1));
    }

    #[test]
    fn test_raft_vote_granted() {
        let mut node = RaftNode::new(NodeId::new(2), Duration::from_millis(150));

        let request = VoteRequest {
            term: 1,
            candidate_id: NodeId::new(1),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response = node.handle_vote_request(&request);
        assert!(response.vote_granted);
    }

    #[test]
    fn test_raft_vote_denied_already_voted() {
        let mut node = RaftNode::new(NodeId::new(3), Duration::from_millis(150));

        // Vote for node 1
        let req1 = VoteRequest {
            term: 1,
            candidate_id: NodeId::new(1),
            last_log_index: 0,
            last_log_term: 0,
        };
        node.handle_vote_request(&req1);

        // Deny vote for node 2 in same term
        let req2 = VoteRequest {
            term: 1,
            candidate_id: NodeId::new(2),
            last_log_index: 0,
            last_log_term: 0,
        };
        let response = node.handle_vote_request(&req2);
        assert!(!response.vote_granted);
    }

    #[test]
    fn test_raft_leader_election_with_quorum() {
        let mut node = RaftNode::new(NodeId::new(1), Duration::from_millis(150));
        node.add_member(NodeId::new(2));
        node.add_member(NodeId::new(3));

        node.start_election();
        assert_eq!(node.role(), RaftRole::Candidate);

        // Receive vote from node 2 (now have 2/3 = quorum)
        node.handle_vote_response(NodeId::new(2), &VoteResponse { term: 1, vote_granted: true });

        assert_eq!(node.role(), RaftRole::Leader);
        assert!(node.is_leader());
    }

    #[test]
    fn test_raft_append_command_leader_only() {
        let mut follower = RaftNode::new(NodeId::new(1), Duration::from_millis(150));
        assert!(follower.append_command(RaftCommand::Noop).is_none());

        // Make it a leader (single node cluster)
        let mut leader = RaftNode::new(NodeId::new(1), Duration::from_millis(150));
        leader.start_election();
        // Single node = immediate leader
        assert!(leader.is_leader());

        let index = leader.append_command(RaftCommand::AddNode { node_id: NodeId::new(2) });
        assert_eq!(index, Some(1));
    }

    #[test]
    fn test_raft_heartbeat() {
        let mut node = RaftNode::new(NodeId::new(2), Duration::from_millis(150));
        assert!(node.receive_heartbeat(NodeId::new(1), 1));
        assert_eq!(node.leader_id(), Some(NodeId::new(1)));
        assert_eq!(node.current_term(), 1);
    }

    #[test]
    fn test_raft_heartbeat_old_term_rejected() {
        let mut node = RaftNode::new(NodeId::new(2), Duration::from_millis(150));
        node.receive_heartbeat(NodeId::new(1), 5);
        // Heartbeat with old term should be rejected
        assert!(!node.receive_heartbeat(NodeId::new(3), 3));
    }

    #[test]
    fn test_raft_state() {
        let node = RaftNode::new(NodeId::new(1), Duration::from_millis(150));
        let state = node.state();
        assert_eq!(state.current_term, 0);
        assert_eq!(state.role, RaftRole::Follower);
        assert_eq!(state.cluster_size, 1);
    }

    // === Split-Brain Detector Tests ===

    #[test]
    fn test_split_brain_has_quorum() {
        let mut detector = SplitBrainDetector::new(5);
        detector.mark_reachable(NodeId::new(1));
        detector.mark_reachable(NodeId::new(2));
        detector.mark_reachable(NodeId::new(3));

        assert!(detector.has_quorum()); // 3/5 >= 3 (quorum)
    }

    #[test]
    fn test_split_brain_no_quorum() {
        let mut detector = SplitBrainDetector::new(5);
        detector.mark_reachable(NodeId::new(1));
        detector.mark_reachable(NodeId::new(2));

        assert!(!detector.has_quorum()); // 2/5 < 3
    }

    #[test]
    fn test_split_brain_fenced() {
        let mut detector = SplitBrainDetector::new(5);
        let action = detector.check();
        assert!(matches!(action, PartitionAction::Fenced));
    }

    #[test]
    fn test_split_brain_read_only() {
        let mut detector = SplitBrainDetector::new(5);
        detector.mark_reachable(NodeId::new(1));
        let action = detector.check();
        assert!(matches!(action, PartitionAction::ReadOnly));
    }

    #[test]
    fn test_split_brain_continue() {
        let mut detector = SplitBrainDetector::new(3);
        detector.mark_reachable(NodeId::new(1));
        detector.mark_reachable(NodeId::new(2));
        let action = detector.check();
        assert!(matches!(action, PartitionAction::Continue));
    }

    // === Work-Stealing Tests ===

    #[test]
    fn test_work_stealing_push_pop() {
        let mut queue = WorkStealingQueue::new(2, 5);
        let node = NodeId::new(1);

        queue.push(
            node,
            StealableTask {
                id: "task-1".to_string(),
                priority: 1,
                estimated_cost: 100,
                stealable: true,
                affinity: None,
            },
        );

        assert_eq!(queue.queue_len(&node), 1);
        let task = queue.pop(&node).unwrap();
        assert_eq!(task.id, "task-1");
        assert_eq!(queue.queue_len(&node), 0);
    }

    #[test]
    fn test_work_stealing_should_steal() {
        let mut queue = WorkStealingQueue::new(2, 5);
        let node = NodeId::new(1);

        // Empty queue: should steal
        assert!(queue.should_steal(&node));

        // Add tasks above threshold: should not steal
        for i in 0..3 {
            queue.push(
                node,
                StealableTask {
                    id: format!("task-{i}"),
                    priority: 1,
                    estimated_cost: 100,
                    stealable: true,
                    affinity: None,
                },
            );
        }
        assert!(!queue.should_steal(&node));
    }

    #[test]
    fn test_work_stealing_steal() {
        let mut queue = WorkStealingQueue::new(2, 5);
        let overloaded = NodeId::new(1);
        let idle = NodeId::new(2);

        for i in 0..6 {
            queue.push(
                overloaded,
                StealableTask {
                    id: format!("task-{i}"),
                    priority: 1,
                    estimated_cost: 100,
                    stealable: true,
                    affinity: None,
                },
            );
        }

        let stolen = queue.steal(&overloaded, &idle);
        assert!(!stolen.is_empty());
        assert!(queue.queue_len(&idle) > 0);
        assert!(queue.total_stolen() > 0);
    }

    #[test]
    fn test_work_stealing_respects_affinity() {
        let mut queue = WorkStealingQueue::new(2, 5);
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);

        // Task with affinity to node1 shouldn't be stolen
        queue.push(
            node1,
            StealableTask {
                id: "affine-task".to_string(),
                priority: 1,
                estimated_cost: 100,
                stealable: true,
                affinity: Some(node1),
            },
        );

        let stolen = queue.steal(&node1, &node2);
        assert!(stolen.is_empty());
    }

    #[test]
    fn test_work_stealing_find_target() {
        let mut queue = WorkStealingQueue::new(2, 5);
        let node1 = NodeId::new(1);
        let node2 = NodeId::new(2);

        for i in 0..5 {
            queue.push(
                node1,
                StealableTask {
                    id: format!("task-{i}"),
                    priority: 1,
                    estimated_cost: 100,
                    stealable: true,
                    affinity: None,
                },
            );
        }

        let target = queue.find_steal_target(&node2);
        assert_eq!(target, Some(node1));
    }
}
