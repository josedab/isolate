//! Debug Adapter Protocol (DAP) types and debug session manager.
//!
//! This module defines DAP message types, breakpoint management, and debug
//! session state for WASM sandbox debugging. It does NOT implement the
//! transport layer (TCP/stdio) — just the protocol types and session logic.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// DAP Message Types
// ---------------------------------------------------------------------------

/// Top-level DAP message envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DapMessage {
    Request(DapRequest),
    Response(DapResponse),
    Event(DapEvent),
}

/// A DAP request sent from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DapRequest {
    pub seq: u64,
    pub command: DapCommand,
}

/// Supported DAP commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DapCommand {
    Initialize,
    Launch { program: String, args: Vec<String> },
    SetBreakpoints { source: String, breakpoints: Vec<SourceBreakpoint> },
    Continue { thread_id: u64 },
    Next { thread_id: u64 },
    StepIn { thread_id: u64 },
    StepOut { thread_id: u64 },
    Pause { thread_id: u64 },
    StackTrace { thread_id: u64 },
    Scopes { frame_id: u64 },
    Variables { variables_reference: u64 },
    Evaluate { expression: String, frame_id: Option<u64> },
    Disconnect,
}

// ---------------------------------------------------------------------------
// DAP Response Types
// ---------------------------------------------------------------------------

/// A DAP response sent from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DapResponse {
    pub seq: u64,
    pub request_seq: u64,
    pub success: bool,
    pub command: String,
    pub message: Option<String>,
    pub body: Option<DapResponseBody>,
}

/// Possible response bodies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DapResponseBody {
    Initialize(InitializeResponseBody),
    StackTrace(StackTraceResponseBody),
    Scopes(ScopesResponseBody),
    Variables(VariablesResponseBody),
    Evaluate(EvaluateResponseBody),
    SetBreakpoints(SetBreakpointsResponseBody),
}

// ---------------------------------------------------------------------------
// Supporting Types
// ---------------------------------------------------------------------------

/// A source-level breakpoint request from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBreakpoint {
    pub line: u32,
    pub column: Option<u32>,
    pub condition: Option<String>,
}

/// A verified breakpoint stored by the debug session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: u32,
    pub verified: bool,
    pub line: u32,
    pub source: String,
    pub condition: Option<String>,
    pub hit_count: u32,
}

/// A single stack frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    pub id: u64,
    pub name: String,
    pub source: Option<String>,
    pub line: u32,
    pub column: u32,
}

/// A variable scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scope {
    pub name: String,
    pub variables_reference: u64,
    pub expensive: bool,
}

/// A variable within a scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub var_type: Option<String>,
    pub variables_reference: u64,
}

// ---------------------------------------------------------------------------
// Response Bodies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResponseBody {
    pub supports_breakpoints: bool,
    pub supports_step: bool,
    pub supports_evaluate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackTraceResponseBody {
    pub stack_frames: Vec<StackFrame>,
    pub total_frames: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopesResponseBody {
    pub scopes: Vec<Scope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariablesResponseBody {
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateResponseBody {
    pub result: String,
    pub var_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetBreakpointsResponseBody {
    pub breakpoints: Vec<Breakpoint>,
}

// ---------------------------------------------------------------------------
// DAP Events
// ---------------------------------------------------------------------------

/// A DAP event emitted by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DapEvent {
    pub seq: u64,
    pub event: DapEventType,
}

/// Event payload types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DapEventType {
    Initialized,
    Stopped { reason: StopReason, thread_id: u64 },
    Continued { thread_id: u64 },
    Exited { exit_code: i32 },
    Terminated,
    Output { category: OutputCategory, output: String },
}

/// Reason the execution stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StopReason {
    Breakpoint,
    Step,
    Pause,
    Exception,
}

/// Output stream category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputCategory {
    Console,
    Stdout,
    Stderr,
}

// ---------------------------------------------------------------------------
// DebugSession
// ---------------------------------------------------------------------------

/// Current state of a debug session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Uninitialized,
    Initialized,
    Running,
    Stopped(StopReason),
    Terminated,
}

/// Manages debug state, breakpoints, and DAP request handling.
pub struct DebugSession {
    seq_counter: u64,
    state: SessionState,
    breakpoints: HashMap<String, Vec<Breakpoint>>,
    next_breakpoint_id: u32,
}

impl DebugSession {
    /// Create a new debug session in the `Uninitialized` state.
    pub fn new() -> Self {
        Self {
            seq_counter: 0,
            state: SessionState::Uninitialized,
            breakpoints: HashMap::new(),
            next_breakpoint_id: 1,
        }
    }

    /// Return the current session state.
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// Process a DAP request and return an appropriate response.
    pub fn handle_request(&mut self, request: DapRequest) -> DapResponse {
        let seq = self.next_seq();
        let request_seq = request.seq;

        match request.command {
            DapCommand::Initialize => {
                self.state = SessionState::Initialized;
                DapResponse {
                    seq,
                    request_seq,
                    success: true,
                    command: "initialize".into(),
                    message: None,
                    body: Some(DapResponseBody::Initialize(InitializeResponseBody {
                        supports_breakpoints: true,
                        supports_step: true,
                        supports_evaluate: true,
                    })),
                }
            }
            DapCommand::Launch { .. } => {
                self.state = SessionState::Running;
                self.success_response(seq, request_seq, "launch")
            }
            DapCommand::SetBreakpoints { ref source, ref breakpoints } => {
                let verified = self.set_breakpoints(source, breakpoints.clone());
                DapResponse {
                    seq,
                    request_seq,
                    success: true,
                    command: "setBreakpoints".into(),
                    message: None,
                    body: Some(DapResponseBody::SetBreakpoints(
                        SetBreakpointsResponseBody { breakpoints: verified },
                    )),
                }
            }
            DapCommand::Continue { .. } => {
                self.state = SessionState::Running;
                self.success_response(seq, request_seq, "continue")
            }
            DapCommand::Pause { .. } => {
                self.state = SessionState::Stopped(StopReason::Pause);
                self.success_response(seq, request_seq, "pause")
            }
            DapCommand::Next { .. } | DapCommand::StepIn { .. } | DapCommand::StepOut { .. } => {
                self.state = SessionState::Running;
                let cmd_name = match request.command {
                    DapCommand::Next { .. } => "next",
                    DapCommand::StepIn { .. } => "stepIn",
                    DapCommand::StepOut { .. } => "stepOut",
                    _ => unreachable!(),
                };
                self.success_response(seq, request_seq, cmd_name)
            }
            DapCommand::StackTrace { .. } => {
                let frames = vec![StackFrame {
                    id: 0,
                    name: "main".into(),
                    source: None,
                    line: 1,
                    column: 0,
                }];
                DapResponse {
                    seq,
                    request_seq,
                    success: true,
                    command: "stackTrace".into(),
                    message: None,
                    body: Some(DapResponseBody::StackTrace(StackTraceResponseBody {
                        total_frames: frames.len() as u32,
                        stack_frames: frames,
                    })),
                }
            }
            DapCommand::Scopes { .. } => {
                let scopes = vec![
                    Scope { name: "Locals".into(), variables_reference: 1, expensive: false },
                    Scope { name: "Globals".into(), variables_reference: 2, expensive: false },
                ];
                DapResponse {
                    seq,
                    request_seq,
                    success: true,
                    command: "scopes".into(),
                    message: None,
                    body: Some(DapResponseBody::Scopes(ScopesResponseBody { scopes })),
                }
            }
            DapCommand::Variables { .. } => DapResponse {
                seq,
                request_seq,
                success: true,
                command: "variables".into(),
                message: None,
                body: Some(DapResponseBody::Variables(VariablesResponseBody {
                    variables: vec![],
                })),
            },
            DapCommand::Evaluate { ref expression, .. } => DapResponse {
                seq,
                request_seq,
                success: true,
                command: "evaluate".into(),
                message: None,
                body: Some(DapResponseBody::Evaluate(EvaluateResponseBody {
                    result: expression.clone(),
                    var_type: None,
                })),
            },
            DapCommand::Disconnect => {
                self.state = SessionState::Terminated;
                self.success_response(seq, request_seq, "disconnect")
            }
        }
    }

    /// Replace all breakpoints for the given source and return the verified list.
    pub fn set_breakpoints(
        &mut self,
        source: &str,
        breakpoints: Vec<SourceBreakpoint>,
    ) -> Vec<Breakpoint> {
        let verified: Vec<Breakpoint> = breakpoints
            .into_iter()
            .map(|sb| {
                let id = self.next_breakpoint_id;
                self.next_breakpoint_id += 1;
                Breakpoint {
                    id,
                    verified: true,
                    line: sb.line,
                    source: source.to_string(),
                    condition: sb.condition,
                    hit_count: 0,
                }
            })
            .collect();
        self.breakpoints.insert(source.to_string(), verified.clone());
        verified
    }

    /// Get breakpoints for a specific source file.
    pub fn get_breakpoints(&self, source: &str) -> Vec<&Breakpoint> {
        self.breakpoints
            .get(source)
            .map(|bps| bps.iter().collect())
            .unwrap_or_default()
    }

    /// Get all breakpoints across every source.
    pub fn all_breakpoints(&self) -> Vec<&Breakpoint> {
        self.breakpoints.values().flat_map(|bps| bps.iter()).collect()
    }

    /// Increment and return the next sequence number.
    pub fn next_seq(&mut self) -> u64 {
        self.seq_counter += 1;
        self.seq_counter
    }

    /// Create a [`DapEvent`] stamped with the next sequence number.
    pub fn create_event(&mut self, event_type: DapEventType) -> DapEvent {
        DapEvent { seq: self.next_seq(), event: event_type }
    }

    fn success_response(&self, seq: u64, request_seq: u64, command: &str) -> DapResponse {
        DapResponse {
            seq,
            request_seq,
            success: true,
            command: command.into(),
            message: None,
            body: None,
        }
    }
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Transport Configuration
// ---------------------------------------------------------------------------

/// Debug transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DebugTransportConfig {
    /// TCP socket transport.
    Tcp {
        /// Host to bind/connect to.
        host: String,
        /// Port number.
        port: u16,
    },
    /// Standard I/O transport (stdin/stdout).
    Stdio,
    /// Unix domain socket transport.
    #[cfg(unix)]
    UnixSocket {
        /// Path to the socket.
        path: String,
    },
}

impl Default for DebugTransportConfig {
    fn default() -> Self {
        Self::Tcp { host: "127.0.0.1".to_string(), port: 4711 }
    }
}

// ---------------------------------------------------------------------------
// Execution Recording for Deterministic Replay
// ---------------------------------------------------------------------------

/// Direction of an I/O event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoDirection {
    /// Data going into the sandbox (stdin, filesystem reads).
    Input,
    /// Data coming out of the sandbox (stdout, stderr).
    Output,
}

/// A recorded I/O event for deterministic replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedIoEvent {
    /// Sequence number.
    pub seq: u64,
    /// Direction of the I/O.
    pub direction: IoDirection,
    /// Raw data bytes.
    pub data: Vec<u8>,
    /// Microseconds since execution start.
    pub timestamp_us: u64,
}

/// Records execution I/O for time-travel debugging.
pub struct ExecutionRecorder {
    events: Vec<RecordedIoEvent>,
    max_events: usize,
}

impl ExecutionRecorder {
    /// Create a new recorder with the given capacity.
    pub fn new(max_events: usize) -> Self {
        Self { events: Vec::new(), max_events }
    }

    /// Record an I/O event.
    pub fn record(&mut self, event: RecordedIoEvent) {
        if self.events.len() >= self.max_events {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Get all recorded events.
    pub fn events(&self) -> &[RecordedIoEvent] {
        &self.events
    }

    /// Get the total duration in microseconds.
    pub fn duration_us(&self) -> u64 {
        if self.events.len() < 2 {
            return 0;
        }
        self.events.last().unwrap().timestamp_us - self.events.first().unwrap().timestamp_us
    }

    /// Clear all recorded events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Serialize the recording for storage.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.events).unwrap_or_default()
    }

    /// Load a recording from JSON.
    pub fn from_json(json: &str) -> Option<Self> {
        let events: Vec<RecordedIoEvent> = serde_json::from_str(json).ok()?;
        let len = events.len();
        Some(Self { events, max_events: len.max(1000) })
    }
}

/// Cursor for replaying recorded execution events.
pub struct ReplayCursor {
    events: Vec<RecordedIoEvent>,
    position: usize,
}

impl ReplayCursor {
    /// Create a new replay cursor.
    pub fn new(events: Vec<RecordedIoEvent>) -> Self {
        Self { events, position: 0 }
    }

    /// Check if there are more events.
    pub fn has_next(&self) -> bool {
        self.position < self.events.len()
    }

    /// Get the next event and advance.
    pub fn next(&mut self) -> Option<&RecordedIoEvent> {
        if self.position < self.events.len() {
            let event = &self.events[self.position];
            self.position += 1;
            Some(event)
        } else {
            None
        }
    }

    /// Seek to a specific position.
    pub fn seek_to(&mut self, position: usize) {
        self.position = position.min(self.events.len());
    }

    /// Get the current position.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Get the total number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the recording is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let session = DebugSession::new();
        assert_eq!(*session.state(), SessionState::Uninitialized);
    }

    #[test]
    fn test_initialize_request() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::Initialize,
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Initialized);
        assert!(matches!(resp.body, Some(DapResponseBody::Initialize(_))));
    }

    #[test]
    fn test_launch_request() {
        let mut session = DebugSession::new();
        session.handle_request(DapRequest { seq: 1, command: DapCommand::Initialize });
        let resp = session.handle_request(DapRequest {
            seq: 2,
            command: DapCommand::Launch {
                program: "test.wasm".into(),
                args: vec![],
            },
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Running);
    }

    #[test]
    fn test_set_breakpoints() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::SetBreakpoints {
                source: "main.rs".into(),
                breakpoints: vec![
                    SourceBreakpoint { line: 10, column: None, condition: None },
                    SourceBreakpoint { line: 20, column: Some(5), condition: None },
                ],
            },
        });
        assert!(resp.success);
        let Some(DapResponseBody::SetBreakpoints(body)) = resp.body else {
            unreachable!("expected SetBreakpoints body");
        };
        assert_eq!(body.breakpoints.len(), 2);
        assert!(body.breakpoints[0].verified);
        assert_eq!(body.breakpoints[0].line, 10);
        assert_eq!(body.breakpoints[1].line, 20);
    }

    #[test]
    fn test_continue_changes_state() {
        let mut session = DebugSession::new();
        session.handle_request(DapRequest { seq: 1, command: DapCommand::Initialize });
        session.handle_request(DapRequest {
            seq: 2,
            command: DapCommand::Pause { thread_id: 1 },
        });
        assert_eq!(*session.state(), SessionState::Stopped(StopReason::Pause));

        let resp = session.handle_request(DapRequest {
            seq: 3,
            command: DapCommand::Continue { thread_id: 1 },
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Running);
    }

    #[test]
    fn test_pause_changes_state() {
        let mut session = DebugSession::new();
        session.handle_request(DapRequest { seq: 1, command: DapCommand::Initialize });
        let resp = session.handle_request(DapRequest {
            seq: 2,
            command: DapCommand::Pause { thread_id: 1 },
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Stopped(StopReason::Pause));
    }

    #[test]
    fn test_stack_trace_returns_frames() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::StackTrace { thread_id: 1 },
        });
        assert!(resp.success);
        let Some(DapResponseBody::StackTrace(body)) = resp.body else {
            unreachable!("expected StackTrace body");
        };
        assert_eq!(body.stack_frames.len(), 1);
        assert_eq!(body.stack_frames[0].name, "main");
        assert_eq!(body.total_frames, 1);
    }

    #[test]
    fn test_scopes_returns_scopes() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::Scopes { frame_id: 0 },
        });
        assert!(resp.success);
        let Some(DapResponseBody::Scopes(body)) = resp.body else {
            unreachable!("expected Scopes body");
        };
        assert_eq!(body.scopes.len(), 2);
        assert_eq!(body.scopes[0].name, "Locals");
        assert_eq!(body.scopes[1].name, "Globals");
    }

    #[test]
    fn test_variables_returns_empty() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::Variables { variables_reference: 1 },
        });
        assert!(resp.success);
        let Some(DapResponseBody::Variables(body)) = resp.body else {
            unreachable!("expected Variables body");
        };
        assert!(body.variables.is_empty());
    }

    #[test]
    fn test_evaluate_echoes_expression() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::Evaluate {
                expression: "2 + 2".into(),
                frame_id: None,
            },
        });
        assert!(resp.success);
        let Some(DapResponseBody::Evaluate(body)) = resp.body else {
            unreachable!("expected Evaluate body");
        };
        assert_eq!(body.result, "2 + 2");
    }

    #[test]
    fn test_disconnect_terminates() {
        let mut session = DebugSession::new();
        session.handle_request(DapRequest { seq: 1, command: DapCommand::Initialize });
        let resp = session.handle_request(DapRequest {
            seq: 2,
            command: DapCommand::Disconnect,
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Terminated);
    }

    #[test]
    fn test_set_breakpoints_replaces_existing() {
        let mut session = DebugSession::new();

        session.set_breakpoints(
            "main.rs",
            vec![SourceBreakpoint { line: 1, column: None, condition: None }],
        );
        assert_eq!(session.get_breakpoints("main.rs").len(), 1);

        session.set_breakpoints(
            "main.rs",
            vec![
                SourceBreakpoint { line: 5, column: None, condition: None },
                SourceBreakpoint { line: 10, column: None, condition: None },
            ],
        );
        let bps = session.get_breakpoints("main.rs");
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0].line, 5);
        assert_eq!(bps[1].line, 10);
    }

    #[test]
    fn test_get_breakpoints_missing_source() {
        let session = DebugSession::new();
        assert!(session.get_breakpoints("nonexistent.rs").is_empty());
    }

    #[test]
    fn test_all_breakpoints_across_sources() {
        let mut session = DebugSession::new();
        session.set_breakpoints(
            "a.rs",
            vec![SourceBreakpoint { line: 1, column: None, condition: None }],
        );
        session.set_breakpoints(
            "b.rs",
            vec![
                SourceBreakpoint { line: 2, column: None, condition: None },
                SourceBreakpoint { line: 3, column: None, condition: None },
            ],
        );
        assert_eq!(session.all_breakpoints().len(), 3);
    }

    #[test]
    fn test_next_seq_increments() {
        let mut session = DebugSession::new();
        let s1 = session.next_seq();
        let s2 = session.next_seq();
        let s3 = session.next_seq();
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[test]
    fn test_create_event() {
        let mut session = DebugSession::new();
        let event = session.create_event(DapEventType::Initialized);
        assert!(event.seq > 0);
        assert!(matches!(event.event, DapEventType::Initialized));
    }

    #[test]
    fn test_dap_message_serialization() {
        let msg = DapMessage::Request(DapRequest {
            seq: 1,
            command: DapCommand::Initialize,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DapMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DapMessage::Request(_)));

        let msg = DapMessage::Event(DapEvent {
            seq: 2,
            event: DapEventType::Terminated,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DapMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DapMessage::Event(_)));
    }

    #[test]
    fn test_stop_reason_variants() {
        let reasons = [
            StopReason::Breakpoint,
            StopReason::Step,
            StopReason::Pause,
            StopReason::Exception,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).unwrap();
            let parsed: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, reason);
        }
    }

    #[test]
    fn test_output_category_variants() {
        let categories = [
            OutputCategory::Console,
            OutputCategory::Stdout,
            OutputCategory::Stderr,
        ];
        for cat in &categories {
            let json = serde_json::to_string(cat).unwrap();
            let parsed: OutputCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, cat);
        }
    }

    #[test]
    fn test_conditional_breakpoint() {
        let mut session = DebugSession::new();
        let bps = session.set_breakpoints(
            "main.rs",
            vec![SourceBreakpoint {
                line: 42,
                column: None,
                condition: Some("x > 10".into()),
            }],
        );
        assert_eq!(bps.len(), 1);
        assert_eq!(bps[0].condition.as_deref(), Some("x > 10"));
        assert_eq!(bps[0].line, 42);
        assert!(bps[0].verified);
    }

    #[test]
    fn test_pause_while_already_paused() {
        let mut session = DebugSession::new();
        session.handle_request(DapRequest { seq: 1, command: DapCommand::Initialize });

        // First pause
        session.handle_request(DapRequest {
            seq: 2,
            command: DapCommand::Pause { thread_id: 1 },
        });
        assert_eq!(*session.state(), SessionState::Stopped(StopReason::Pause));

        // Pause again while already paused — should succeed
        let resp = session.handle_request(DapRequest {
            seq: 3,
            command: DapCommand::Pause { thread_id: 1 },
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Stopped(StopReason::Pause));
    }

    #[test]
    fn test_breakpoint_add_remove_via_replace() {
        let mut session = DebugSession::new();

        // Add breakpoints
        let bps = session.set_breakpoints(
            "test.rs",
            vec![
                SourceBreakpoint { line: 1, column: None, condition: None },
                SourceBreakpoint { line: 5, column: None, condition: None },
                SourceBreakpoint { line: 10, column: None, condition: None },
            ],
        );
        assert_eq!(bps.len(), 3);
        assert_eq!(session.get_breakpoints("test.rs").len(), 3);

        // Replace with fewer (effectively removing some)
        let bps = session.set_breakpoints(
            "test.rs",
            vec![SourceBreakpoint { line: 5, column: None, condition: None }],
        );
        assert_eq!(bps.len(), 1);
        assert_eq!(session.get_breakpoints("test.rs").len(), 1);

        // Clear all by setting empty
        let bps = session.set_breakpoints("test.rs", vec![]);
        assert_eq!(bps.len(), 0);
        assert_eq!(session.get_breakpoints("test.rs").len(), 0);
    }

    #[test]
    fn test_seq_counter_starts_at_zero() {
        let mut session = DebugSession::new();
        assert_eq!(session.next_seq(), 1);
        assert_eq!(session.next_seq(), 2);
    }

    #[test]
    fn test_seq_counter_wrapping() {
        let mut session = DebugSession::new();
        session.seq_counter = u64::MAX - 1;
        assert_eq!(session.next_seq(), u64::MAX);
        // Next call would overflow; Rust wraps in debug → panics, or wraps in release.
        // We just verify it reaches MAX.
    }

    #[test]
    fn test_many_breakpoints() {
        let mut session = DebugSession::new();
        let bps: Vec<SourceBreakpoint> = (0..1000)
            .map(|i| SourceBreakpoint { line: i, column: None, condition: None })
            .collect();
        let verified = session.set_breakpoints("large.rs", bps);
        assert_eq!(verified.len(), 1000);
        assert_eq!(session.all_breakpoints().len(), 1000);
    }

    #[test]
    fn test_step_commands_set_running() {
        let mut session = DebugSession::new();
        session.handle_request(DapRequest { seq: 1, command: DapCommand::Initialize });

        let commands = [
            DapCommand::Next { thread_id: 1 },
            DapCommand::StepIn { thread_id: 1 },
            DapCommand::StepOut { thread_id: 1 },
        ];

        for (i, cmd) in commands.into_iter().enumerate() {
            // Pause first
            session.handle_request(DapRequest {
                seq: (i * 2 + 2) as u64,
                command: DapCommand::Pause { thread_id: 1 },
            });
            assert_eq!(*session.state(), SessionState::Stopped(StopReason::Pause));

            // Step should set Running
            let resp = session.handle_request(DapRequest {
                seq: (i * 2 + 3) as u64,
                command: cmd,
            });
            assert!(resp.success);
            assert_eq!(*session.state(), SessionState::Running);
        }
    }

    #[test]
    fn test_breakpoint_ids_are_unique() {
        let mut session = DebugSession::new();
        let bp1 = session.set_breakpoints(
            "a.rs",
            vec![SourceBreakpoint { line: 1, column: None, condition: None }],
        );
        let bp2 = session.set_breakpoints(
            "b.rs",
            vec![SourceBreakpoint { line: 1, column: None, condition: None }],
        );
        assert_ne!(bp1[0].id, bp2[0].id);
    }

    #[test]
    fn test_disconnect_from_uninitialized() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::Disconnect,
        });
        assert!(resp.success);
        assert_eq!(*session.state(), SessionState::Terminated);
    }

    #[test]
    fn test_create_event_increments_seq() {
        let mut session = DebugSession::new();
        let e1 = session.create_event(DapEventType::Initialized);
        let e2 = session.create_event(DapEventType::Terminated);
        assert_eq!(e2.seq, e1.seq + 1);
    }

    #[test]
    fn test_evaluate_with_frame_id() {
        let mut session = DebugSession::new();
        let resp = session.handle_request(DapRequest {
            seq: 1,
            command: DapCommand::Evaluate {
                expression: "x + y".into(),
                frame_id: Some(42),
            },
        });
        assert!(resp.success);
        let Some(DapResponseBody::Evaluate(body)) = resp.body else {
            unreachable!("expected Evaluate body");
        };
        assert_eq!(body.result, "x + y");
    }

    #[test]
    fn test_debug_transport_config() {
        let tcp = DebugTransportConfig::Tcp { host: "127.0.0.1".into(), port: 4711 };
        match tcp {
            DebugTransportConfig::Tcp { host, port } => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, 4711);
            }
            _ => panic!("Expected Tcp"),
        }

        let stdio = DebugTransportConfig::Stdio;
        assert!(matches!(stdio, DebugTransportConfig::Stdio));
    }

    #[test]
    fn test_execution_recorder() {
        let mut recorder = ExecutionRecorder::new(100);
        recorder.record(RecordedIoEvent {
            seq: 0,
            direction: IoDirection::Input,
            data: b"hello".to_vec(),
            timestamp_us: 1000,
        });
        recorder.record(RecordedIoEvent {
            seq: 1,
            direction: IoDirection::Output,
            data: b"world".to_vec(),
            timestamp_us: 2000,
        });
        assert_eq!(recorder.events().len(), 2);
        assert_eq!(recorder.duration_us(), 1000);
    }

    #[test]
    fn test_execution_recorder_capacity() {
        let mut recorder = ExecutionRecorder::new(2);
        for i in 0..5 {
            recorder.record(RecordedIoEvent {
                seq: i,
                direction: IoDirection::Input,
                data: vec![i as u8],
                timestamp_us: i as u64 * 100,
            });
        }
        assert_eq!(recorder.events().len(), 2);
    }

    #[test]
    fn test_replay_cursor() {
        let mut recorder = ExecutionRecorder::new(100);
        for i in 0..3 {
            recorder.record(RecordedIoEvent {
                seq: i,
                direction: IoDirection::Output,
                data: vec![i as u8],
                timestamp_us: i as u64 * 1000,
            });
        }

        let mut cursor = ReplayCursor::new(recorder.events().to_vec());
        assert!(cursor.has_next());
        let e = cursor.next().unwrap();
        assert_eq!(e.seq, 0);

        let e = cursor.next().unwrap();
        assert_eq!(e.seq, 1);

        cursor.seek_to(0);
        let e = cursor.next().unwrap();
        assert_eq!(e.seq, 0);
    }
}
