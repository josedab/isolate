//! Execution event types for time-travel debugging.

use super::EventId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Type of execution event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// Instruction executed.
    Instruction,
    /// Function call.
    FunctionCall,
    /// Function return.
    FunctionReturn,
    /// Memory read.
    MemoryRead,
    /// Memory write.
    MemoryWrite,
    /// WASI call.
    WasiCall,
    /// WASI return.
    WasiReturn,
    /// Breakpoint hit.
    Breakpoint,
    /// Exception occurred.
    Exception,
    /// Execution started.
    Start,
    /// Execution paused.
    Pause,
    /// Execution resumed.
    Resume,
    /// Execution ended.
    End,
}

impl EventType {
    /// Check if this event type represents a control flow change.
    pub fn is_control_flow(&self) -> bool {
        matches!(
            self,
            EventType::FunctionCall
                | EventType::FunctionReturn
                | EventType::Breakpoint
                | EventType::Exception
        )
    }

    /// Check if this event type represents a state change.
    pub fn is_state_change(&self) -> bool {
        matches!(self, EventType::MemoryWrite | EventType::WasiCall)
    }
}

/// A memory change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChange {
    /// Memory address.
    pub address: u64,
    /// Size of the change in bytes.
    pub size: u32,
    /// Old value (before the change).
    pub old_value: Vec<u8>,
    /// New value (after the change).
    pub new_value: Vec<u8>,
}

impl MemoryChange {
    /// Create a new memory change.
    pub fn new(address: u64, old_value: Vec<u8>, new_value: Vec<u8>) -> Self {
        let size = new_value.len() as u32;
        Self { address, size, old_value, new_value }
    }

    /// Create a memory write event (no old value known).
    pub fn write(address: u64, value: Vec<u8>) -> Self {
        let size = value.len() as u32;
        Self { address, size, old_value: Vec::new(), new_value: value }
    }

    /// Create a memory read event.
    pub fn read(address: u64, value: Vec<u8>) -> Self {
        let size = value.len() as u32;
        Self { address, size, old_value: value.clone(), new_value: value }
    }
}

/// A register change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterChange {
    /// Register name.
    pub name: String,
    /// Old value.
    pub old_value: u64,
    /// New value.
    pub new_value: u64,
}

impl RegisterChange {
    /// Create a new register change.
    pub fn new(name: impl Into<String>, old_value: u64, new_value: u64) -> Self {
        Self { name: name.into(), old_value, new_value }
    }
}

/// WASI call information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasiCallInfo {
    /// WASI function name.
    pub function: String,
    /// Arguments (as strings for display).
    pub arguments: Vec<String>,
    /// Return value (if available).
    pub return_value: Option<String>,
    /// Error code (if error occurred).
    pub error_code: Option<u32>,
}

impl WasiCallInfo {
    /// Create a new WASI call info.
    pub fn new(function: impl Into<String>, arguments: Vec<String>) -> Self {
        Self { function: function.into(), arguments, return_value: None, error_code: None }
    }

    /// Set the return value.
    pub fn with_return(mut self, value: impl Into<String>) -> Self {
        self.return_value = Some(value.into());
        self
    }

    /// Set the error code.
    pub fn with_error(mut self, code: u32) -> Self {
        self.error_code = Some(code);
        self
    }
}

/// An execution event in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    /// Unique event ID.
    pub id: EventId,
    /// Event type.
    pub event_type: EventType,
    /// Timestamp when the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Instruction pointer / program counter.
    pub instruction_pointer: u64,
    /// Stack depth at the time of event.
    pub stack_depth: u32,
    /// Fuel consumed up to this point.
    pub fuel_consumed: u64,
    /// Function name (if in a function).
    pub function_name: Option<String>,
    /// Source location (if available).
    pub source_location: Option<SourceLocation>,
    /// Memory changes associated with this event.
    pub memory_changes: Vec<MemoryChange>,
    /// Register changes associated with this event.
    pub register_changes: Vec<RegisterChange>,
    /// WASI call info (if applicable).
    pub wasi_call: Option<WasiCallInfo>,
    /// Associated data (event-specific).
    pub data: Option<Vec<u8>>,
}

/// Source code location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// File name.
    pub file: String,
    /// Line number.
    pub line: u32,
    /// Column number.
    pub column: Option<u32>,
}

impl SourceLocation {
    /// Create a new source location.
    pub fn new(file: impl Into<String>, line: u32) -> Self {
        Self { file: file.into(), line, column: None }
    }

    /// Create with column.
    pub fn with_column(file: impl Into<String>, line: u32, column: u32) -> Self {
        Self { file: file.into(), line, column: Some(column) }
    }
}

impl ExecutionEvent {
    /// Create a new execution event.
    pub fn new(id: EventId, event_type: EventType, instruction_pointer: u64) -> Self {
        Self {
            id,
            event_type,
            timestamp: Utc::now(),
            instruction_pointer,
            stack_depth: 0,
            fuel_consumed: 0,
            function_name: None,
            source_location: None,
            memory_changes: Vec::new(),
            register_changes: Vec::new(),
            wasi_call: None,
            data: None,
        }
    }

    /// Create a start event.
    pub fn start() -> Self {
        Self::new(0, EventType::Start, 0)
    }

    /// Create an end event.
    pub fn end(id: EventId, fuel_consumed: u64) -> Self {
        let mut event = Self::new(id, EventType::End, 0);
        event.fuel_consumed = fuel_consumed;
        event
    }

    /// Set the function name.
    pub fn with_function(mut self, name: impl Into<String>) -> Self {
        self.function_name = Some(name.into());
        self
    }

    /// Set the source location.
    pub fn with_source(mut self, location: SourceLocation) -> Self {
        self.source_location = Some(location);
        self
    }

    /// Set the stack depth.
    pub fn with_stack_depth(mut self, depth: u32) -> Self {
        self.stack_depth = depth;
        self
    }

    /// Add a memory change.
    pub fn with_memory_change(mut self, change: MemoryChange) -> Self {
        self.memory_changes.push(change);
        self
    }

    /// Add a register change.
    pub fn with_register_change(mut self, change: RegisterChange) -> Self {
        self.register_changes.push(change);
        self
    }

    /// Set WASI call info.
    pub fn with_wasi_call(mut self, call: WasiCallInfo) -> Self {
        self.wasi_call = Some(call);
        self
    }

    /// Check if this event has any state changes.
    pub fn has_state_changes(&self) -> bool {
        !self.memory_changes.is_empty() || !self.register_changes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_control_flow() {
        assert!(EventType::FunctionCall.is_control_flow());
        assert!(EventType::FunctionReturn.is_control_flow());
        assert!(!EventType::MemoryWrite.is_control_flow());
    }

    #[test]
    fn test_event_type_state_change() {
        assert!(EventType::MemoryWrite.is_state_change());
        assert!(EventType::WasiCall.is_state_change());
        assert!(!EventType::FunctionCall.is_state_change());
    }

    #[test]
    fn test_memory_change() {
        let change = MemoryChange::new(0x1000, vec![0x00], vec![0x42]);
        assert_eq!(change.address, 0x1000);
        assert_eq!(change.size, 1);
        assert_eq!(change.old_value, vec![0x00]);
        assert_eq!(change.new_value, vec![0x42]);
    }

    #[test]
    fn test_register_change() {
        let change = RegisterChange::new("rax", 0, 42);
        assert_eq!(change.name, "rax");
        assert_eq!(change.old_value, 0);
        assert_eq!(change.new_value, 42);
    }

    #[test]
    fn test_wasi_call_info() {
        let call = WasiCallInfo::new("fd_write", vec!["1".to_string(), "hello".to_string()])
            .with_return("5")
            .with_error(0);

        assert_eq!(call.function, "fd_write");
        assert_eq!(call.return_value, Some("5".to_string()));
        assert_eq!(call.error_code, Some(0));
    }

    #[test]
    fn test_execution_event() {
        let event = ExecutionEvent::new(1, EventType::Instruction, 0x1000)
            .with_function("main")
            .with_stack_depth(1)
            .with_memory_change(MemoryChange::write(0x2000, vec![0x42]));

        assert_eq!(event.id, 1);
        assert_eq!(event.instruction_pointer, 0x1000);
        assert_eq!(event.function_name, Some("main".to_string()));
        assert!(event.has_state_changes());
    }

    #[test]
    fn test_source_location() {
        let loc = SourceLocation::with_column("main.rs", 42, 10);
        assert_eq!(loc.file, "main.rs");
        assert_eq!(loc.line, 42);
        assert_eq!(loc.column, Some(10));
    }
}
