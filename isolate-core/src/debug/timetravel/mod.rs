//! Time-travel debugging for WebAssembly sandboxes.
//!
//! This module provides recording and replay capabilities for sandbox execution,
//! enabling developers to step backwards through execution history.
//!
//! # Features
//!
//! - **Execution Recording**: Capture all inputs, outputs, and state changes
//! - **Backwards Stepping**: Step backwards through execution history
//! - **State Snapshots**: Periodic snapshots for efficient state restoration
//! - **Event Timeline**: Query and navigate execution events
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::debug::timetravel::{Recorder, Timeline};
//!
//! # async fn example() -> isolate_core::Result<()> {
//! // Create a recorder
//! let recorder = Recorder::new();
//!
//! // Record execution
//! recorder.start_recording(sandbox_id);
//! // ... execute sandbox ...
//! recorder.stop_recording();
//!
//! // Create timeline from recording
//! let timeline = recorder.build_timeline()?;
//!
//! // Navigate execution history
//! timeline.goto_step(100)?;  // Go to step 100
//! timeline.step_back()?;     // Step backwards
//! timeline.step_forward()?;  // Step forward
//! # Ok(())
//! # }
//! ```

mod event;
mod recorder;
mod snapshot;
mod timeline;

pub use event::{EventType, ExecutionEvent, MemoryChange, RegisterChange};
pub use recorder::{Recorder, RecordingConfig, RecordingSession};
pub use snapshot::{SnapshotManager, StateSnapshot};
pub use timeline::{StepResult, Timeline, TimelineNavigation};

use uuid::Uuid;

/// Unique identifier for a recording.
pub type RecordingId = Uuid;

/// Unique identifier for an event in the timeline.
pub type EventId = u64;

/// Configuration for time-travel debugging.
#[derive(Debug, Clone)]
pub struct TimeTravelConfig {
    /// Enable time-travel recording.
    pub enabled: bool,
    /// Maximum number of events to record.
    pub max_events: usize,
    /// Snapshot interval (take snapshot every N instructions).
    pub snapshot_interval: u64,
    /// Maximum number of snapshots to keep.
    pub max_snapshots: usize,
    /// Record memory changes.
    pub record_memory: bool,
    /// Record register changes.
    pub record_registers: bool,
    /// Record WASI calls.
    pub record_wasi_calls: bool,
}

impl Default for TimeTravelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_events: 1_000_000,
            snapshot_interval: 10_000,
            max_snapshots: 100,
            record_memory: true,
            record_registers: true,
            record_wasi_calls: true,
        }
    }
}

impl TimeTravelConfig {
    /// Create a new configuration with time-travel enabled.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Create a minimal configuration for low overhead.
    pub fn minimal() -> Self {
        Self {
            enabled: true,
            max_events: 100_000,
            snapshot_interval: 50_000,
            max_snapshots: 20,
            record_memory: false,
            record_registers: false,
            record_wasi_calls: true,
        }
    }

    /// Create a full configuration for detailed debugging.
    pub fn full() -> Self {
        Self {
            enabled: true,
            max_events: 10_000_000,
            snapshot_interval: 1_000,
            max_snapshots: 1000,
            record_memory: true,
            record_registers: true,
            record_wasi_calls: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TimeTravelConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_events, 1_000_000);
    }

    #[test]
    fn test_config_enabled() {
        let config = TimeTravelConfig::enabled();
        assert!(config.enabled);
    }

    #[test]
    fn test_config_minimal() {
        let config = TimeTravelConfig::minimal();
        assert!(config.enabled);
        assert!(!config.record_memory);
        assert!(!config.record_registers);
    }

    #[test]
    fn test_config_full() {
        let config = TimeTravelConfig::full();
        assert!(config.enabled);
        assert!(config.record_memory);
        assert!(config.record_registers);
        assert_eq!(config.snapshot_interval, 1_000);
    }
}
