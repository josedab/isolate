//! Timeline navigation for replay.

use serde::{Deserialize, Serialize};

use super::recording::{EventKind, Recording};

/// A timeline entry (summarized event for navigation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub sequence: u64,
    pub timestamp_us: u64,
    pub label: String,
    pub category: String,
}

/// A user-defined bookmark in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub sequence: u64,
    pub note: Option<String>,
}

/// Timeline providing navigation over a recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    entries: Vec<TimelineEntry>,
    bookmarks: Vec<Bookmark>,
    total_duration_us: u64,
}

/// A view into a slice of the timeline.
#[derive(Debug, Clone)]
pub struct TimelineView<'a> {
    pub entries: &'a [TimelineEntry],
    pub start_us: u64,
    pub end_us: u64,
}

impl Timeline {
    /// Build a timeline from a recording.
    pub fn from_recording(recording: &Recording) -> Self {
        let entries: Vec<TimelineEntry> = recording.events.iter().map(|e| {
            let (label, category) = Self::summarize_event(&e.kind);
            TimelineEntry {
                sequence: e.sequence,
                timestamp_us: e.timestamp_us,
                label,
                category,
            }
        }).collect();

        Self {
            total_duration_us: recording.duration_us,
            entries,
            bookmarks: Vec::new(),
        }
    }

    fn summarize_event(kind: &EventKind) -> (String, String) {
        match kind {
            EventKind::Input(d) => (format!("Input ({} bytes)", d.len()), "io".into()),
            EventKind::Output(d) => (format!("Output ({} bytes)", d.len()), "io".into()),
            EventKind::ErrorOutput(d) => (format!("Stderr ({} bytes)", d.len()), "io".into()),
            EventKind::EnvAccess { key, .. } => (format!("Env: {}", key), "env".into()),
            EventKind::Random(d) => (format!("Random ({} bytes)", d.len()), "nondeterminism".into()),
            EventKind::ClockRead(ts) => (format!("Clock: {}μs", ts), "nondeterminism".into()),
            EventKind::FileOp { path, op } => (format!("{}: {}", op, path), "fs".into()),
            EventKind::NetOp { host, port, op } => (format!("{}: {}:{}", op, host, port), "net".into()),
            EventKind::MemorySnapshot { pages, used_bytes } => (format!("Memory: {} pages, {} bytes", pages, used_bytes), "memory".into()),
            EventKind::FuelCheckpoint(fuel) => (format!("Fuel: {}", fuel), "resource".into()),
            EventKind::Exit(code) => (format!("Exit({})", code), "lifecycle".into()),
        }
    }

    /// Total number of timeline entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entry at a specific sequence number.
    pub fn at_sequence(&self, seq: u64) -> Option<&TimelineEntry> {
        self.entries.iter().find(|e| e.sequence == seq)
    }

    /// Get entries in a time range.
    pub fn range(&self, start_us: u64, end_us: u64) -> Vec<&TimelineEntry> {
        self.entries.iter()
            .filter(|e| e.timestamp_us >= start_us && e.timestamp_us <= end_us)
            .collect()
    }

    /// Get entries by category.
    pub fn by_category(&self, category: &str) -> Vec<&TimelineEntry> {
        self.entries.iter().filter(|e| e.category == category).collect()
    }

    /// Add a bookmark.
    pub fn add_bookmark(&mut self, name: impl Into<String>, sequence: u64, note: Option<String>) {
        self.bookmarks.push(Bookmark {
            name: name.into(),
            sequence,
            note,
        });
    }

    /// Get all bookmarks.
    pub fn bookmarks(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    /// Total duration in microseconds.
    pub fn duration_us(&self) -> u64 {
        self.total_duration_us
    }

    /// All entries.
    pub fn entries(&self) -> &[TimelineEntry] {
        &self.entries
    }

    /// Jump to percentage position in timeline (0.0-1.0).
    pub fn at_percent(&self, pct: f64) -> Option<&TimelineEntry> {
        if self.entries.is_empty() || pct < 0.0 || pct > 1.0 {
            return None;
        }
        let target_time = (self.total_duration_us as f64 * pct) as u64;
        self.entries.iter()
            .rev()
            .find(|e| e.timestamp_us <= target_time)
            .or(self.entries.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::recording::ExecutionRecorder;

    fn make_recording() -> Recording {
        let rec = ExecutionRecorder::new("test");
        rec.record_event(EventKind::Input(b"in".to_vec()));
        rec.record_event(EventKind::ClockRead(100));
        rec.record_event(EventKind::Output(b"out".to_vec()));
        rec.record_event(EventKind::FileOp { path: "/tmp/x".into(), op: "read".into() });
        rec.record_event(EventKind::Exit(0));
        rec.finish()
    }

    #[test]
    fn test_timeline_from_recording() {
        let recording = make_recording();
        let timeline = Timeline::from_recording(&recording);
        assert_eq!(timeline.len(), 5);
        assert!(!timeline.is_empty());
    }

    #[test]
    fn test_at_sequence() {
        let recording = make_recording();
        let timeline = Timeline::from_recording(&recording);
        let entry = timeline.at_sequence(0).unwrap();
        assert!(entry.label.contains("Input"));
        assert_eq!(entry.category, "io");
    }

    #[test]
    fn test_by_category() {
        let recording = make_recording();
        let timeline = Timeline::from_recording(&recording);
        let io_events = timeline.by_category("io");
        assert_eq!(io_events.len(), 2); // Input + Output
    }

    #[test]
    fn test_bookmarks() {
        let recording = make_recording();
        let mut timeline = Timeline::from_recording(&recording);
        timeline.add_bookmark("bug-start", 2, Some("Issue begins here".into()));
        timeline.add_bookmark("fix-applied", 3, None);

        assert_eq!(timeline.bookmarks().len(), 2);
        assert_eq!(timeline.bookmarks()[0].name, "bug-start");
    }

    #[test]
    fn test_empty_timeline() {
        let rec = ExecutionRecorder::new("empty");
        let recording = rec.finish();
        let timeline = Timeline::from_recording(&recording);
        assert!(timeline.is_empty());
        assert_eq!(timeline.len(), 0);
    }

    #[test]
    fn test_event_labels() {
        let rec = ExecutionRecorder::new("labels");
        rec.record_event(EventKind::MemorySnapshot { pages: 10, used_bytes: 65536 });
        rec.record_event(EventKind::FuelCheckpoint(500000));
        rec.record_event(EventKind::NetOp { host: "api.com".into(), port: 443, op: "connect".into() });

        let recording = rec.finish();
        let timeline = Timeline::from_recording(&recording);

        assert!(timeline.entries()[0].label.contains("Memory"));
        assert!(timeline.entries()[1].label.contains("Fuel"));
        assert!(timeline.entries()[2].label.contains("api.com"));
    }
}
