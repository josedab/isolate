//! Debug session management.

use super::breakpoint::{Breakpoint, BreakpointId};
use super::inspector::{GlobalsSnapshot, Inspector, MemoryView, StackFrame};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Debug session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugState {
    /// Session is not attached.
    Detached,
    /// Sandbox is running (not stopped).
    Running,
    /// Stopped at a breakpoint.
    Stopped,
    /// Stepping through code.
    Stepping,
    /// Session has ended.
    Terminated,
}

impl std::fmt::Display for DebugState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DebugState::Detached => write!(f, "detached"),
            DebugState::Running => write!(f, "running"),
            DebugState::Stopped => write!(f, "stopped"),
            DebugState::Stepping => write!(f, "stepping"),
            DebugState::Terminated => write!(f, "terminated"),
        }
    }
}

/// Debug command to send to a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugCommand {
    /// Continue execution.
    Continue,
    /// Step into the next instruction.
    StepInto,
    /// Step over the current instruction.
    StepOver,
    /// Step out of the current function.
    StepOut,
    /// Pause execution.
    Pause,
    /// Terminate the sandbox.
    Terminate,
    /// Set a breakpoint.
    SetBreakpoint(Breakpoint),
    /// Remove a breakpoint.
    RemoveBreakpoint(BreakpointId),
    /// Enable a breakpoint.
    EnableBreakpoint(BreakpointId),
    /// Disable a breakpoint.
    DisableBreakpoint(BreakpointId),
    /// Read memory at address.
    ReadMemory { address: u64, size: usize },
    /// Evaluate an expression.
    Evaluate(String),
    /// Add a watch expression.
    AddWatch(String),
    /// Remove a watch expression.
    RemoveWatch(String),
}

/// Debug event emitted by a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugEvent {
    /// Session attached to sandbox.
    Attached { sandbox_id: Uuid },
    /// Session detached from sandbox.
    Detached { sandbox_id: Uuid },
    /// Execution stopped at breakpoint.
    BreakpointHit {
        breakpoint_id: BreakpointId,
        stack: Vec<StackFrame>,
    },
    /// Execution stopped (step complete).
    StepComplete { stack: Vec<StackFrame> },
    /// Execution stopped (pause).
    Paused { stack: Vec<StackFrame> },
    /// Execution resumed.
    Resumed,
    /// Sandbox terminated.
    Terminated { exit_code: i32 },
    /// Expression evaluated.
    EvaluationResult { expression: String, value: String },
    /// Memory read complete.
    MemoryRead { view: MemoryView },
    /// Watch updated.
    WatchUpdated { expression: String, value: String },
    /// Error occurred.
    Error { message: String },
    /// Output captured.
    Output { stream: OutputStream, data: Vec<u8> },
}

/// Output stream type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Debug session configuration.
#[derive(Debug, Clone)]
pub struct DebugConfig {
    /// Stop on entry.
    pub stop_on_entry: bool,
    /// Stop on unhandled exception/trap.
    pub stop_on_exception: bool,
    /// Maximum stack frames to capture.
    pub max_stack_frames: usize,
    /// Collect output during debugging.
    pub capture_output: bool,
    /// Enable source maps if available.
    pub enable_source_maps: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            stop_on_entry: false,
            stop_on_exception: true,
            max_stack_frames: super::MAX_STACK_DEPTH,
            capture_output: true,
            enable_source_maps: true,
        }
    }
}

impl DebugConfig {
    /// Create a new config with stop on entry.
    pub fn with_stop_on_entry(mut self, stop: bool) -> Self {
        self.stop_on_entry = stop;
        self
    }

    /// Create a new config with stop on exception.
    pub fn with_stop_on_exception(mut self, stop: bool) -> Self {
        self.stop_on_exception = stop;
        self
    }
}

/// Internal session state.
struct SessionState {
    state: DebugState,
    breakpoints: HashMap<BreakpointId, Breakpoint>,
    inspector: Inspector,
    pending_commands: Vec<DebugCommand>,
    events: Vec<DebugEvent>,
    last_breakpoint: Option<BreakpointId>,
    step_mode: Option<StepMode>,
}

/// Step mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepMode {
    Into,
    Over { target_depth: usize },
    Out { target_depth: usize },
}

/// A debug session for a sandbox.
pub struct DebugSession {
    /// Session ID.
    id: Uuid,
    /// Associated sandbox ID.
    sandbox_id: Uuid,
    /// Session configuration.
    config: DebugConfig,
    /// Session state.
    state: Arc<RwLock<SessionState>>,
    /// Creation timestamp.
    created_at: DateTime<Utc>,
}

impl DebugSession {
    /// Create a new debug session for a sandbox.
    pub fn new(sandbox_id: Uuid) -> Self {
        Self::with_config(sandbox_id, DebugConfig::default())
    }

    /// Create a new debug session with custom config.
    pub fn with_config(sandbox_id: Uuid, config: DebugConfig) -> Self {
        let state = SessionState {
            state: DebugState::Detached,
            breakpoints: HashMap::new(),
            inspector: Inspector::new(sandbox_id),
            pending_commands: Vec::new(),
            events: Vec::new(),
            last_breakpoint: None,
            step_mode: None,
        };

        Self {
            id: Uuid::new_v4(),
            sandbox_id,
            config,
            state: Arc::new(RwLock::new(state)),
            created_at: Utc::now(),
        }
    }

    /// Get the session ID.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the sandbox ID.
    pub fn sandbox_id(&self) -> Uuid {
        self.sandbox_id
    }

    /// Get the configuration.
    pub fn config(&self) -> &DebugConfig {
        &self.config
    }

    /// Get the creation timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get the current state.
    pub fn state(&self) -> DebugState {
        self.state.read().state
    }

    /// Attach to the sandbox.
    pub fn attach(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        if state.state != DebugState::Detached {
            return Err(DebugError::AlreadyAttached);
        }

        state.state = DebugState::Running;
        state.events.push(DebugEvent::Attached {
            sandbox_id: self.sandbox_id,
        });

        Ok(())
    }

    /// Detach from the sandbox.
    pub fn detach(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        if state.state == DebugState::Detached {
            return Err(DebugError::NotAttached);
        }

        state.state = DebugState::Detached;
        state.events.push(DebugEvent::Detached {
            sandbox_id: self.sandbox_id,
        });

        Ok(())
    }

    /// Set a breakpoint.
    pub fn set_breakpoint(&self, breakpoint: Breakpoint) -> Result<BreakpointId, DebugError> {
        let mut state = self.state.write();

        if state.breakpoints.len() >= super::MAX_BREAKPOINTS {
            return Err(DebugError::TooManyBreakpoints);
        }

        let id = breakpoint.id;
        state.breakpoints.insert(id, breakpoint);
        Ok(id)
    }

    /// Remove a breakpoint.
    pub fn remove_breakpoint(&self, id: BreakpointId) -> Result<(), DebugError> {
        let mut state = self.state.write();
        state
            .breakpoints
            .remove(&id)
            .ok_or(DebugError::BreakpointNotFound(id))?;
        Ok(())
    }

    /// Enable a breakpoint.
    pub fn enable_breakpoint(&self, id: BreakpointId) -> Result<(), DebugError> {
        let mut state = self.state.write();
        let bp = state
            .breakpoints
            .get_mut(&id)
            .ok_or(DebugError::BreakpointNotFound(id))?;
        bp.enable();
        Ok(())
    }

    /// Disable a breakpoint.
    pub fn disable_breakpoint(&self, id: BreakpointId) -> Result<(), DebugError> {
        let mut state = self.state.write();
        let bp = state
            .breakpoints
            .get_mut(&id)
            .ok_or(DebugError::BreakpointNotFound(id))?;
        bp.disable();
        Ok(())
    }

    /// Get all breakpoints.
    pub fn breakpoints(&self) -> Vec<Breakpoint> {
        self.state.read().breakpoints.values().cloned().collect()
    }

    /// Get a breakpoint by ID.
    pub fn get_breakpoint(&self, id: BreakpointId) -> Option<Breakpoint> {
        self.state.read().breakpoints.get(&id).cloned()
    }

    /// Continue execution.
    pub fn continue_execution(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        self.require_stopped(&state)?;

        state.state = DebugState::Running;
        state.step_mode = None;
        state.pending_commands.push(DebugCommand::Continue);
        state.events.push(DebugEvent::Resumed);

        Ok(())
    }

    /// Step into the next instruction.
    pub fn step_into(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        self.require_stopped(&state)?;

        state.state = DebugState::Stepping;
        state.step_mode = Some(StepMode::Into);
        state.pending_commands.push(DebugCommand::StepInto);

        Ok(())
    }

    /// Step over the current instruction.
    pub fn step_over(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        self.require_stopped(&state)?;

        let current_depth = state.inspector.stack_depth();
        state.state = DebugState::Stepping;
        state.step_mode = Some(StepMode::Over {
            target_depth: current_depth,
        });
        state.pending_commands.push(DebugCommand::StepOver);

        Ok(())
    }

    /// Step out of the current function.
    pub fn step_out(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        self.require_stopped(&state)?;

        let current_depth = state.inspector.stack_depth();
        if current_depth == 0 {
            return Err(DebugError::CannotStepOut);
        }

        state.state = DebugState::Stepping;
        state.step_mode = Some(StepMode::Out {
            target_depth: current_depth - 1,
        });
        state.pending_commands.push(DebugCommand::StepOut);

        Ok(())
    }

    /// Pause execution.
    pub fn pause(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        if state.state != DebugState::Running {
            return Err(DebugError::NotRunning);
        }

        state.pending_commands.push(DebugCommand::Pause);
        Ok(())
    }

    /// Terminate the sandbox.
    pub fn terminate(&self) -> Result<(), DebugError> {
        let mut state = self.state.write();
        if state.state == DebugState::Terminated || state.state == DebugState::Detached {
            return Err(DebugError::NotAttached);
        }

        state.pending_commands.push(DebugCommand::Terminate);
        Ok(())
    }

    /// Evaluate an expression.
    pub fn evaluate(&self, expr: impl Into<String>) -> Result<(), DebugError> {
        let mut state = self.state.write();
        self.require_stopped(&state)?;

        state
            .pending_commands
            .push(DebugCommand::Evaluate(expr.into()));
        Ok(())
    }

    /// Add a watch expression.
    pub fn add_watch(&self, expr: impl Into<String>) {
        let mut state = self.state.write();
        let expr = expr.into();
        state.inspector.add_watch(&expr);
        state.pending_commands.push(DebugCommand::AddWatch(expr));
    }

    /// Remove a watch expression.
    pub fn remove_watch(&self, expr: &str) {
        let mut state = self.state.write();
        state.inspector.remove_watch(expr);
        state
            .pending_commands
            .push(DebugCommand::RemoveWatch(expr.to_string()));
    }

    /// Request memory read.
    pub fn read_memory(&self, address: u64, size: usize) -> Result<(), DebugError> {
        let mut state = self.state.write();
        self.require_stopped(&state)?;

        let size = size.min(super::MAX_MEMORY_VIEW);
        state
            .pending_commands
            .push(DebugCommand::ReadMemory { address, size });
        Ok(())
    }

    /// Get pending commands and clear the queue.
    pub fn take_pending_commands(&self) -> Vec<DebugCommand> {
        let mut state = self.state.write();
        std::mem::take(&mut state.pending_commands)
    }

    /// Get pending events and clear the queue.
    pub fn take_events(&self) -> Vec<DebugEvent> {
        let mut state = self.state.write();
        std::mem::take(&mut state.events)
    }

    /// Push an event.
    pub fn push_event(&self, event: DebugEvent) {
        self.state.write().events.push(event);
    }

    /// Notify that a breakpoint was hit.
    pub fn notify_breakpoint_hit(&self, breakpoint_id: BreakpointId, stack: Vec<StackFrame>) {
        let mut state = self.state.write();
        state.state = DebugState::Stopped;
        state.last_breakpoint = Some(breakpoint_id);
        state.inspector.set_call_stack(stack.clone());

        // Record hit
        if let Some(bp) = state.breakpoints.get_mut(&breakpoint_id) {
            bp.record_hit();
        }

        state.events.push(DebugEvent::BreakpointHit {
            breakpoint_id,
            stack,
        });
    }

    /// Notify that a step completed.
    pub fn notify_step_complete(&self, stack: Vec<StackFrame>) {
        let mut state = self.state.write();
        state.state = DebugState::Stopped;
        state.step_mode = None;
        state.inspector.set_call_stack(stack.clone());
        state.events.push(DebugEvent::StepComplete { stack });
    }

    /// Notify that execution was paused.
    pub fn notify_paused(&self, stack: Vec<StackFrame>) {
        let mut state = self.state.write();
        state.state = DebugState::Stopped;
        state.inspector.set_call_stack(stack.clone());
        state.events.push(DebugEvent::Paused { stack });
    }

    /// Notify that sandbox terminated.
    pub fn notify_terminated(&self, exit_code: i32) {
        let mut state = self.state.write();
        state.state = DebugState::Terminated;
        state.events.push(DebugEvent::Terminated { exit_code });
    }

    /// Notify of output.
    pub fn notify_output(&self, stream: OutputStream, data: Vec<u8>) {
        if self.config.capture_output {
            self.state
                .write()
                .events
                .push(DebugEvent::Output { stream, data });
        }
    }

    /// Update the call stack.
    pub fn update_call_stack(&self, stack: Vec<StackFrame>) {
        self.state.write().inspector.set_call_stack(stack);
    }

    /// Update globals.
    pub fn update_globals(&self, globals: GlobalsSnapshot) {
        self.state.write().inspector.set_globals(globals);
    }

    /// Cache memory view.
    pub fn cache_memory(&self, view: MemoryView) {
        let mut state = self.state.write();
        state.inspector.cache_memory(view.clone());
        state.events.push(DebugEvent::MemoryRead { view });
    }

    /// Get current stack frames.
    pub fn call_stack(&self) -> Vec<StackFrame> {
        self.state.read().inspector.call_stack().to_vec()
    }

    /// Get the current frame.
    pub fn current_frame(&self) -> Option<StackFrame> {
        self.state.read().inspector.current_frame().cloned()
    }

    /// Get watch expressions and values.
    pub fn watches(&self) -> HashMap<String, String> {
        self.state.read().inspector.watches().clone()
    }

    /// Get the last hit breakpoint ID.
    pub fn last_breakpoint(&self) -> Option<BreakpointId> {
        self.state.read().last_breakpoint
    }

    /// Check if session is stopped.
    fn require_stopped(&self, state: &SessionState) -> Result<(), DebugError> {
        match state.state {
            DebugState::Stopped => Ok(()),
            DebugState::Detached => Err(DebugError::NotAttached),
            DebugState::Terminated => Err(DebugError::Terminated),
            _ => Err(DebugError::NotStopped),
        }
    }
}

impl Clone for DebugSession {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            sandbox_id: self.sandbox_id,
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            created_at: self.created_at,
        }
    }
}

impl std::fmt::Debug for DebugSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugSession")
            .field("id", &self.id)
            .field("sandbox_id", &self.sandbox_id)
            .field("state", &self.state())
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Debug-related errors.
#[derive(Debug, thiserror::Error)]
pub enum DebugError {
    /// Already attached to a sandbox.
    #[error("Already attached to sandbox")]
    AlreadyAttached,

    /// Not attached to any sandbox.
    #[error("Not attached to any sandbox")]
    NotAttached,

    /// Sandbox is not stopped.
    #[error("Sandbox is not stopped")]
    NotStopped,

    /// Sandbox is not running.
    #[error("Sandbox is not running")]
    NotRunning,

    /// Sandbox has terminated.
    #[error("Sandbox has terminated")]
    Terminated,

    /// Cannot step out from top level.
    #[error("Cannot step out from top level")]
    CannotStepOut,

    /// Too many breakpoints.
    #[error("Too many breakpoints (max: {})", super::MAX_BREAKPOINTS)]
    TooManyBreakpoints,

    /// Breakpoint not found.
    #[error("Breakpoint {0} not found")]
    BreakpointNotFound(BreakpointId),

    /// Invalid memory address.
    #[error("Invalid memory address: 0x{0:x}")]
    InvalidAddress(u64),

    /// Evaluation error.
    #[error("Evaluation error: {0}")]
    EvaluationError(String),

    /// Internal error.
    #[error("Internal debug error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_state_display() {
        assert_eq!(DebugState::Running.to_string(), "running");
        assert_eq!(DebugState::Stopped.to_string(), "stopped");
        assert_eq!(DebugState::Terminated.to_string(), "terminated");
    }

    #[test]
    fn test_session_new() {
        let sandbox_id = Uuid::new_v4();
        let session = DebugSession::new(sandbox_id);

        assert_eq!(session.sandbox_id(), sandbox_id);
        assert_eq!(session.state(), DebugState::Detached);
    }

    #[test]
    fn test_session_attach_detach() {
        let session = DebugSession::new(Uuid::new_v4());

        session.attach().unwrap();
        assert_eq!(session.state(), DebugState::Running);

        // Can't attach twice
        assert!(matches!(session.attach(), Err(DebugError::AlreadyAttached)));

        session.detach().unwrap();
        assert_eq!(session.state(), DebugState::Detached);

        // Can't detach twice
        assert!(matches!(session.detach(), Err(DebugError::NotAttached)));
    }

    #[test]
    fn test_session_breakpoints() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        let bp = Breakpoint::function("main");
        let id = session.set_breakpoint(bp).unwrap();

        assert!(session.get_breakpoint(id).is_some());
        assert_eq!(session.breakpoints().len(), 1);

        session.disable_breakpoint(id).unwrap();
        assert!(!session.get_breakpoint(id).unwrap().enabled);

        session.enable_breakpoint(id).unwrap();
        assert!(session.get_breakpoint(id).unwrap().enabled);

        session.remove_breakpoint(id).unwrap();
        assert!(session.get_breakpoint(id).is_none());
    }

    #[test]
    fn test_session_breakpoint_not_found() {
        let session = DebugSession::new(Uuid::new_v4());
        let bad_id = BreakpointId(9999);

        assert!(matches!(
            session.remove_breakpoint(bad_id),
            Err(DebugError::BreakpointNotFound(_))
        ));
    }

    #[test]
    fn test_session_continue_requires_stopped() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        // Running, not stopped
        assert!(matches!(
            session.continue_execution(),
            Err(DebugError::NotStopped)
        ));

        // Simulate stopped
        session.notify_paused(vec![]);
        assert_eq!(session.state(), DebugState::Stopped);

        // Now can continue
        session.continue_execution().unwrap();
        assert_eq!(session.state(), DebugState::Running);
    }

    #[test]
    fn test_session_stepping() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();
        session.notify_paused(vec![StackFrame::new(0, 0x1000)]);

        session.step_into().unwrap();
        assert_eq!(session.state(), DebugState::Stepping);

        session.notify_step_complete(vec![StackFrame::new(0, 0x1004)]);
        assert_eq!(session.state(), DebugState::Stopped);
    }

    #[test]
    fn test_session_step_out() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        // Empty stack - can't step out
        session.notify_paused(vec![]);
        assert!(matches!(session.step_out(), Err(DebugError::CannotStepOut)));

        // With stack
        session.notify_paused(vec![StackFrame::new(0, 0x1000), StackFrame::new(1, 0x2000)]);
        session.step_out().unwrap();
        assert_eq!(session.state(), DebugState::Stepping);
    }

    #[test]
    fn test_session_pause() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        session.pause().unwrap();

        let commands = session.take_pending_commands();
        assert!(commands.iter().any(|c| matches!(c, DebugCommand::Pause)));
    }

    #[test]
    fn test_session_terminate() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        session.terminate().unwrap();

        let commands = session.take_pending_commands();
        assert!(commands
            .iter()
            .any(|c| matches!(c, DebugCommand::Terminate)));

        session.notify_terminated(0);
        assert_eq!(session.state(), DebugState::Terminated);
    }

    #[test]
    fn test_session_watches() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        session.add_watch("x + y");
        assert!(session.watches().contains_key("x + y"));

        session.remove_watch("x + y");
        assert!(!session.watches().contains_key("x + y"));
    }

    #[test]
    fn test_session_breakpoint_hit() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        let bp = Breakpoint::function("main");
        let id = session.set_breakpoint(bp).unwrap();

        session.notify_breakpoint_hit(id, vec![StackFrame::new(0, 0x1000)]);

        assert_eq!(session.state(), DebugState::Stopped);
        assert_eq!(session.last_breakpoint(), Some(id));
        assert_eq!(session.get_breakpoint(id).unwrap().hit_count, 1);
    }

    #[test]
    fn test_session_events() {
        let session = DebugSession::new(Uuid::new_v4());

        session.attach().unwrap();
        session.notify_paused(vec![]);

        let events = session.take_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, DebugEvent::Attached { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, DebugEvent::Paused { .. })));

        // Events cleared
        assert!(session.take_events().is_empty());
    }

    #[test]
    fn test_session_output() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        session.notify_output(OutputStream::Stdout, b"Hello".to_vec());

        let events = session.take_events();
        assert!(events.iter().any(|e| matches!(
            e,
            DebugEvent::Output {
                stream: OutputStream::Stdout,
                ..
            }
        )));
    }

    #[test]
    fn test_session_memory() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();
        session.notify_paused(vec![]);

        session.read_memory(0x1000, 256).unwrap();

        let commands = session.take_pending_commands();
        assert!(commands.iter().any(|c| matches!(
            c,
            DebugCommand::ReadMemory {
                address: 0x1000,
                size: 256
            }
        )));
    }

    #[test]
    fn test_session_call_stack() {
        let session = DebugSession::new(Uuid::new_v4());
        session.attach().unwrap();

        let stack = vec![
            StackFrame::new(0, 0x1000).with_function_name("inner"),
            StackFrame::new(1, 0x2000).with_function_name("outer"),
        ];
        session.update_call_stack(stack);

        let frames = session.call_stack();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].function_name, Some("inner".to_string()));

        let current = session.current_frame().unwrap();
        assert_eq!(current.function_name, Some("inner".to_string()));
    }

    #[test]
    fn test_session_clone_shares_state() {
        let session1 = DebugSession::new(Uuid::new_v4());
        let session2 = session1.clone();

        session1.attach().unwrap();
        assert_eq!(session2.state(), DebugState::Running);
    }

    #[test]
    fn test_debug_config() {
        let config = DebugConfig::default()
            .with_stop_on_entry(true)
            .with_stop_on_exception(false);

        assert!(config.stop_on_entry);
        assert!(!config.stop_on_exception);
    }
}
