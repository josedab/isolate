//! Execution recording for time-travel debugging.

use super::event::{MemoryChange, RegisterChange, SourceLocation, WasiCallInfo};
use super::{EventType, ExecutionEvent, RecordingId, TimeTravelConfig};
use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use uuid::Uuid;

/// Configuration for recording sessions.
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    /// Maximum number of events to buffer before flushing.
    pub buffer_size: usize,
    /// Whether to record memory changes.
    pub record_memory: bool,
    /// Whether to record register changes.
    pub record_registers: bool,
    /// Whether to record WASI calls.
    pub record_wasi: bool,
    /// Whether to include source locations.
    pub include_source: bool,
    /// Sampling rate (1 = every event, 10 = every 10th event).
    pub sampling_rate: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            buffer_size: 10_000,
            record_memory: true,
            record_registers: true,
            record_wasi: true,
            include_source: true,
            sampling_rate: 1,
        }
    }
}

impl RecordingConfig {
    /// Create a minimal configuration for low overhead.
    pub fn minimal() -> Self {
        Self {
            buffer_size: 1_000,
            record_memory: false,
            record_registers: false,
            record_wasi: true,
            include_source: false,
            sampling_rate: 100,
        }
    }

    /// Create from TimeTravelConfig.
    pub fn from_timetravel_config(config: &TimeTravelConfig) -> Self {
        Self {
            buffer_size: config.max_events.min(100_000),
            record_memory: config.record_memory,
            record_registers: config.record_registers,
            record_wasi: config.record_wasi_calls,
            include_source: true,
            sampling_rate: 1,
        }
    }
}

/// A recording session capturing execution events.
#[derive(Debug)]
pub struct RecordingSession {
    /// Unique session ID.
    id: RecordingId,
    /// Sandbox ID being recorded.
    sandbox_id: Uuid,
    /// Start time of the recording.
    start_time: DateTime<Utc>,
    /// End time of the recording (if finished).
    end_time: Option<DateTime<Utc>>,
    /// Recorded events.
    events: Vec<ExecutionEvent>,
    /// Total events recorded (including dropped).
    total_events: u64,
    /// Events dropped due to limits.
    dropped_events: u64,
    /// Recording configuration.
    config: RecordingConfig,
    /// Whether the session is active.
    active: bool,
}

impl RecordingSession {
    /// Create a new recording session.
    pub fn new(sandbox_id: Uuid, config: RecordingConfig) -> Self {
        Self {
            id: Uuid::new_v4(),
            sandbox_id,
            start_time: Utc::now(),
            end_time: None,
            events: Vec::with_capacity(config.buffer_size),
            total_events: 0,
            dropped_events: 0,
            config,
            active: true,
        }
    }

    /// Get the session ID.
    pub fn id(&self) -> RecordingId {
        self.id
    }

    /// Get the sandbox ID.
    pub fn sandbox_id(&self) -> Uuid {
        self.sandbox_id
    }

    /// Get the start time.
    pub fn start_time(&self) -> DateTime<Utc> {
        self.start_time
    }

    /// Get the end time.
    pub fn end_time(&self) -> Option<DateTime<Utc>> {
        self.end_time
    }

    /// Check if the session is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the number of recorded events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Get total events (including dropped).
    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    /// Get the number of dropped events.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
    }

    /// Get all recorded events.
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    /// Add an event to the session.
    pub fn add_event(&mut self, event: ExecutionEvent) {
        self.total_events += 1;

        // Apply sampling
        if self.config.sampling_rate > 1
            && self.total_events % self.config.sampling_rate as u64 != 0
        {
            self.dropped_events += 1;
            return;
        }

        // Check buffer limits
        if self.events.len() >= self.config.buffer_size {
            self.dropped_events += 1;
            return;
        }

        self.events.push(event);
    }

    /// Stop the recording session.
    pub fn stop(&mut self) {
        self.active = false;
        self.end_time = Some(Utc::now());
    }

    /// Get recording duration.
    pub fn duration(&self) -> chrono::Duration {
        let end = self.end_time.unwrap_or_else(Utc::now);
        end - self.start_time
    }

    /// Get recording statistics.
    pub fn stats(&self) -> RecordingStats {
        RecordingStats {
            event_count: self.events.len(),
            total_events: self.total_events,
            dropped_events: self.dropped_events,
            duration: self.duration(),
            memory_events: self
                .events
                .iter()
                .filter(|e| matches!(e.event_type, EventType::MemoryRead | EventType::MemoryWrite))
                .count(),
            wasi_events: self
                .events
                .iter()
                .filter(|e| matches!(e.event_type, EventType::WasiCall | EventType::WasiReturn))
                .count(),
        }
    }
}

/// Statistics about a recording session.
#[derive(Debug, Clone)]
pub struct RecordingStats {
    /// Number of recorded events.
    pub event_count: usize,
    /// Total events seen.
    pub total_events: u64,
    /// Events dropped due to limits.
    pub dropped_events: u64,
    /// Recording duration.
    pub duration: chrono::Duration,
    /// Number of memory events.
    pub memory_events: usize,
    /// Number of WASI events.
    pub wasi_events: usize,
}

/// Recorder for capturing execution events.
pub struct Recorder {
    /// Recording configuration.
    config: RecordingConfig,
    /// Current active session.
    session: Option<Arc<Mutex<RecordingSession>>>,
    /// Whether recording is enabled.
    enabled: AtomicBool,
    /// Next event ID.
    next_event_id: AtomicU64,
    /// Event buffer for batch processing.
    event_buffer: Mutex<VecDeque<ExecutionEvent>>,
}

impl Recorder {
    /// Create a new recorder.
    pub fn new() -> Self {
        Self::with_config(RecordingConfig::default())
    }

    /// Create a recorder with custom configuration.
    pub fn with_config(config: RecordingConfig) -> Self {
        Self {
            config,
            session: None,
            enabled: AtomicBool::new(false),
            next_event_id: AtomicU64::new(0),
            event_buffer: Mutex::new(VecDeque::with_capacity(1000)),
        }
    }

    /// Check if recording is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Start a new recording session.
    pub fn start_recording(&mut self, sandbox_id: Uuid) -> RecordingId {
        let session = RecordingSession::new(sandbox_id, self.config.clone());
        let id = session.id();
        self.session = Some(Arc::new(Mutex::new(session)));
        self.enabled.store(true, Ordering::Release);
        self.next_event_id.store(0, Ordering::Release);
        id
    }

    /// Stop the current recording session.
    pub fn stop_recording(&mut self) -> Option<Arc<Mutex<RecordingSession>>> {
        self.enabled.store(false, Ordering::Release);
        if let Some(session) = &self.session {
            session.lock().unwrap().stop();
        }
        self.session.take()
    }

    /// Get the current session.
    pub fn session(&self) -> Option<Arc<Mutex<RecordingSession>>> {
        self.session.clone()
    }

    /// Record an instruction execution.
    pub fn record_instruction(&self, ip: u64, function_name: Option<&str>) {
        if !self.is_enabled() {
            return;
        }

        let event = self.create_event(EventType::Instruction, ip).map(|mut e| {
            if let Some(name) = function_name {
                e.function_name = Some(name.to_string());
            }
            e
        });

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a function call.
    pub fn record_function_call(&self, ip: u64, function_name: &str, stack_depth: u32) {
        if !self.is_enabled() {
            return;
        }

        let event = self
            .create_event(EventType::FunctionCall, ip)
            .map(|e| e.with_function(function_name).with_stack_depth(stack_depth));

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a function return.
    pub fn record_function_return(&self, ip: u64, function_name: &str, stack_depth: u32) {
        if !self.is_enabled() {
            return;
        }

        let event = self
            .create_event(EventType::FunctionReturn, ip)
            .map(|e| e.with_function(function_name).with_stack_depth(stack_depth));

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a memory read.
    pub fn record_memory_read(&self, ip: u64, address: u64, value: &[u8]) {
        if !self.is_enabled() || !self.config.record_memory {
            return;
        }

        let event = self
            .create_event(EventType::MemoryRead, ip)
            .map(|e| e.with_memory_change(MemoryChange::read(address, value.to_vec())));

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a memory write.
    pub fn record_memory_write(&self, ip: u64, address: u64, old_value: &[u8], new_value: &[u8]) {
        if !self.is_enabled() || !self.config.record_memory {
            return;
        }

        let event = self.create_event(EventType::MemoryWrite, ip).map(|e| {
            e.with_memory_change(MemoryChange::new(address, old_value.to_vec(), new_value.to_vec()))
        });

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a register change.
    pub fn record_register_change(&self, ip: u64, name: &str, old_value: u64, new_value: u64) {
        if !self.is_enabled() || !self.config.record_registers {
            return;
        }

        let event = self
            .create_event(EventType::Instruction, ip)
            .map(|e| e.with_register_change(RegisterChange::new(name, old_value, new_value)));

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a WASI call.
    pub fn record_wasi_call(&self, ip: u64, function: &str, arguments: Vec<String>) {
        if !self.is_enabled() || !self.config.record_wasi {
            return;
        }

        let event = self
            .create_event(EventType::WasiCall, ip)
            .map(|e| e.with_wasi_call(WasiCallInfo::new(function, arguments)));

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a WASI return.
    pub fn record_wasi_return(
        &self,
        ip: u64,
        function: &str,
        result: Option<&str>,
        error_code: Option<u32>,
    ) {
        if !self.is_enabled() || !self.config.record_wasi {
            return;
        }

        let mut call_info = WasiCallInfo::new(function, vec![]);
        if let Some(result) = result {
            call_info = call_info.with_return(result);
        }
        if let Some(code) = error_code {
            call_info = call_info.with_error(code);
        }

        let event =
            self.create_event(EventType::WasiReturn, ip).map(|e| e.with_wasi_call(call_info));

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record a breakpoint hit.
    pub fn record_breakpoint(&self, ip: u64, source: Option<SourceLocation>) {
        if !self.is_enabled() {
            return;
        }

        let event = self.create_event(EventType::Breakpoint, ip).map(|mut e| {
            if let Some(loc) = source {
                e = e.with_source(loc);
            }
            e
        });

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record an exception.
    pub fn record_exception(&self, ip: u64, message: &str) {
        if !self.is_enabled() {
            return;
        }

        let event = self.create_event(EventType::Exception, ip).map(|mut e| {
            e.data = Some(message.as_bytes().to_vec());
            e
        });

        if let Some(event) = event {
            self.record_event(event);
        }
    }

    /// Record execution start.
    pub fn record_start(&self) {
        if !self.is_enabled() {
            return;
        }

        let event = ExecutionEvent::start();
        self.record_event(event);
    }

    /// Record execution end.
    pub fn record_end(&self, fuel_consumed: u64) {
        if !self.is_enabled() {
            return;
        }

        let id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let event = ExecutionEvent::end(id, fuel_consumed);
        self.record_event(event);
    }

    /// Create a new event with the next ID.
    fn create_event(&self, event_type: EventType, ip: u64) -> Option<ExecutionEvent> {
        let id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        Some(ExecutionEvent::new(id, event_type, ip))
    }

    /// Record an event to the current session.
    fn record_event(&self, event: ExecutionEvent) {
        if let Some(session) = &self.session {
            if let Ok(mut session) = session.lock() {
                session.add_event(event);
            }
        }
    }

    /// Build a timeline from the current session.
    pub fn build_timeline(&self) -> Result<super::Timeline> {
        let session = self.session.as_ref().ok_or_else(|| Error::InvalidState {
            expected: "Active recording session".to_string(),
            actual: "No session".to_string(),
        })?;

        let session =
            session.lock().map_err(|_| Error::Engine("Failed to lock session".to_string()))?;

        super::Timeline::from_events(session.events().to_vec())
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_config_default() {
        let config = RecordingConfig::default();
        assert!(config.record_memory);
        assert!(config.record_registers);
        assert!(config.record_wasi);
        assert_eq!(config.sampling_rate, 1);
    }

    #[test]
    fn test_recording_config_minimal() {
        let config = RecordingConfig::minimal();
        assert!(!config.record_memory);
        assert!(!config.record_registers);
        assert!(config.record_wasi);
        assert_eq!(config.sampling_rate, 100);
    }

    #[test]
    fn test_recording_session() {
        let config = RecordingConfig::default();
        let sandbox_id = Uuid::new_v4();
        let mut session = RecordingSession::new(sandbox_id, config);

        assert!(session.is_active());
        assert_eq!(session.event_count(), 0);

        let event = ExecutionEvent::new(0, EventType::Instruction, 0x1000);
        session.add_event(event);
        assert_eq!(session.event_count(), 1);

        session.stop();
        assert!(!session.is_active());
        assert!(session.end_time().is_some());
    }

    #[test]
    fn test_recording_session_sampling() {
        let mut config = RecordingConfig::default();
        config.sampling_rate = 10;
        let sandbox_id = Uuid::new_v4();
        let mut session = RecordingSession::new(sandbox_id, config);

        // Add 100 events
        for i in 0..100 {
            let event = ExecutionEvent::new(i, EventType::Instruction, 0x1000 + i);
            session.add_event(event);
        }

        // Only every 10th event should be recorded
        assert_eq!(session.event_count(), 10);
        assert_eq!(session.total_events(), 100);
        assert_eq!(session.dropped_events(), 90);
    }

    #[test]
    fn test_recorder() {
        let mut recorder = Recorder::new();
        let sandbox_id = Uuid::new_v4();

        assert!(!recorder.is_enabled());

        let recording_id = recorder.start_recording(sandbox_id);
        assert!(recorder.is_enabled());

        recorder.record_start();
        recorder.record_instruction(0x1000, Some("main"));
        recorder.record_function_call(0x2000, "helper", 1);
        recorder.record_memory_write(0x2010, 0x3000, &[0], &[42]);
        recorder.record_function_return(0x2020, "helper", 1);
        recorder.record_end(1000);

        let session = recorder.stop_recording().unwrap();
        let session = session.lock().unwrap();

        assert_eq!(session.id(), recording_id);
        assert!(session.event_count() >= 5);
    }

    #[test]
    fn test_recorder_disabled() {
        let recorder = Recorder::new();

        // These should be no-ops when recording is disabled
        recorder.record_instruction(0x1000, Some("main"));
        recorder.record_memory_write(0x2000, 0x3000, &[0], &[42]);

        assert!(recorder.session().is_none());
    }

    #[test]
    fn test_recording_stats() {
        let config = RecordingConfig::default();
        let sandbox_id = Uuid::new_v4();
        let mut session = RecordingSession::new(sandbox_id, config);

        session.add_event(ExecutionEvent::new(0, EventType::Instruction, 0x1000));
        session.add_event(ExecutionEvent::new(1, EventType::MemoryWrite, 0x1000));
        session.add_event(ExecutionEvent::new(2, EventType::WasiCall, 0x1000));
        session.add_event(ExecutionEvent::new(3, EventType::WasiReturn, 0x1000));

        let stats = session.stats();
        assert_eq!(stats.event_count, 4);
        assert_eq!(stats.memory_events, 1);
        assert_eq!(stats.wasi_events, 2);
    }
}
