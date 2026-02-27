//! Execution recording.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Type of recorded event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventKind {
    /// Input data (stdin, arguments).
    Input(Vec<u8>),
    /// Output data (stdout).
    Output(Vec<u8>),
    /// Error output (stderr).
    ErrorOutput(Vec<u8>),
    /// Environment variable access.
    EnvAccess { key: String, value: Option<String> },
    /// Random bytes generated.
    Random(Vec<u8>),
    /// Clock/time read.
    ClockRead(u64),
    /// Filesystem operation.
    FileOp { path: String, op: String },
    /// Network operation.
    NetOp { host: String, port: u16, op: String },
    /// Memory snapshot at a point.
    MemorySnapshot { pages: u32, used_bytes: u64 },
    /// Fuel consumed at a checkpoint.
    FuelCheckpoint(u64),
    /// Exit with code.
    Exit(i32),
}

/// A single recorded event with timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingEvent {
    pub sequence: u64,
    pub timestamp_us: u64,
    pub kind: EventKind,
}

/// A complete execution recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub sandbox_id: String,
    pub events: Vec<RecordingEvent>,
    pub started_at: u64,
    pub duration_us: u64,
    pub module_hash: Option<String>,
}

impl Recording {
    /// Total data size of all recorded events (approximate).
    pub fn data_size(&self) -> usize {
        self.events
            .iter()
            .map(|e| match &e.kind {
                EventKind::Input(d)
                | EventKind::Output(d)
                | EventKind::ErrorOutput(d)
                | EventKind::Random(d) => d.len(),
                EventKind::MemorySnapshot { .. } => 16,
                _ => 32,
            })
            .sum()
    }

    /// Get events of a specific kind.
    pub fn events_of_kind(&self, predicate: impl Fn(&EventKind) -> bool) -> Vec<&RecordingEvent> {
        self.events.iter().filter(|e| predicate(&e.kind)).collect()
    }

    /// Get exit code if recorded.
    pub fn exit_code(&self) -> Option<i32> {
        self.events.iter().rev().find_map(|e| match &e.kind {
            EventKind::Exit(code) => Some(*code),
            _ => None,
        })
    }
}

/// Records execution events in real-time.
#[derive(Clone)]
pub struct ExecutionRecorder {
    inner: Arc<RecorderInner>,
}

struct RecorderInner {
    sandbox_id: String,
    events: Mutex<Vec<RecordingEvent>>,
    start_time: u64,
    sequence: Mutex<u64>,
}

impl ExecutionRecorder {
    pub fn new(sandbox_id: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        Self {
            inner: Arc::new(RecorderInner {
                sandbox_id: sandbox_id.to_string(),
                events: Mutex::new(Vec::new()),
                start_time: now,
                sequence: Mutex::new(0),
            }),
        }
    }

    /// Record an event.
    pub fn record_event(&self, kind: EventKind) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let mut seq = self.inner.sequence.lock();
        let event =
            RecordingEvent { sequence: *seq, timestamp_us: now - self.inner.start_time, kind };
        *seq += 1;
        drop(seq);

        self.inner.events.lock().push(event);
    }

    /// Finish recording and produce a Recording.
    pub fn finish(&self) -> Recording {
        let events = self.inner.events.lock().clone();
        let duration = events.last().map_or(0, |e| e.timestamp_us);

        Recording {
            sandbox_id: self.inner.sandbox_id.clone(),
            events,
            started_at: self.inner.start_time,
            duration_us: duration,
            module_hash: None,
        }
    }

    /// Current event count.
    pub fn event_count(&self) -> usize {
        self.inner.events.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_events() {
        let rec = ExecutionRecorder::new("test-sb");
        rec.record_event(EventKind::Input(b"hello".to_vec()));
        rec.record_event(EventKind::Output(b"world".to_vec()));
        rec.record_event(EventKind::Exit(0));

        assert_eq!(rec.event_count(), 3);

        let recording = rec.finish();
        assert_eq!(recording.sandbox_id, "test-sb");
        assert_eq!(recording.events.len(), 3);
        assert_eq!(recording.exit_code(), Some(0));
    }

    #[test]
    fn test_sequence_numbers() {
        let rec = ExecutionRecorder::new("seq-test");
        rec.record_event(EventKind::ClockRead(100));
        rec.record_event(EventKind::ClockRead(200));
        rec.record_event(EventKind::ClockRead(300));

        let recording = rec.finish();
        assert_eq!(recording.events[0].sequence, 0);
        assert_eq!(recording.events[1].sequence, 1);
        assert_eq!(recording.events[2].sequence, 2);
    }

    #[test]
    fn test_data_size() {
        let rec = ExecutionRecorder::new("size-test");
        rec.record_event(EventKind::Input(vec![0u8; 1000]));
        rec.record_event(EventKind::Output(vec![0u8; 2000]));

        let recording = rec.finish();
        assert_eq!(recording.data_size(), 3000);
    }

    #[test]
    fn test_events_of_kind() {
        let rec = ExecutionRecorder::new("filter");
        rec.record_event(EventKind::Input(b"a".to_vec()));
        rec.record_event(EventKind::Output(b"b".to_vec()));
        rec.record_event(EventKind::Input(b"c".to_vec()));

        let recording = rec.finish();
        let inputs = recording.events_of_kind(|k| matches!(k, EventKind::Input(_)));
        assert_eq!(inputs.len(), 2);
    }

    #[test]
    fn test_empty_recording() {
        let rec = ExecutionRecorder::new("empty");
        let recording = rec.finish();
        assert_eq!(recording.events.len(), 0);
        assert_eq!(recording.exit_code(), None);
        assert_eq!(recording.data_size(), 0);
    }

    #[test]
    fn test_various_event_kinds() {
        let rec = ExecutionRecorder::new("varied");
        rec.record_event(EventKind::EnvAccess { key: "HOME".into(), value: Some("/home".into()) });
        rec.record_event(EventKind::Random(vec![1, 2, 3, 4]));
        rec.record_event(EventKind::FileOp { path: "/tmp/f".into(), op: "read".into() });
        rec.record_event(EventKind::NetOp {
            host: "example.com".into(),
            port: 443,
            op: "connect".into(),
        });
        rec.record_event(EventKind::MemorySnapshot { pages: 10, used_bytes: 65536 });
        rec.record_event(EventKind::FuelCheckpoint(500000));

        let recording = rec.finish();
        assert_eq!(recording.events.len(), 6);
    }
}
