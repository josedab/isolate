//! Live debugging support for sandboxes.
//!
//! This module provides debugging capabilities for running sandboxes,
//! including breakpoints, stepping, and inspection.
//!
//! # Features
//!
//! - **Breakpoints**: Set and manage execution breakpoints
//! - **Stepping**: Step through code execution
//! - **Inspection**: Inspect variables, memory, and call stack
//! - **Watch Expressions**: Monitor values during execution
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::debug::{DebugSession, Breakpoint};
//!
//! let mut session = DebugSession::new(sandbox_id);
//!
//! // Set a breakpoint at a function
//! session.set_breakpoint(Breakpoint::function("_start"))?;
//!
//! // When hit, inspect state
//! if let Some(state) = session.current_state() {
//!     println!("Call stack: {:?}", state.call_stack);
//!     println!("Locals: {:?}", state.locals);
//! }
//!
//! // Continue execution
//! session.continue_execution()?;
//! ```

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

mod breakpoint;
pub mod dap;
pub mod flamegraph;
mod inspector;
pub mod profiler;
mod session;
pub mod timetravel;

pub use breakpoint::{Breakpoint, BreakpointCondition, BreakpointId, BreakpointType};
pub use dap::{
    DapCommand, DapEvent, DapMessage, DapRequest, DapResponse, DapServer, DashboardSummary,
    ResourceDashboard, ResourceDataPoint,
};
pub use flamegraph::{FlameGraphBuilder, FlameGraphOptions, FlameGraphSummary, FlameNode};
pub use inspector::{Inspector, MemoryView, StackFrame, Variable, VariableType};
pub use profiler::{
    ExecutionProfile, FunctionProfile, ProfileEvent, ProfileSession, SandboxProfiler,
};
pub use session::{DebugCommand, DebugError, DebugEvent, DebugSession, DebugState};
pub use timetravel::{
    EventType, ExecutionEvent, Recorder, RecordingConfig, RecordingSession, SnapshotManager,
    StateSnapshot, StepResult, TimeTravelConfig, Timeline, TimelineNavigation,
};

/// Default maximum number of breakpoints per session.
pub const MAX_BREAKPOINTS: usize = 100;

/// Default maximum stack depth to capture.
pub const MAX_STACK_DEPTH: usize = 64;

/// Default maximum memory view size.
pub const MAX_MEMORY_VIEW: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify exports compile
        let bp = Breakpoint::function("_start");
        assert_eq!(bp.name, Some("_start".to_string()));
    }
}
