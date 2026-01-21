//! Sandbox Replay & Debugging.
//!
//! Record sandbox executions for deterministic replay, time-travel
//! debugging, and session sharing.
//!
//! # Features
//!
//! - **Execution Recording**: Capture all non-deterministic inputs
//! - **Deterministic Replay**: Replay with identical behavior
//! - **Timeline**: Navigate to any point in execution
//! - **Session Sharing**: Share replay sessions via tokens

#![allow(dead_code)]

pub mod recording;
pub mod session;
pub mod timeline;

pub use recording::{ExecutionRecorder, Recording, RecordingEvent, EventKind};
pub use session::{ReplaySession, SessionManager, SessionToken, ShareSettings};
pub use timeline::{Timeline, TimelineEntry, TimelineView, Bookmark};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_replay_flow() {
        let recorder = ExecutionRecorder::new("sandbox-1");
        recorder.record_event(EventKind::Input(b"hello".to_vec()));
        recorder.record_event(EventKind::Output(b"world".to_vec()));
        recorder.record_event(EventKind::Exit(0));

        let recording = recorder.finish();
        assert_eq!(recording.events.len(), 3);

        let timeline = Timeline::from_recording(&recording);
        assert_eq!(timeline.len(), 3);

        let manager = SessionManager::new();
        let token = manager.create_session(recording, ShareSettings::default());
        assert!(manager.get_session(&token).is_some());
    }
}
