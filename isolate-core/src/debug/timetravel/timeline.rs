//! Timeline navigation for time-travel debugging.
//!
//! The timeline provides an interface for navigating through recorded execution,
//! stepping forward and backward through events.

use super::snapshot::{SnapshotManager, StateSnapshot};
use super::{EventId, EventType, ExecutionEvent};
use crate::error::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;

/// Result of a step operation.
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Successfully stepped to a new event.
    Stepped {
        /// The event we stepped to.
        event: ExecutionEvent,
        /// Current position in the timeline.
        position: TimelinePosition,
    },
    /// Reached the beginning of the timeline.
    AtStart {
        /// Current position.
        position: TimelinePosition,
    },
    /// Reached the end of the timeline.
    AtEnd {
        /// Current position.
        position: TimelinePosition,
    },
    /// Stepped to a breakpoint.
    Breakpoint {
        /// The breakpoint event.
        event: ExecutionEvent,
        /// Breakpoint ID.
        breakpoint_id: u64,
    },
    /// Stepped to an exception.
    Exception {
        /// The exception event.
        event: ExecutionEvent,
        /// Exception message.
        message: String,
    },
}

/// Current position in the timeline.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimelinePosition {
    /// Current event index.
    pub index: usize,
    /// Current event ID.
    pub event_id: EventId,
    /// Total number of events.
    pub total_events: usize,
    /// Progress as a percentage (0-100).
    pub progress: f64,
}

/// Navigation mode for stepping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationMode {
    /// Step one event at a time.
    Single,
    /// Step to next function call/return.
    Function,
    /// Step to next memory change.
    Memory,
    /// Step to next WASI call.
    Wasi,
    /// Step to next breakpoint.
    Breakpoint,
    /// Run until condition is met.
    RunTo,
}

/// A breakpoint in the timeline.
#[derive(Debug, Clone)]
pub struct TimelineBreakpoint {
    /// Unique breakpoint ID.
    pub id: u64,
    /// Event ID where breakpoint is set (if specific).
    pub event_id: Option<EventId>,
    /// Function name to break on.
    pub function_name: Option<String>,
    /// Instruction pointer to break on.
    pub instruction_pointer: Option<u64>,
    /// Condition expression (simplified).
    pub condition: Option<BreakCondition>,
    /// Whether the breakpoint is enabled.
    pub enabled: bool,
}

/// Breakpoint condition.
#[derive(Debug, Clone)]
pub enum BreakCondition {
    /// Break on memory access to address.
    MemoryAccess(u64),
    /// Break on memory write to address.
    MemoryWrite(u64),
    /// Break when fuel exceeds threshold.
    FuelExceeds(u64),
    /// Break on WASI function.
    WasiFunction(String),
    /// Break on event count.
    EventCount(u64),
}

/// Navigation interface for the timeline.
pub trait TimelineNavigation {
    /// Step forward one event.
    fn step_forward(&mut self) -> Result<StepResult>;

    /// Step backward one event.
    fn step_back(&mut self) -> Result<StepResult>;

    /// Step forward to next matching event type.
    fn step_forward_to(&mut self, event_type: EventType) -> Result<StepResult>;

    /// Step backward to previous matching event type.
    fn step_back_to(&mut self, event_type: EventType) -> Result<StepResult>;

    /// Go to a specific event by index.
    fn goto_index(&mut self, index: usize) -> Result<StepResult>;

    /// Go to a specific event by ID.
    fn goto_event(&mut self, event_id: EventId) -> Result<StepResult>;

    /// Get the current position.
    fn position(&self) -> TimelinePosition;

    /// Get the current event (if any).
    fn current_event(&self) -> Option<&ExecutionEvent>;
}

/// Timeline for navigating recorded execution.
pub struct Timeline {
    /// All recorded events.
    events: Vec<ExecutionEvent>,
    /// Current position index.
    current_index: usize,
    /// Event ID to index mapping.
    event_index: HashMap<EventId, usize>,
    /// Snapshot manager for efficient state restoration.
    snapshots: Option<Arc<SnapshotManager>>,
    /// Breakpoints.
    breakpoints: Vec<TimelineBreakpoint>,
    /// Next breakpoint ID.
    next_breakpoint_id: u64,
    /// Current navigation mode.
    navigation_mode: NavigationMode,
    /// Cached state at current position.
    cached_state: Option<StateSnapshot>,
}

impl Timeline {
    /// Create a new timeline from events.
    pub fn from_events(events: Vec<ExecutionEvent>) -> Result<Self> {
        if events.is_empty() {
            return Err(Error::InvalidState {
                expected: "Non-empty event list".to_string(),
                actual: "Empty event list".to_string(),
            });
        }

        let mut event_index = HashMap::with_capacity(events.len());
        for (idx, event) in events.iter().enumerate() {
            event_index.insert(event.id, idx);
        }

        Ok(Self {
            events,
            current_index: 0,
            event_index,
            snapshots: None,
            breakpoints: Vec::new(),
            next_breakpoint_id: 1,
            navigation_mode: NavigationMode::Single,
            cached_state: None,
        })
    }

    /// Create with snapshot support.
    pub fn with_snapshots(mut self, snapshots: Arc<SnapshotManager>) -> Self {
        self.snapshots = Some(snapshots);
        self
    }

    /// Get all events.
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    /// Get total event count.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Set the navigation mode.
    pub fn set_navigation_mode(&mut self, mode: NavigationMode) {
        self.navigation_mode = mode;
    }

    /// Get the navigation mode.
    pub fn navigation_mode(&self) -> NavigationMode {
        self.navigation_mode
    }

    /// Add a breakpoint.
    pub fn add_breakpoint(&mut self, breakpoint: TimelineBreakpoint) -> u64 {
        let id = self.next_breakpoint_id;
        self.next_breakpoint_id += 1;
        let mut bp = breakpoint;
        bp.id = id;
        self.breakpoints.push(bp);
        id
    }

    /// Add a breakpoint at a specific event ID.
    pub fn add_breakpoint_at_event(&mut self, event_id: EventId) -> u64 {
        self.add_breakpoint(TimelineBreakpoint {
            id: 0,
            event_id: Some(event_id),
            function_name: None,
            instruction_pointer: None,
            condition: None,
            enabled: true,
        })
    }

    /// Add a breakpoint at a function.
    pub fn add_breakpoint_at_function(&mut self, function_name: impl Into<String>) -> u64 {
        self.add_breakpoint(TimelineBreakpoint {
            id: 0,
            event_id: None,
            function_name: Some(function_name.into()),
            instruction_pointer: None,
            condition: None,
            enabled: true,
        })
    }

    /// Remove a breakpoint.
    pub fn remove_breakpoint(&mut self, id: u64) -> bool {
        let len = self.breakpoints.len();
        self.breakpoints.retain(|bp| bp.id != id);
        self.breakpoints.len() < len
    }

    /// Enable/disable a breakpoint.
    pub fn set_breakpoint_enabled(&mut self, id: u64, enabled: bool) -> bool {
        for bp in &mut self.breakpoints {
            if bp.id == id {
                bp.enabled = enabled;
                return true;
            }
        }
        false
    }

    /// Get all breakpoints.
    pub fn breakpoints(&self) -> &[TimelineBreakpoint] {
        &self.breakpoints
    }

    /// Get events filtered by type.
    pub fn events_of_type(&self, event_type: EventType) -> Vec<&ExecutionEvent> {
        self.events
            .iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }

    /// Get function call events.
    pub fn function_calls(&self) -> Vec<&ExecutionEvent> {
        self.events_of_type(EventType::FunctionCall)
    }

    /// Get memory write events.
    pub fn memory_writes(&self) -> Vec<&ExecutionEvent> {
        self.events_of_type(EventType::MemoryWrite)
    }

    /// Get WASI call events.
    pub fn wasi_calls(&self) -> Vec<&ExecutionEvent> {
        self.events_of_type(EventType::WasiCall)
    }

    /// Search events by function name.
    pub fn search_by_function(&self, name: &str) -> Vec<&ExecutionEvent> {
        self.events
            .iter()
            .filter(|e| {
                e.function_name
                    .as_ref()
                    .map(|n| n.contains(name))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Search events by instruction pointer range.
    pub fn search_by_ip_range(&self, start: u64, end: u64) -> Vec<&ExecutionEvent> {
        self.events
            .iter()
            .filter(|e| e.instruction_pointer >= start && e.instruction_pointer <= end)
            .collect()
    }

    /// Get events in a time range.
    pub fn events_in_range(&self, start_id: EventId, end_id: EventId) -> Vec<&ExecutionEvent> {
        self.events
            .iter()
            .filter(|e| e.id >= start_id && e.id <= end_id)
            .collect()
    }

    /// Get state at current position (from nearest snapshot + replay).
    pub fn current_state(&self) -> Option<StateSnapshot> {
        self.cached_state.clone().or_else(|| {
            self.snapshots.as_ref().and_then(|sm| {
                let current = self.events.get(self.current_index)?;
                sm.get_nearest(current.id)
            })
        })
    }

    /// Run to the next breakpoint.
    pub fn run_to_breakpoint(&mut self) -> Result<StepResult> {
        while self.current_index < self.events.len() - 1 {
            self.current_index += 1;
            let event = &self.events[self.current_index];

            if let Some(bp) = self.check_breakpoint(event) {
                return Ok(StepResult::Breakpoint {
                    event: event.clone(),
                    breakpoint_id: bp,
                });
            }
        }

        Ok(StepResult::AtEnd {
            position: self.position(),
        })
    }

    /// Run backwards to the previous breakpoint.
    pub fn run_back_to_breakpoint(&mut self) -> Result<StepResult> {
        while self.current_index > 0 {
            self.current_index -= 1;
            let event = &self.events[self.current_index];

            if let Some(bp) = self.check_breakpoint(event) {
                return Ok(StepResult::Breakpoint {
                    event: event.clone(),
                    breakpoint_id: bp,
                });
            }
        }

        Ok(StepResult::AtStart {
            position: self.position(),
        })
    }

    /// Check if an event matches any breakpoint.
    fn check_breakpoint(&self, event: &ExecutionEvent) -> Option<u64> {
        for bp in &self.breakpoints {
            if !bp.enabled {
                continue;
            }

            // Check event ID
            if let Some(event_id) = bp.event_id {
                if event.id == event_id {
                    return Some(bp.id);
                }
            }

            // Check function name
            if let Some(ref func_name) = bp.function_name {
                if event.function_name.as_ref() == Some(func_name) {
                    return Some(bp.id);
                }
            }

            // Check instruction pointer
            if let Some(ip) = bp.instruction_pointer {
                if event.instruction_pointer == ip {
                    return Some(bp.id);
                }
            }

            // Check condition
            if let Some(ref condition) = bp.condition {
                if self.check_condition(event, condition) {
                    return Some(bp.id);
                }
            }
        }

        None
    }

    /// Check if a condition is met.
    fn check_condition(&self, event: &ExecutionEvent, condition: &BreakCondition) -> bool {
        match condition {
            BreakCondition::MemoryAccess(addr) => {
                event.memory_changes.iter().any(|c| c.address == *addr)
            }
            BreakCondition::MemoryWrite(addr) => {
                event.event_type == EventType::MemoryWrite
                    && event.memory_changes.iter().any(|c| c.address == *addr)
            }
            BreakCondition::FuelExceeds(threshold) => event.fuel_consumed > *threshold,
            BreakCondition::WasiFunction(name) => event
                .wasi_call
                .as_ref()
                .map(|c| &c.function == name)
                .unwrap_or(false),
            BreakCondition::EventCount(count) => event.id >= *count,
        }
    }

    /// Step forward based on navigation mode.
    fn step_forward_by_mode(&mut self) -> Result<StepResult> {
        match self.navigation_mode {
            NavigationMode::Single => self.step_forward(),
            NavigationMode::Function => self.step_forward_to(EventType::FunctionCall),
            NavigationMode::Memory => self.step_forward_to(EventType::MemoryWrite),
            NavigationMode::Wasi => self.step_forward_to(EventType::WasiCall),
            NavigationMode::Breakpoint => self.run_to_breakpoint(),
            NavigationMode::RunTo => self.run_to_breakpoint(),
        }
    }

    /// Step backward based on navigation mode.
    fn step_back_by_mode(&mut self) -> Result<StepResult> {
        match self.navigation_mode {
            NavigationMode::Single => self.step_back(),
            NavigationMode::Function => self.step_back_to(EventType::FunctionCall),
            NavigationMode::Memory => self.step_back_to(EventType::MemoryWrite),
            NavigationMode::Wasi => self.step_back_to(EventType::WasiCall),
            NavigationMode::Breakpoint => self.run_back_to_breakpoint(),
            NavigationMode::RunTo => self.run_back_to_breakpoint(),
        }
    }

    /// Get timeline statistics.
    pub fn stats(&self) -> TimelineStats {
        let mut stats = TimelineStats::default();
        stats.total_events = self.events.len();

        for event in &self.events {
            match event.event_type {
                EventType::Instruction => stats.instructions += 1,
                EventType::FunctionCall => stats.function_calls += 1,
                EventType::FunctionReturn => stats.function_returns += 1,
                EventType::MemoryRead => stats.memory_reads += 1,
                EventType::MemoryWrite => stats.memory_writes += 1,
                EventType::WasiCall => stats.wasi_calls += 1,
                EventType::WasiReturn => stats.wasi_returns += 1,
                EventType::Breakpoint => stats.breakpoints += 1,
                EventType::Exception => stats.exceptions += 1,
                EventType::Start => {}
                EventType::Pause => {}
                EventType::Resume => {}
                EventType::End => {}
            }
        }

        stats
    }
}

impl TimelineNavigation for Timeline {
    fn step_forward(&mut self) -> Result<StepResult> {
        if self.current_index >= self.events.len() - 1 {
            return Ok(StepResult::AtEnd {
                position: self.position(),
            });
        }

        self.current_index += 1;
        self.cached_state = None;
        let event = &self.events[self.current_index];

        // Check for breakpoint
        if let Some(bp_id) = self.check_breakpoint(event) {
            return Ok(StepResult::Breakpoint {
                event: event.clone(),
                breakpoint_id: bp_id,
            });
        }

        // Check for exception
        if event.event_type == EventType::Exception {
            let message = event
                .data
                .as_ref()
                .map(|d| String::from_utf8_lossy(d).to_string())
                .unwrap_or_default();
            return Ok(StepResult::Exception {
                event: event.clone(),
                message,
            });
        }

        Ok(StepResult::Stepped {
            event: event.clone(),
            position: self.position(),
        })
    }

    fn step_back(&mut self) -> Result<StepResult> {
        if self.current_index == 0 {
            return Ok(StepResult::AtStart {
                position: self.position(),
            });
        }

        self.current_index -= 1;
        self.cached_state = None;
        let event = &self.events[self.current_index];

        // Check for breakpoint
        if let Some(bp_id) = self.check_breakpoint(event) {
            return Ok(StepResult::Breakpoint {
                event: event.clone(),
                breakpoint_id: bp_id,
            });
        }

        Ok(StepResult::Stepped {
            event: event.clone(),
            position: self.position(),
        })
    }

    fn step_forward_to(&mut self, event_type: EventType) -> Result<StepResult> {
        while self.current_index < self.events.len() - 1 {
            self.current_index += 1;
            let event = &self.events[self.current_index];

            if event.event_type == event_type {
                self.cached_state = None;
                return Ok(StepResult::Stepped {
                    event: event.clone(),
                    position: self.position(),
                });
            }

            // Check for breakpoint
            if let Some(bp_id) = self.check_breakpoint(event) {
                self.cached_state = None;
                return Ok(StepResult::Breakpoint {
                    event: event.clone(),
                    breakpoint_id: bp_id,
                });
            }
        }

        Ok(StepResult::AtEnd {
            position: self.position(),
        })
    }

    fn step_back_to(&mut self, event_type: EventType) -> Result<StepResult> {
        while self.current_index > 0 {
            self.current_index -= 1;
            let event = &self.events[self.current_index];

            if event.event_type == event_type {
                self.cached_state = None;
                return Ok(StepResult::Stepped {
                    event: event.clone(),
                    position: self.position(),
                });
            }

            // Check for breakpoint
            if let Some(bp_id) = self.check_breakpoint(event) {
                self.cached_state = None;
                return Ok(StepResult::Breakpoint {
                    event: event.clone(),
                    breakpoint_id: bp_id,
                });
            }
        }

        Ok(StepResult::AtStart {
            position: self.position(),
        })
    }

    fn goto_index(&mut self, index: usize) -> Result<StepResult> {
        if index >= self.events.len() {
            return Err(Error::InvalidState {
                expected: format!("Index < {}", self.events.len()),
                actual: format!("Index {}", index),
            });
        }

        self.current_index = index;
        self.cached_state = None;
        let event = &self.events[self.current_index];

        Ok(StepResult::Stepped {
            event: event.clone(),
            position: self.position(),
        })
    }

    fn goto_event(&mut self, event_id: EventId) -> Result<StepResult> {
        let index = self
            .event_index
            .get(&event_id)
            .ok_or_else(|| Error::InvalidState {
                expected: "Valid event ID".to_string(),
                actual: format!("Event ID {} not found", event_id),
            })?;

        self.goto_index(*index)
    }

    fn position(&self) -> TimelinePosition {
        let event_id = self
            .events
            .get(self.current_index)
            .map(|e| e.id)
            .unwrap_or(0);

        let progress = if self.events.is_empty() {
            0.0
        } else {
            (self.current_index as f64 / (self.events.len() - 1) as f64) * 100.0
        };

        TimelinePosition {
            index: self.current_index,
            event_id,
            total_events: self.events.len(),
            progress,
        }
    }

    fn current_event(&self) -> Option<&ExecutionEvent> {
        self.events.get(self.current_index)
    }
}

/// Timeline statistics.
#[derive(Debug, Clone, Default)]
pub struct TimelineStats {
    /// Total events.
    pub total_events: usize,
    /// Instruction events.
    pub instructions: usize,
    /// Function call events.
    pub function_calls: usize,
    /// Function return events.
    pub function_returns: usize,
    /// Memory read events.
    pub memory_reads: usize,
    /// Memory write events.
    pub memory_writes: usize,
    /// WASI call events.
    pub wasi_calls: usize,
    /// WASI return events.
    pub wasi_returns: usize,
    /// Breakpoint events.
    pub breakpoints: usize,
    /// Exception events.
    pub exceptions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_events(count: usize) -> Vec<ExecutionEvent> {
        (0..count)
            .map(|i| ExecutionEvent::new(i as u64, EventType::Instruction, 0x1000 + i as u64))
            .collect()
    }

    #[test]
    fn test_timeline_creation() {
        let events = create_test_events(10);
        let timeline = Timeline::from_events(events).unwrap();

        assert_eq!(timeline.event_count(), 10);
        assert_eq!(timeline.position().index, 0);
    }

    #[test]
    fn test_timeline_empty_events() {
        let result = Timeline::from_events(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_step_forward() {
        let events = create_test_events(5);
        let mut timeline = Timeline::from_events(events).unwrap();

        let result = timeline.step_forward().unwrap();
        match result {
            StepResult::Stepped { position, .. } => {
                assert_eq!(position.index, 1);
            }
            _ => panic!("Expected Stepped result"),
        }
    }

    #[test]
    fn test_step_back() {
        let events = create_test_events(5);
        let mut timeline = Timeline::from_events(events).unwrap();

        // Move forward first
        timeline.goto_index(3).unwrap();

        let result = timeline.step_back().unwrap();
        match result {
            StepResult::Stepped { position, .. } => {
                assert_eq!(position.index, 2);
            }
            _ => panic!("Expected Stepped result"),
        }
    }

    #[test]
    fn test_step_forward_at_end() {
        let events = create_test_events(3);
        let mut timeline = Timeline::from_events(events).unwrap();

        timeline.goto_index(2).unwrap();
        let result = timeline.step_forward().unwrap();

        match result {
            StepResult::AtEnd { .. } => {}
            _ => panic!("Expected AtEnd result"),
        }
    }

    #[test]
    fn test_step_back_at_start() {
        let events = create_test_events(3);
        let mut timeline = Timeline::from_events(events).unwrap();

        let result = timeline.step_back().unwrap();

        match result {
            StepResult::AtStart { .. } => {}
            _ => panic!("Expected AtStart result"),
        }
    }

    #[test]
    fn test_goto_index() {
        let events = create_test_events(10);
        let mut timeline = Timeline::from_events(events).unwrap();

        let result = timeline.goto_index(5).unwrap();
        match result {
            StepResult::Stepped { position, .. } => {
                assert_eq!(position.index, 5);
            }
            _ => panic!("Expected Stepped result"),
        }
    }

    #[test]
    fn test_goto_event() {
        let events = create_test_events(10);
        let mut timeline = Timeline::from_events(events).unwrap();

        let result = timeline.goto_event(7).unwrap();
        match result {
            StepResult::Stepped { event, .. } => {
                assert_eq!(event.id, 7);
            }
            _ => panic!("Expected Stepped result"),
        }
    }

    #[test]
    fn test_breakpoint() {
        let events = create_test_events(10);
        let mut timeline = Timeline::from_events(events).unwrap();

        let bp_id = timeline.add_breakpoint_at_event(5);

        // Step forward until we hit the breakpoint
        loop {
            match timeline.step_forward().unwrap() {
                StepResult::Breakpoint {
                    breakpoint_id,
                    event,
                } => {
                    assert_eq!(breakpoint_id, bp_id);
                    assert_eq!(event.id, 5);
                    break;
                }
                StepResult::AtEnd { .. } => panic!("Should have hit breakpoint"),
                _ => continue,
            }
        }
    }

    #[test]
    fn test_step_forward_to_type() {
        let mut events = create_test_events(5);
        events[3] = ExecutionEvent::new(3, EventType::FunctionCall, 0x2000);
        let mut timeline = Timeline::from_events(events).unwrap();

        let result = timeline.step_forward_to(EventType::FunctionCall).unwrap();
        match result {
            StepResult::Stepped { event, position } => {
                assert_eq!(event.event_type, EventType::FunctionCall);
                assert_eq!(position.index, 3);
            }
            _ => panic!("Expected Stepped result"),
        }
    }

    #[test]
    fn test_position_progress() {
        let events = create_test_events(10);
        let mut timeline = Timeline::from_events(events).unwrap();

        let pos = timeline.position();
        assert_eq!(pos.progress, 0.0);

        timeline.goto_index(4).unwrap();
        let pos = timeline.position();
        assert!((pos.progress - 44.44).abs() < 1.0);

        timeline.goto_index(9).unwrap();
        let pos = timeline.position();
        assert_eq!(pos.progress, 100.0);
    }

    #[test]
    fn test_timeline_stats() {
        let mut events = vec![
            ExecutionEvent::new(0, EventType::Start, 0),
            ExecutionEvent::new(1, EventType::Instruction, 0x1000),
            ExecutionEvent::new(2, EventType::FunctionCall, 0x1000),
            ExecutionEvent::new(3, EventType::MemoryWrite, 0x1000),
            ExecutionEvent::new(4, EventType::WasiCall, 0x1000),
            ExecutionEvent::new(5, EventType::FunctionReturn, 0x1000),
            ExecutionEvent::new(6, EventType::End, 0),
        ];

        let timeline = Timeline::from_events(events).unwrap();
        let stats = timeline.stats();

        assert_eq!(stats.total_events, 7);
        assert_eq!(stats.instructions, 1);
        assert_eq!(stats.function_calls, 1);
        assert_eq!(stats.function_returns, 1);
        assert_eq!(stats.memory_writes, 1);
        assert_eq!(stats.wasi_calls, 1);
    }

    #[test]
    fn test_search_by_function() {
        let mut events = create_test_events(5);
        events[2] =
            ExecutionEvent::new(2, EventType::FunctionCall, 0x2000).with_function("my_function");
        events[4] = ExecutionEvent::new(4, EventType::FunctionCall, 0x3000)
            .with_function("my_other_function");

        let timeline = Timeline::from_events(events).unwrap();
        let results = timeline.search_by_function("my_");

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_events_in_range() {
        let events = create_test_events(10);
        let timeline = Timeline::from_events(events).unwrap();

        let range = timeline.events_in_range(3, 6);
        assert_eq!(range.len(), 4); // Events 3, 4, 5, 6
    }
}
