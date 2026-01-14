//! Bidirectional replay engine for time-travel debugging.
//!
//! Provides forward/backward stepping through recorded execution events
//! with bookmarks and configurable replay behavior.

use super::{EventType, ExecutionEvent, RecordingId, StateSnapshot};
use serde::{Deserialize, Serialize};

/// Replay session for time-travel debugging.
pub struct ReplaySession {
    recording_id: RecordingId,
    events: Vec<ExecutionEvent>,
    snapshots: Vec<StateSnapshot>,
    current_position: usize,
    bookmarks: Vec<Bookmark>,
    config: ReplayConfig,
    state: ReplayState,
}

/// Configuration for replay behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    /// Replay speed.
    pub speed: ReplaySpeed,
    /// Event types to auto-pause on.
    pub auto_pause_on: Vec<EventType>,
    /// Event types to skip during replay.
    pub skip_event_types: Vec<EventType>,
    /// Maximum steps per single action (prevents runaway).
    pub max_steps_per_action: usize,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            speed: ReplaySpeed::Normal,
            auto_pause_on: Vec::new(),
            skip_event_types: Vec::new(),
            max_steps_per_action: 10_000,
        }
    }
}

/// Replay speed setting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ReplaySpeed {
    Slow,
    Normal,
    Fast,
    Instant,
}

/// Current state of the replay session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayState {
    Idle,
    Playing,
    Paused,
    SteppingForward,
    SteppingBackward,
    AtStart,
    AtEnd,
    Error { message: String },
}

/// A named bookmark at a position in the replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Unique bookmark ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Position (event index) in the timeline.
    pub position: usize,
    /// Optional description.
    pub description: Option<String>,
    /// When the bookmark was created.
    pub created_at: std::time::SystemTime,
}

/// Navigation commands for the replay engine.
#[derive(Debug, Clone)]
pub enum ReplayCommand {
    StepForward,
    StepBackward,
    StepOver,
    RunToPosition(usize),
    RunToEvent(EventType),
    RunToBookmark(String),
    Pause,
    Reset,
    SetSpeed(ReplaySpeed),
}

/// Result of a replay navigation action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    /// Current position after the action.
    pub position: usize,
    /// Total number of events.
    pub total_events: usize,
    /// The event at the current position.
    pub current_event: Option<ExecutionEvent>,
    /// Current replay state.
    pub state: ReplayState,
    /// Index of the nearest snapshot, if any.
    pub nearest_snapshot: Option<usize>,
}

impl ReplaySession {
    /// Create a new replay session.
    pub fn new(
        recording_id: RecordingId,
        events: Vec<ExecutionEvent>,
        snapshots: Vec<StateSnapshot>,
        config: ReplayConfig,
    ) -> Self {
        let state = if events.is_empty() {
            ReplayState::Error { message: "No events to replay".to_string() }
        } else {
            ReplayState::AtStart
        };

        Self {
            recording_id,
            events,
            snapshots,
            current_position: 0,
            bookmarks: Vec::new(),
            config,
            state,
        }
    }

    /// Execute a replay command.
    pub fn execute_command(&mut self, cmd: ReplayCommand) -> ReplayResult {
        match cmd {
            ReplayCommand::StepForward => self.step_forward(),
            ReplayCommand::StepBackward => self.step_backward(),
            ReplayCommand::StepOver => self.step_over(),
            ReplayCommand::RunToPosition(pos) => self.goto_position(pos),
            ReplayCommand::RunToEvent(event_type) => self.run_to_event(event_type),
            ReplayCommand::RunToBookmark(id) => {
                let pos = self.bookmarks.iter().find(|b| b.id == id).map(|b| b.position);
                match pos {
                    Some(p) => self.goto_position(p),
                    None => {
                        self.state =
                            ReplayState::Error { message: format!("Bookmark '{}' not found", id) };
                        self.build_result()
                    }
                }
            }
            ReplayCommand::Pause => {
                self.state = ReplayState::Paused;
                self.build_result()
            }
            ReplayCommand::Reset => {
                self.current_position = 0;
                self.state = ReplayState::AtStart;
                self.build_result()
            }
            ReplayCommand::SetSpeed(speed) => {
                self.config.speed = speed;
                self.build_result()
            }
        }
    }

    /// Step forward one event.
    pub fn step_forward(&mut self) -> ReplayResult {
        if self.events.is_empty() {
            return self.build_result();
        }

        self.state = ReplayState::SteppingForward;

        if self.current_position >= self.events.len() - 1 {
            self.state = ReplayState::AtEnd;
            return self.build_result();
        }

        self.current_position += 1;

        // Skip configured event types
        let mut steps = 0;
        while steps < self.config.max_steps_per_action
            && self.current_position < self.events.len() - 1
        {
            if let Some(event) = self.events.get(self.current_position) {
                if self.config.skip_event_types.contains(&event.event_type) {
                    self.current_position += 1;
                    steps += 1;
                    continue;
                }
            }
            break;
        }

        // Check auto-pause
        if let Some(event) = self.events.get(self.current_position) {
            if self.config.auto_pause_on.contains(&event.event_type) {
                self.state = ReplayState::Paused;
            } else {
                self.state = ReplayState::Paused;
            }
        }

        if self.current_position >= self.events.len() - 1 {
            self.state = ReplayState::AtEnd;
        }

        self.build_result()
    }

    /// Step backward one event.
    pub fn step_backward(&mut self) -> ReplayResult {
        if self.events.is_empty() {
            return self.build_result();
        }

        self.state = ReplayState::SteppingBackward;

        if self.current_position == 0 {
            self.state = ReplayState::AtStart;
            return self.build_result();
        }

        self.current_position -= 1;

        // Skip configured event types
        let mut steps = 0;
        while steps < self.config.max_steps_per_action && self.current_position > 0 {
            if let Some(event) = self.events.get(self.current_position) {
                if self.config.skip_event_types.contains(&event.event_type) {
                    self.current_position -= 1;
                    steps += 1;
                    continue;
                }
            }
            break;
        }

        self.state = ReplayState::Paused;

        if self.current_position == 0 {
            self.state = ReplayState::AtStart;
        }

        self.build_result()
    }

    /// Step over function calls (skip to next event at same or lower stack depth).
    fn step_over(&mut self) -> ReplayResult {
        if self.events.is_empty() || self.current_position >= self.events.len() - 1 {
            self.state = ReplayState::AtEnd;
            return self.build_result();
        }

        let current_depth =
            self.events.get(self.current_position).map(|e| e.stack_depth).unwrap_or(0);

        let mut steps = 0;
        self.current_position += 1;

        while self.current_position < self.events.len() - 1
            && steps < self.config.max_steps_per_action
        {
            if let Some(event) = self.events.get(self.current_position) {
                if event.stack_depth <= current_depth {
                    break;
                }
            }
            self.current_position += 1;
            steps += 1;
        }

        self.state = ReplayState::Paused;

        if self.current_position >= self.events.len() - 1 {
            self.state = ReplayState::AtEnd;
        }

        self.build_result()
    }

    /// Go to a specific position.
    pub fn goto_position(&mut self, pos: usize) -> ReplayResult {
        if self.events.is_empty() {
            return self.build_result();
        }

        if pos >= self.events.len() {
            self.state = ReplayState::Error {
                message: format!("Position {} out of range (max {})", pos, self.events.len() - 1),
            };
            return self.build_result();
        }

        self.current_position = pos;
        self.state = if pos == 0 {
            ReplayState::AtStart
        } else if pos >= self.events.len() - 1 {
            ReplayState::AtEnd
        } else {
            ReplayState::Paused
        };

        self.build_result()
    }

    /// Run forward until an event of the given type is found.
    pub fn run_to_event(&mut self, event_type: EventType) -> ReplayResult {
        if self.events.is_empty() {
            return self.build_result();
        }

        let mut steps = 0;
        let start = self.current_position + 1;

        for i in start..self.events.len() {
            if steps >= self.config.max_steps_per_action {
                self.state = ReplayState::Error { message: "Max steps exceeded".to_string() };
                return self.build_result();
            }

            if let Some(event) = self.events.get(i) {
                if event.event_type == event_type {
                    self.current_position = i;
                    self.state = ReplayState::Paused;
                    return self.build_result();
                }
            }
            steps += 1;
        }

        // Not found, go to end
        self.current_position = self.events.len() - 1;
        self.state = ReplayState::AtEnd;
        self.build_result()
    }

    /// Add a bookmark at the current position.
    pub fn add_bookmark(&mut self, name: String, description: Option<String>) -> Bookmark {
        let bookmark = Bookmark {
            id: format!("bk-{}", self.bookmarks.len() + 1),
            name,
            position: self.current_position,
            description,
            created_at: std::time::SystemTime::now(),
        };
        self.bookmarks.push(bookmark.clone());
        bookmark
    }

    /// Remove a bookmark by ID.
    pub fn remove_bookmark(&mut self, id: &str) {
        self.bookmarks.retain(|b| b.id != id);
    }

    /// List all bookmarks.
    pub fn list_bookmarks(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    /// Get the event at the current position.
    pub fn current_event(&self) -> Option<&ExecutionEvent> {
        self.events.get(self.current_position)
    }

    /// Get the current position.
    pub fn current_position(&self) -> usize {
        self.current_position
    }

    /// Get the total number of events.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Get the current replay state.
    pub fn state(&self) -> &ReplayState {
        &self.state
    }

    /// Find the nearest snapshot to a given position.
    pub fn find_nearest_snapshot(&self, position: usize) -> Option<&StateSnapshot> {
        if self.snapshots.is_empty() {
            return None;
        }

        let target_event_id = self.events.get(position)?.id;

        self.snapshots.iter().filter(|s| s.event_id <= target_event_id).max_by_key(|s| s.event_id)
    }

    /// Search events matching a predicate, returning (position, event) pairs.
    pub fn search_events<F>(&self, predicate: F) -> Vec<(usize, &ExecutionEvent)>
    where
        F: Fn(&ExecutionEvent) -> bool,
    {
        self.events.iter().enumerate().filter(|(_, event)| predicate(event)).collect()
    }

    fn build_result(&self) -> ReplayResult {
        let nearest_snapshot = self
            .find_nearest_snapshot(self.current_position)
            .and_then(|s| self.snapshots.iter().position(|snap| snap.event_id == s.event_id));

        ReplayResult {
            position: self.current_position,
            total_events: self.events.len(),
            current_event: self.events.get(self.current_position).cloned(),
            state: self.state.clone(),
            nearest_snapshot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_events(count: usize) -> Vec<ExecutionEvent> {
        (0..count)
            .map(|i| ExecutionEvent::new(i as u64, EventType::Instruction, 0x1000 + i as u64))
            .collect()
    }

    fn make_session(events: Vec<ExecutionEvent>) -> ReplaySession {
        ReplaySession::new(Uuid::new_v4(), events, Vec::new(), ReplayConfig::default())
    }

    #[test]
    fn test_new_session_empty_events() {
        let session = make_session(Vec::new());
        assert_eq!(
            session.state(),
            &ReplayState::Error { message: "No events to replay".to_string() }
        );
    }

    #[test]
    fn test_new_session_with_events() {
        let session = make_session(make_events(5));
        assert_eq!(session.state(), &ReplayState::AtStart);
        assert_eq!(session.current_position(), 0);
        assert_eq!(session.total_events(), 5);
    }

    #[test]
    fn test_step_forward() {
        let mut session = make_session(make_events(5));
        let result = session.step_forward();
        assert_eq!(result.position, 1);
        assert_eq!(result.state, ReplayState::Paused);
    }

    #[test]
    fn test_step_forward_at_end() {
        let mut session = make_session(make_events(3));
        session.goto_position(2);
        let result = session.step_forward();
        assert_eq!(result.state, ReplayState::AtEnd);
        assert_eq!(result.position, 2);
    }

    #[test]
    fn test_step_backward() {
        let mut session = make_session(make_events(5));
        session.goto_position(3);
        let result = session.step_backward();
        assert_eq!(result.position, 2);
        assert_eq!(result.state, ReplayState::Paused);
    }

    #[test]
    fn test_step_backward_at_start() {
        let mut session = make_session(make_events(5));
        let result = session.step_backward();
        assert_eq!(result.position, 0);
        assert_eq!(result.state, ReplayState::AtStart);
    }

    #[test]
    fn test_goto_position() {
        let mut session = make_session(make_events(10));
        let result = session.goto_position(5);
        assert_eq!(result.position, 5);
        assert_eq!(result.state, ReplayState::Paused);
    }

    #[test]
    fn test_goto_position_out_of_range() {
        let mut session = make_session(make_events(5));
        let result = session.goto_position(10);
        assert!(matches!(result.state, ReplayState::Error { .. }));
    }

    #[test]
    fn test_run_to_event() {
        let mut events = make_events(5);
        events[3] = ExecutionEvent::new(3, EventType::FunctionCall, 0x2000);
        let mut session = make_session(events);

        let result = session.run_to_event(EventType::FunctionCall);
        assert_eq!(result.position, 3);
        assert_eq!(result.state, ReplayState::Paused);
    }

    #[test]
    fn test_run_to_event_not_found() {
        let mut session = make_session(make_events(5));
        let result = session.run_to_event(EventType::Exception);
        assert_eq!(result.state, ReplayState::AtEnd);
    }

    #[test]
    fn test_bookmarks() {
        let mut session = make_session(make_events(10));
        session.goto_position(5);

        let bk = session.add_bookmark("test".to_string(), Some("A test bookmark".to_string()));
        assert_eq!(bk.position, 5);
        assert_eq!(session.list_bookmarks().len(), 1);

        // Navigate to bookmark
        session.goto_position(0);
        let result = session.execute_command(ReplayCommand::RunToBookmark(bk.id.clone()));
        assert_eq!(result.position, 5);

        // Remove bookmark
        session.remove_bookmark(&bk.id);
        assert_eq!(session.list_bookmarks().len(), 0);
    }

    #[test]
    fn test_execute_command_reset() {
        let mut session = make_session(make_events(10));
        session.goto_position(5);
        let result = session.execute_command(ReplayCommand::Reset);
        assert_eq!(result.position, 0);
        assert_eq!(result.state, ReplayState::AtStart);
    }

    #[test]
    fn test_execute_command_set_speed() {
        let mut session = make_session(make_events(5));
        session.execute_command(ReplayCommand::SetSpeed(ReplaySpeed::Fast));
        assert_eq!(session.config.speed, ReplaySpeed::Fast);
    }

    #[test]
    fn test_search_events() {
        let mut events = make_events(5);
        events[2] = ExecutionEvent::new(2, EventType::FunctionCall, 0x2000);
        events[4] = ExecutionEvent::new(4, EventType::FunctionCall, 0x3000);
        let session = make_session(events);

        let results = session.search_events(|e| e.event_type == EventType::FunctionCall);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 2);
        assert_eq!(results[1].0, 4);
    }

    #[test]
    fn test_current_event() {
        let events = make_events(5);
        let mut session = make_session(events);
        assert_eq!(session.current_event().unwrap().id, 0);

        session.step_forward();
        assert_eq!(session.current_event().unwrap().id, 1);
    }

    #[test]
    fn test_find_nearest_snapshot() {
        let events = make_events(10);
        let snapshots = vec![StateSnapshot::new(0), StateSnapshot::new(5), StateSnapshot::new(9)];

        let session =
            ReplaySession::new(Uuid::new_v4(), events, snapshots, ReplayConfig::default());

        // Position 3 -> nearest snapshot is at event_id 0
        let snap = session.find_nearest_snapshot(3).unwrap();
        assert_eq!(snap.event_id, 0);

        // Position 7 -> nearest snapshot is at event_id 5
        let snap = session.find_nearest_snapshot(7).unwrap();
        assert_eq!(snap.event_id, 5);
    }

    #[test]
    fn test_step_over() {
        let events = vec![
            ExecutionEvent::new(0, EventType::Instruction, 0x1000).with_stack_depth(0),
            ExecutionEvent::new(1, EventType::FunctionCall, 0x2000).with_stack_depth(0),
            ExecutionEvent::new(2, EventType::Instruction, 0x2010).with_stack_depth(1),
            ExecutionEvent::new(3, EventType::Instruction, 0x2020).with_stack_depth(1),
            ExecutionEvent::new(4, EventType::FunctionReturn, 0x2030).with_stack_depth(0),
            ExecutionEvent::new(5, EventType::Instruction, 0x1010).with_stack_depth(0),
        ];
        let mut session = make_session(events);
        // At position 0, step over should skip the deeper call frames
        let result = session.execute_command(ReplayCommand::StepOver);
        assert_eq!(result.position, 1);
    }

    #[test]
    fn test_skip_event_types() {
        let events = vec![
            ExecutionEvent::new(0, EventType::Instruction, 0x1000),
            ExecutionEvent::new(1, EventType::MemoryRead, 0x1010),
            ExecutionEvent::new(2, EventType::MemoryRead, 0x1020),
            ExecutionEvent::new(3, EventType::FunctionCall, 0x2000),
            ExecutionEvent::new(4, EventType::Instruction, 0x2010),
        ];
        let config =
            ReplayConfig { skip_event_types: vec![EventType::MemoryRead], ..Default::default() };
        let mut session = ReplaySession::new(Uuid::new_v4(), events, Vec::new(), config);

        let result = session.step_forward();
        // Should skip past the MemoryRead events to FunctionCall
        assert_eq!(result.position, 3);
    }

    #[test]
    fn test_replay_config_default() {
        let config = ReplayConfig::default();
        assert_eq!(config.speed, ReplaySpeed::Normal);
        assert!(config.auto_pause_on.is_empty());
        assert!(config.skip_event_types.is_empty());
        assert_eq!(config.max_steps_per_action, 10_000);
    }
}
