//! Causal analysis engine for time-travel debugging.
//!
//! Traces cause-and-effect chains through execution events to find root causes
//! of failures, build dependency graphs, and answer "why did this happen?" queries.
//!
//! # Features
//!
//! - **Causal chains**: Build chains of events linked by data/control dependencies
//! - **Root cause analysis**: Find the originating event that led to a failure
//! - **Execution slicing**: Extract minimal event subsequences relevant to a query
//! - **Impact analysis**: Given an event, find everything it affected downstream

use super::{EventId, EventType, ExecutionEvent};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// A directed edge in the causal graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEdge {
    /// Source event ID (cause).
    pub from: EventId,
    /// Destination event ID (effect).
    pub to: EventId,
    /// Type of causal relationship.
    pub relation: CausalRelation,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f64,
}

/// Types of causal relationships between events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CausalRelation {
    /// Data dependency: event B reads data written by event A.
    DataDependency { address: u64 },
    /// Control dependency: event B's execution depends on a branch in event A.
    ControlDependency,
    /// Call dependency: event B is a function called by event A.
    CallDependency,
    /// Return dependency: event B returns to event A.
    ReturnDependency,
    /// WASI dependency: event B uses a resource opened/modified by event A.
    WasiDependency { resource: String },
    /// Temporal ordering (weak): A happened before B.
    TemporalOrder,
}

impl CausalRelation {
    fn base_confidence(&self) -> f64 {
        match self {
            Self::DataDependency { .. } => 0.95,
            Self::ControlDependency => 0.85,
            Self::CallDependency => 0.99,
            Self::ReturnDependency => 0.99,
            Self::WasiDependency { .. } => 0.90,
            Self::TemporalOrder => 0.30,
        }
    }
}

/// A chain of causally linked events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChain {
    /// Ordered list of event IDs from root cause to final effect.
    pub events: Vec<EventId>,
    /// Edges connecting consecutive events.
    pub edges: Vec<CausalEdge>,
    /// Overall confidence in this chain.
    pub confidence: f64,
    /// Human-readable description of the chain.
    pub description: String,
}

impl CausalChain {
    /// Length of the chain (number of events).
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Root cause event (first in chain).
    pub fn root_cause(&self) -> Option<EventId> {
        self.events.first().copied()
    }

    /// Final effect event (last in chain).
    pub fn final_effect(&self) -> Option<EventId> {
        self.events.last().copied()
    }
}

/// Result of a root-cause analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseResult {
    /// The target event being analyzed.
    pub target_event: EventId,
    /// Candidate root causes, ordered by confidence.
    pub candidates: Vec<RootCauseCandidate>,
    /// Total events analyzed.
    pub events_analyzed: usize,
}

/// A candidate root cause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseCandidate {
    /// The event identified as potential root cause.
    pub event_id: EventId,
    /// The causal chain from this event to the target.
    pub chain: CausalChain,
    /// Confidence score.
    pub confidence: f64,
    /// Why this is considered a root cause.
    pub reason: String,
}

/// Result of an execution slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSlice {
    /// Events included in the slice.
    pub events: Vec<EventId>,
    /// Percentage of original events retained.
    pub reduction_ratio: f64,
    /// Criterion used for slicing.
    pub criterion: SliceCriterion,
}

/// Criteria for execution slicing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SliceCriterion {
    /// All events that affect a given event.
    BackwardSlice { target: EventId },
    /// All events affected by a given event.
    ForwardSlice { source: EventId },
    /// Events affecting a specific memory address.
    MemorySlice { address: u64 },
    /// Events within a function call tree.
    FunctionSlice { function_name: String },
}

/// The causal analysis engine.
pub struct CausalAnalyzer {
    /// All execution events.
    events: Vec<ExecutionEvent>,
    /// Event index for fast lookup.
    event_map: HashMap<EventId, usize>,
    /// Forward edges: event_id -> [(target_id, edge)].
    forward_edges: HashMap<EventId, Vec<CausalEdge>>,
    /// Backward edges: event_id -> [(source_id, edge)].
    backward_edges: HashMap<EventId, Vec<CausalEdge>>,
    /// Memory write index: address -> last writer event_id.
    memory_writers: HashMap<u64, Vec<EventId>>,
    /// Function call stack used during analysis.
    call_stack: Vec<EventId>,
}

impl CausalAnalyzer {
    /// Build a causal analyzer from a set of execution events.
    pub fn from_events(events: Vec<ExecutionEvent>) -> Self {
        let mut analyzer = Self {
            event_map: events.iter().enumerate().map(|(i, e)| (e.id, i)).collect(),
            events,
            forward_edges: HashMap::new(),
            backward_edges: HashMap::new(),
            memory_writers: HashMap::new(),
            call_stack: Vec::new(),
        };
        analyzer.build_causal_graph();
        analyzer
    }

    /// Build the causal graph by analyzing all events.
    fn build_causal_graph(&mut self) {
        let events = self.events.clone();

        for (i, event) in events.iter().enumerate() {
            // Data dependencies from memory writes/reads
            for mc in &event.memory_changes {
                match event.event_type {
                    EventType::MemoryWrite => {
                        self.memory_writers.entry(mc.address).or_default().push(event.id);
                    }
                    EventType::MemoryRead => {
                        if let Some(writers) = self.memory_writers.get(&mc.address) {
                            if let Some(&last_writer) = writers.last() {
                                self.add_edge(CausalEdge {
                                    from: last_writer,
                                    to: event.id,
                                    relation: CausalRelation::DataDependency {
                                        address: mc.address,
                                    },
                                    confidence: CausalRelation::DataDependency {
                                        address: mc.address,
                                    }
                                    .base_confidence(),
                                });
                            }
                        }
                    }
                    _ => {
                        if !mc.new_value.is_empty() && mc.old_value != mc.new_value {
                            self.memory_writers.entry(mc.address).or_default().push(event.id);
                        }
                    }
                }
            }

            // Call/return dependencies
            match event.event_type {
                EventType::FunctionCall => {
                    if let Some(&caller) = self.call_stack.last() {
                        self.add_edge(CausalEdge {
                            from: caller,
                            to: event.id,
                            relation: CausalRelation::CallDependency,
                            confidence: CausalRelation::CallDependency.base_confidence(),
                        });
                    }
                    self.call_stack.push(event.id);
                }
                EventType::FunctionReturn => {
                    if let Some(call_id) = self.call_stack.pop() {
                        self.add_edge(CausalEdge {
                            from: call_id,
                            to: event.id,
                            relation: CausalRelation::ReturnDependency,
                            confidence: CausalRelation::ReturnDependency.base_confidence(),
                        });
                    }
                }
                _ => {}
            }

            // WASI dependencies
            if let Some(ref wasi) = event.wasi_call {
                if event.event_type == EventType::WasiReturn {
                    // Find matching WASI call
                    for j in (0..i).rev() {
                        let prev = &events[j];
                        if prev.event_type == EventType::WasiCall {
                            if let Some(ref prev_wasi) = prev.wasi_call {
                                if prev_wasi.function == wasi.function {
                                    self.add_edge(CausalEdge {
                                        from: prev.id,
                                        to: event.id,
                                        relation: CausalRelation::WasiDependency {
                                            resource: wasi.function.clone(),
                                        },
                                        confidence: CausalRelation::WasiDependency {
                                            resource: wasi.function.clone(),
                                        }
                                        .base_confidence(),
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn add_edge(&mut self, edge: CausalEdge) {
        self.backward_edges.entry(edge.to).or_default().push(edge.clone());
        self.forward_edges.entry(edge.from).or_default().push(edge);
    }

    /// Find the root causes of a target event.
    pub fn find_root_causes(
        &self,
        target: EventId,
        max_depth: usize,
    ) -> RootCauseResult {
        let mut candidates = Vec::new();
        let mut visited = HashSet::new();
        let mut events_analyzed = 0;

        // BFS backward through causal graph
        let mut queue: VecDeque<(EventId, Vec<EventId>, Vec<CausalEdge>, f64)> = VecDeque::new();
        queue.push_back((target, vec![target], Vec::new(), 1.0));

        while let Some((current, path, edges, confidence)) = queue.pop_front() {
            if path.len() > max_depth + 1 {
                continue;
            }
            events_analyzed += 1;

            let backward = self.backward_edges.get(&current);
            let is_root = backward.map_or(true, |e| e.is_empty());

            if is_root && current != target {
                let mut chain_events = path.clone();
                chain_events.reverse();
                let mut chain_edges = edges.clone();
                chain_edges.reverse();

                let event = self.get_event(current);
                let reason = match event {
                    Some(e) => format_root_cause_reason(e),
                    None => "Unknown event".to_string(),
                };

                candidates.push(RootCauseCandidate {
                    event_id: current,
                    chain: CausalChain {
                        events: chain_events,
                        edges: chain_edges,
                        confidence,
                        description: format!("Chain of {} events to target", path.len()),
                    },
                    confidence,
                    reason,
                });
                continue;
            }

            if let Some(back_edges) = backward {
                for edge in back_edges {
                    if !visited.contains(&edge.from) {
                        visited.insert(edge.from);
                        let mut new_path = path.clone();
                        new_path.push(edge.from);
                        let mut new_edges = edges.clone();
                        new_edges.push(edge.clone());
                        let new_confidence = confidence * edge.confidence;
                        queue.push_back((edge.from, new_path, new_edges, new_confidence));
                    }
                }
            }
        }

        candidates.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        RootCauseResult { target_event: target, candidates, events_analyzed }
    }

    /// Compute a backward slice: all events that influence the target event.
    pub fn backward_slice(&self, target: EventId) -> ExecutionSlice {
        let mut slice_events = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(target);
        slice_events.insert(target);

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = self.backward_edges.get(&current) {
                for edge in edges {
                    if slice_events.insert(edge.from) {
                        queue.push_back(edge.from);
                    }
                }
            }
        }

        let mut events: Vec<EventId> = slice_events.into_iter().collect();
        events.sort();

        let reduction = if self.events.is_empty() {
            0.0
        } else {
            1.0 - (events.len() as f64 / self.events.len() as f64)
        };

        ExecutionSlice {
            events,
            reduction_ratio: reduction,
            criterion: SliceCriterion::BackwardSlice { target },
        }
    }

    /// Compute a forward slice: all events affected by the source event.
    pub fn forward_slice(&self, source: EventId) -> ExecutionSlice {
        let mut slice_events = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(source);
        slice_events.insert(source);

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = self.forward_edges.get(&current) {
                for edge in edges {
                    if slice_events.insert(edge.to) {
                        queue.push_back(edge.to);
                    }
                }
            }
        }

        let mut events: Vec<EventId> = slice_events.into_iter().collect();
        events.sort();

        let reduction = if self.events.is_empty() {
            0.0
        } else {
            1.0 - (events.len() as f64 / self.events.len() as f64)
        };

        ExecutionSlice {
            events,
            reduction_ratio: reduction,
            criterion: SliceCriterion::ForwardSlice { source },
        }
    }

    /// Find all events that touch a specific memory address.
    pub fn memory_slice(&self, address: u64) -> ExecutionSlice {
        let events: Vec<EventId> = self
            .events
            .iter()
            .filter(|e| e.memory_changes.iter().any(|mc| mc.address == address))
            .map(|e| e.id)
            .collect();

        let reduction = if self.events.is_empty() {
            0.0
        } else {
            1.0 - (events.len() as f64 / self.events.len() as f64)
        };

        ExecutionSlice {
            events,
            reduction_ratio: reduction,
            criterion: SliceCriterion::MemorySlice { address },
        }
    }

    /// Get the causal chain between two events, if one exists.
    pub fn find_chain(&self, from: EventId, to: EventId, max_depth: usize) -> Option<CausalChain> {
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(EventId, Vec<EventId>, Vec<CausalEdge>, f64)> = VecDeque::new();
        queue.push_back((from, vec![from], Vec::new(), 1.0));
        visited.insert(from);

        while let Some((current, path, edges, confidence)) = queue.pop_front() {
            if current == to {
                return Some(CausalChain {
                    events: path,
                    edges,
                    confidence,
                    description: format!("Chain from event {} to event {}", from, to),
                });
            }

            if path.len() > max_depth {
                continue;
            }

            if let Some(fwd_edges) = self.forward_edges.get(&current) {
                for edge in fwd_edges {
                    if visited.insert(edge.to) {
                        let mut new_path = path.clone();
                        new_path.push(edge.to);
                        let mut new_edges = edges.clone();
                        new_edges.push(edge.clone());
                        queue.push_back((edge.to, new_path, new_edges, confidence * edge.confidence));
                    }
                }
            }
        }

        None
    }

    /// Get summary statistics about the causal graph.
    pub fn stats(&self) -> CausalGraphStats {
        let total_edges: usize = self.forward_edges.values().map(|v| v.len()).sum();
        let mut data_deps = 0;
        let mut call_deps = 0;
        let mut wasi_deps = 0;

        for edges in self.forward_edges.values() {
            for edge in edges {
                match edge.relation {
                    CausalRelation::DataDependency { .. } => data_deps += 1,
                    CausalRelation::CallDependency | CausalRelation::ReturnDependency => {
                        call_deps += 1
                    }
                    CausalRelation::WasiDependency { .. } => wasi_deps += 1,
                    _ => {}
                }
            }
        }

        let root_count = self
            .events
            .iter()
            .filter(|e| {
                self.backward_edges
                    .get(&e.id)
                    .map_or(true, |edges| edges.is_empty())
            })
            .count();

        CausalGraphStats {
            total_events: self.events.len(),
            total_edges,
            data_dependencies: data_deps,
            call_dependencies: call_deps,
            wasi_dependencies: wasi_deps,
            root_events: root_count,
        }
    }

    fn get_event(&self, id: EventId) -> Option<&ExecutionEvent> {
        self.event_map.get(&id).and_then(|&idx| self.events.get(idx))
    }
}

/// Statistics about the causal graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphStats {
    pub total_events: usize,
    pub total_edges: usize,
    pub data_dependencies: usize,
    pub call_dependencies: usize,
    pub wasi_dependencies: usize,
    pub root_events: usize,
}

fn format_root_cause_reason(event: &ExecutionEvent) -> String {
    match event.event_type {
        EventType::MemoryWrite => format!(
            "Memory write at address {:#x}",
            event.memory_changes.first().map(|mc| mc.address).unwrap_or(0)
        ),
        EventType::FunctionCall => format!(
            "Function call to '{}'",
            event.function_name.as_deref().unwrap_or("unknown")
        ),
        EventType::WasiCall => format!(
            "WASI call to '{}'",
            event.wasi_call.as_ref().map(|w| w.function.as_str()).unwrap_or("unknown")
        ),
        EventType::Exception => "Exception occurred".to_string(),
        EventType::Start => "Execution started".to_string(),
        _ => format!("Event type: {:?}", event.event_type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::timetravel::event::{MemoryChange, WasiCallInfo};

    fn make_instruction(id: u64, ip: u64) -> ExecutionEvent {
        ExecutionEvent::new(id, EventType::Instruction, ip)
    }

    fn make_write(id: u64, addr: u64, old: Vec<u8>, new: Vec<u8>) -> ExecutionEvent {
        ExecutionEvent::new(id, EventType::MemoryWrite, 0x1000)
            .with_memory_change(MemoryChange::new(addr, old, new))
    }

    fn make_read(id: u64, addr: u64, val: Vec<u8>) -> ExecutionEvent {
        ExecutionEvent::new(id, EventType::MemoryRead, 0x1000)
            .with_memory_change(MemoryChange::read(addr, val))
    }

    fn make_call(id: u64, name: &str) -> ExecutionEvent {
        ExecutionEvent::new(id, EventType::FunctionCall, 0x2000).with_function(name)
    }

    fn make_ret(id: u64, name: &str) -> ExecutionEvent {
        ExecutionEvent::new(id, EventType::FunctionReturn, 0x2000).with_function(name)
    }

    #[test]
    fn test_empty_analyzer() {
        let analyzer = CausalAnalyzer::from_events(vec![]);
        let stats = analyzer.stats();
        assert_eq!(stats.total_events, 0);
        assert_eq!(stats.total_edges, 0);
    }

    #[test]
    fn test_data_dependency_detection() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_read(1, 0x100, vec![42]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let stats = analyzer.stats();
        assert_eq!(stats.data_dependencies, 1);
    }

    #[test]
    fn test_call_dependency_detection() {
        let events = vec![
            make_call(0, "main"),
            make_call(1, "helper"),
            make_ret(2, "helper"),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let stats = analyzer.stats();
        assert!(stats.call_dependencies >= 2); // call + return
    }

    #[test]
    fn test_find_chain() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_read(1, 0x100, vec![42]),
            make_write(2, 0x200, vec![0], vec![99]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);

        let chain = analyzer.find_chain(0, 1, 10);
        assert!(chain.is_some());
        let chain = chain.unwrap();
        assert_eq!(chain.events.len(), 2);
        assert_eq!(chain.events[0], 0);
        assert_eq!(chain.events[1], 1);
    }

    #[test]
    fn test_find_chain_no_path() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_write(1, 0x200, vec![0], vec![99]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let chain = analyzer.find_chain(0, 1, 10);
        assert!(chain.is_none());
    }

    #[test]
    fn test_backward_slice() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_instruction(1, 0x1010),
            make_read(2, 0x100, vec![42]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let slice = analyzer.backward_slice(2);

        // Event 2 depends on event 0 (via data dep), event 1 is independent
        assert!(slice.events.contains(&2));
        assert!(slice.events.contains(&0));
        assert!(!slice.events.contains(&1));
        assert!(slice.reduction_ratio > 0.0);
    }

    #[test]
    fn test_forward_slice() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_instruction(1, 0x1010),
            make_read(2, 0x100, vec![42]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let slice = analyzer.forward_slice(0);

        assert!(slice.events.contains(&0));
        assert!(slice.events.contains(&2));
        assert!(!slice.events.contains(&1));
    }

    #[test]
    fn test_memory_slice() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_write(1, 0x200, vec![0], vec![99]),
            make_read(2, 0x100, vec![42]),
            make_write(3, 0x100, vec![42], vec![84]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let slice = analyzer.memory_slice(0x100);

        assert_eq!(slice.events.len(), 3); // events 0, 2, 3
        assert!(!slice.events.contains(&1));
    }

    #[test]
    fn test_root_cause_analysis() {
        let events = vec![
            make_write(0, 0x100, vec![0], vec![42]),
            make_read(1, 0x100, vec![42]),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let result = analyzer.find_root_causes(1, 10);

        assert_eq!(result.target_event, 1);
        assert!(!result.candidates.is_empty());
        assert_eq!(result.candidates[0].event_id, 0);
    }

    #[test]
    fn test_causal_chain_methods() {
        let chain = CausalChain {
            events: vec![1, 2, 3],
            edges: Vec::new(),
            confidence: 0.9,
            description: "test".to_string(),
        };
        assert_eq!(chain.len(), 3);
        assert!(!chain.is_empty());
        assert_eq!(chain.root_cause(), Some(1));
        assert_eq!(chain.final_effect(), Some(3));
    }

    #[test]
    fn test_wasi_dependency() {
        let events = vec![
            ExecutionEvent::new(0, EventType::WasiCall, 0x1000)
                .with_wasi_call(WasiCallInfo::new("fd_write", vec![])),
            ExecutionEvent::new(1, EventType::WasiReturn, 0x1000)
                .with_wasi_call(WasiCallInfo::new("fd_write", vec![])),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let stats = analyzer.stats();
        assert_eq!(stats.wasi_dependencies, 1);
    }

    #[test]
    fn test_complex_causal_graph() {
        let events = vec![
            make_call(0, "main"),
            make_write(1, 0x100, vec![0], vec![10]),
            make_call(2, "process"),
            make_read(3, 0x100, vec![10]),
            make_write(4, 0x200, vec![0], vec![20]),
            make_ret(5, "process"),
            make_read(6, 0x200, vec![20]),
            make_ret(7, "main"),
        ];
        let analyzer = CausalAnalyzer::from_events(events);
        let stats = analyzer.stats();

        assert_eq!(stats.total_events, 8);
        assert!(stats.total_edges >= 4);
        assert!(stats.data_dependencies >= 2);
        assert!(stats.call_dependencies >= 2);

        // Event 6 reads from 0x200 written by event 4
        let chain = analyzer.find_chain(4, 6, 10);
        assert!(chain.is_some());
    }
}
