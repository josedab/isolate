//! Execution differential analysis for time-travel debugging.
//!
//! Compare two execution runs or analyze state changes between positions.

use super::{EventType, ExecutionEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Difference between two execution states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    /// Starting position.
    pub from_position: usize,
    /// Ending position.
    pub to_position: usize,
    /// Memory changes in the range.
    pub memory_changes: Vec<MemoryDiff>,
    /// Register changes in the range.
    pub register_changes: Vec<RegisterDiff>,
    /// Events in the range.
    pub event_changes: Vec<EventDiff>,
    /// Summary statistics.
    pub summary: DiffSummary,
}

/// A memory difference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDiff {
    /// Memory offset.
    pub offset: u64,
    /// Old value bytes.
    pub old_value: Vec<u8>,
    /// New value bytes.
    pub new_value: Vec<u8>,
    /// Size of the change in bytes.
    pub size: usize,
}

/// A register value difference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDiff {
    /// Register name.
    pub name: String,
    /// Old value.
    pub old_value: u64,
    /// New value.
    pub new_value: u64,
}

/// An event difference entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDiff {
    /// Position in the timeline.
    pub position: usize,
    /// Event type.
    pub event_type: EventType,
    /// Human-readable description.
    pub description: String,
}

/// Summary of differences between two states.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Total number of memory changes.
    pub total_memory_changes: usize,
    /// Total bytes changed.
    pub total_bytes_changed: usize,
    /// Total register changes.
    pub total_register_changes: usize,
    /// Events in the analyzed range.
    pub events_in_range: usize,
    /// Unique event types seen.
    pub unique_event_types: Vec<EventType>,
}

/// Result of comparing two execution runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunComparison {
    /// Identifier for run A.
    pub run_a_id: String,
    /// Identifier for run B.
    pub run_b_id: String,
    /// Position where runs diverge, if at all.
    pub divergence_point: Option<usize>,
    /// Whether the behavior matches.
    pub behavior_match: bool,
    /// Whether the outputs match.
    pub output_match: bool,
    /// Detailed differences.
    pub differences: Vec<RunDifference>,
    /// Comparison summary.
    pub summary: ComparisonSummary,
}

/// A single difference between two runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDifference {
    /// Category of the difference.
    pub category: DiffCategory,
    /// Description of the difference.
    pub description: String,
    /// Position in run A.
    pub position_a: Option<usize>,
    /// Position in run B.
    pub position_b: Option<usize>,
}

/// Category of a run difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffCategory {
    Memory,
    Control,
    Output,
    Timing,
    EventSequence,
}

/// Summary of a run comparison.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComparisonSummary {
    /// Total number of differences.
    pub total_differences: usize,
    /// Difference counts per category.
    pub categories: HashMap<String, usize>,
    /// Position of first divergence.
    pub first_divergence: Option<usize>,
    /// Similarity score between 0.0 and 1.0.
    pub similarity_score: f64,
}

/// Differ for analyzing execution differences.
pub struct ExecutionDiffer;

impl ExecutionDiffer {
    /// Compute the state diff between two positions in an event sequence.
    pub fn diff_states(events: &[ExecutionEvent], from: usize, to: usize) -> StateDiff {
        let (start, end) = if from <= to { (from, to) } else { (to, from) };
        let range_events = &events[start..end.min(events.len())];

        let mut memory_changes = Vec::new();
        let mut register_changes = Vec::new();
        let mut event_changes = Vec::new();
        let mut unique_types: Vec<EventType> = Vec::new();

        for (offset, event) in range_events.iter().enumerate() {
            let pos = start + offset;

            // Collect memory changes
            for mc in &event.memory_changes {
                memory_changes.push(MemoryDiff {
                    offset: mc.address,
                    old_value: mc.old_value.clone(),
                    new_value: mc.new_value.clone(),
                    size: mc.new_value.len(),
                });
            }

            // Collect register changes
            for rc in &event.register_changes {
                register_changes.push(RegisterDiff {
                    name: rc.name.clone(),
                    old_value: rc.old_value,
                    new_value: rc.new_value,
                });
            }

            // Record event
            event_changes.push(EventDiff {
                position: pos,
                event_type: event.event_type.clone(),
                description: format!("Event {} at ip 0x{:x}", event.id, event.instruction_pointer),
            });

            if !unique_types.contains(&event.event_type) {
                unique_types.push(event.event_type.clone());
            }
        }

        let total_bytes_changed: usize = memory_changes.iter().map(|m| m.size).sum();

        let summary = DiffSummary {
            total_memory_changes: memory_changes.len(),
            total_bytes_changed,
            total_register_changes: register_changes.len(),
            events_in_range: range_events.len(),
            unique_event_types: unique_types,
        };

        StateDiff {
            from_position: from,
            to_position: to,
            memory_changes,
            register_changes,
            event_changes,
            summary,
        }
    }

    /// Compare two execution runs.
    pub fn compare_runs(events_a: &[ExecutionEvent], events_b: &[ExecutionEvent]) -> RunComparison {
        let divergence_point = Self::find_divergence(events_a, events_b);
        let mut differences = Vec::new();
        let mut categories: HashMap<String, usize> = HashMap::new();

        // Check length difference
        if events_a.len() != events_b.len() {
            differences.push(RunDifference {
                category: DiffCategory::EventSequence,
                description: format!(
                    "Run A has {} events, Run B has {} events",
                    events_a.len(),
                    events_b.len()
                ),
                position_a: Some(events_a.len()),
                position_b: Some(events_b.len()),
            });
            *categories.entry("EventSequence".to_string()).or_insert(0) += 1;
        }

        // Compare overlapping events
        let min_len = events_a.len().min(events_b.len());
        for i in 0..min_len {
            let ea = &events_a[i];
            let eb = &events_b[i];

            if ea.event_type != eb.event_type {
                differences.push(RunDifference {
                    category: DiffCategory::Control,
                    description: format!(
                        "Event type differs at position {}: {:?} vs {:?}",
                        i, ea.event_type, eb.event_type
                    ),
                    position_a: Some(i),
                    position_b: Some(i),
                });
                *categories.entry("Control".to_string()).or_insert(0) += 1;
            }

            if ea.instruction_pointer != eb.instruction_pointer {
                differences.push(RunDifference {
                    category: DiffCategory::Control,
                    description: format!(
                        "Instruction pointer differs at position {}: 0x{:x} vs 0x{:x}",
                        i, ea.instruction_pointer, eb.instruction_pointer
                    ),
                    position_a: Some(i),
                    position_b: Some(i),
                });
                *categories.entry("Control".to_string()).or_insert(0) += 1;
            }

            // Compare memory changes
            if ea.memory_changes.len() != eb.memory_changes.len() {
                differences.push(RunDifference {
                    category: DiffCategory::Memory,
                    description: format!(
                        "Memory change count differs at position {}: {} vs {}",
                        i,
                        ea.memory_changes.len(),
                        eb.memory_changes.len()
                    ),
                    position_a: Some(i),
                    position_b: Some(i),
                });
                *categories.entry("Memory".to_string()).or_insert(0) += 1;
            }
        }

        // Check output match (compare events with data)
        let output_match = events_a
            .iter()
            .filter(|e| e.data.is_some())
            .zip(events_b.iter().filter(|e| e.data.is_some()))
            .all(|(a, b)| a.data == b.data);

        let behavior_match = divergence_point.is_none() && events_a.len() == events_b.len();

        let total_differences = differences.len();
        let similarity_score = if min_len == 0 {
            if events_a.is_empty() && events_b.is_empty() {
                1.0
            } else {
                0.0
            }
        } else {
            let matching = min_len.saturating_sub(total_differences);
            matching as f64 / min_len as f64
        };

        RunComparison {
            run_a_id: "run_a".to_string(),
            run_b_id: "run_b".to_string(),
            divergence_point,
            behavior_match,
            output_match,
            differences,
            summary: ComparisonSummary {
                total_differences,
                categories,
                first_divergence: divergence_point,
                similarity_score,
            },
        }
    }

    /// Find the first position where two event sequences diverge.
    pub fn find_divergence(
        events_a: &[ExecutionEvent],
        events_b: &[ExecutionEvent],
    ) -> Option<usize> {
        let min_len = events_a.len().min(events_b.len());

        for i in 0..min_len {
            if events_a[i].event_type != events_b[i].event_type
                || events_a[i].instruction_pointer != events_b[i].instruction_pointer
            {
                return Some(i);
            }
        }

        if events_a.len() != events_b.len() {
            Some(min_len)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::timetravel::event::{MemoryChange, RegisterChange};

    fn make_events(count: usize) -> Vec<ExecutionEvent> {
        (0..count)
            .map(|i| ExecutionEvent::new(i as u64, EventType::Instruction, 0x1000 + i as u64))
            .collect()
    }

    #[test]
    fn test_diff_states_basic() {
        let events = make_events(5);
        let diff = ExecutionDiffer::diff_states(&events, 1, 4);

        assert_eq!(diff.from_position, 1);
        assert_eq!(diff.to_position, 4);
        assert_eq!(diff.summary.events_in_range, 3);
    }

    #[test]
    fn test_diff_states_with_memory_changes() {
        let mut events = make_events(5);
        events[2] = ExecutionEvent::new(2, EventType::MemoryWrite, 0x1002)
            .with_memory_change(MemoryChange::new(0x3000, vec![0x00], vec![0x42]));

        let diff = ExecutionDiffer::diff_states(&events, 0, 5);
        assert_eq!(diff.summary.total_memory_changes, 1);
        assert_eq!(diff.memory_changes[0].offset, 0x3000);
        assert_eq!(diff.summary.total_bytes_changed, 1);
    }

    #[test]
    fn test_diff_states_with_register_changes() {
        let mut events = make_events(3);
        events[1] = ExecutionEvent::new(1, EventType::Instruction, 0x1001)
            .with_register_change(RegisterChange::new("rax", 0, 42));

        let diff = ExecutionDiffer::diff_states(&events, 0, 3);
        assert_eq!(diff.summary.total_register_changes, 1);
        assert_eq!(diff.register_changes[0].name, "rax");
    }

    #[test]
    fn test_diff_states_unique_event_types() {
        let events = vec![
            ExecutionEvent::new(0, EventType::Instruction, 0x1000),
            ExecutionEvent::new(1, EventType::FunctionCall, 0x2000),
            ExecutionEvent::new(2, EventType::MemoryWrite, 0x2010),
            ExecutionEvent::new(3, EventType::FunctionCall, 0x3000),
        ];

        let diff = ExecutionDiffer::diff_states(&events, 0, 4);
        assert_eq!(diff.summary.unique_event_types.len(), 3);
    }

    #[test]
    fn test_compare_runs_identical() {
        let events_a = make_events(5);
        let events_b = make_events(5);

        let comparison = ExecutionDiffer::compare_runs(&events_a, &events_b);
        assert!(comparison.behavior_match);
        assert!(comparison.divergence_point.is_none());
        assert_eq!(comparison.summary.similarity_score, 1.0);
    }

    #[test]
    fn test_compare_runs_different_types() {
        let events_a = make_events(5);
        let mut events_b = make_events(5);
        events_b[3] = ExecutionEvent::new(3, EventType::FunctionCall, 0x1003);

        let comparison = ExecutionDiffer::compare_runs(&events_a, &events_b);
        assert!(!comparison.behavior_match);
        assert_eq!(comparison.divergence_point, Some(3));
        assert!(!comparison.differences.is_empty());
    }

    #[test]
    fn test_compare_runs_different_lengths() {
        let events_a = make_events(5);
        let events_b = make_events(3);

        let comparison = ExecutionDiffer::compare_runs(&events_a, &events_b);
        assert!(!comparison.behavior_match);
        assert_eq!(comparison.divergence_point, Some(3));
    }

    #[test]
    fn test_find_divergence_identical() {
        let events_a = make_events(5);
        let events_b = make_events(5);

        assert_eq!(ExecutionDiffer::find_divergence(&events_a, &events_b), None);
    }

    #[test]
    fn test_find_divergence_at_position() {
        let events_a = make_events(5);
        let mut events_b = make_events(5);
        events_b[2] = ExecutionEvent::new(2, EventType::Exception, 0x9999);

        assert_eq!(ExecutionDiffer::find_divergence(&events_a, &events_b), Some(2));
    }

    #[test]
    fn test_compare_runs_empty() {
        let empty: Vec<ExecutionEvent> = Vec::new();
        let comparison = ExecutionDiffer::compare_runs(&empty, &empty);
        assert!(comparison.behavior_match);
        assert_eq!(comparison.summary.similarity_score, 1.0);
    }

    #[test]
    fn test_diff_states_reversed_range() {
        let events = make_events(5);
        // from > to should still work (swaps internally)
        let diff = ExecutionDiffer::diff_states(&events, 4, 1);
        assert_eq!(diff.summary.events_in_range, 3);
    }
}
