//! Async I/O layer for WASI 0.3 preparation.
//!
//! This module provides the foundation for WASI 0.3's native async I/O model
//! using streams and pollables. It bridges the gap between Tokio's async runtime
//! and WASM component async primitives.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Unique identifier for an async stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(u64);

/// Unique identifier for a pollable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PollableId(u64);

/// Global ID counter for streams and pollables.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Readiness state of a pollable resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Readiness {
    /// Resource is not ready, would block.
    NotReady,
    /// Resource is ready for the operation.
    Ready,
    /// Resource has encountered an error.
    Error,
    /// Resource has been closed.
    Closed,
}

/// An async input stream that components can read from.
pub struct AsyncInputStream {
    id: StreamId,
    buffer: Arc<Mutex<Vec<u8>>>,
    position: usize,
    closed: bool,
    bytes_read: u64,
    read_limit: Option<u64>,
}

impl AsyncInputStream {
    /// Create a new input stream with the given data.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            id: StreamId(next_id()),
            buffer: Arc::new(Mutex::new(data)),
            position: 0,
            closed: false,
            bytes_read: 0,
            read_limit: None,
        }
    }

    /// Create an empty input stream.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Set a read limit.
    pub fn with_read_limit(mut self, limit: u64) -> Self {
        self.read_limit = Some(limit);
        self
    }

    /// Get the stream ID.
    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Check readiness for reading.
    pub fn readiness(&self) -> Readiness {
        if self.closed {
            return Readiness::Closed;
        }
        let buffer = self.buffer.lock();
        if self.position < buffer.len() {
            Readiness::Ready
        } else {
            Readiness::Closed
        }
    }

    /// Read up to `max_bytes` from the stream.
    pub fn read(&mut self, max_bytes: usize) -> Result<Vec<u8>, StreamError> {
        if self.closed {
            return Err(StreamError::Closed);
        }

        let buffer = self.buffer.lock();
        let available = buffer.len().saturating_sub(self.position);
        if available == 0 {
            self.closed = true;
            return Ok(Vec::new());
        }

        let to_read = max_bytes.min(available);

        // Check read limit
        if let Some(limit) = self.read_limit {
            if self.bytes_read + to_read as u64 > limit {
                return Err(StreamError::LimitExceeded);
            }
        }

        let data = buffer[self.position..self.position + to_read].to_vec();
        self.position += to_read;
        self.bytes_read += to_read as u64;

        Ok(data)
    }

    /// Get total bytes read.
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }
}

/// An async output stream that components can write to.
pub struct AsyncOutputStream {
    id: StreamId,
    buffer: Arc<Mutex<Vec<u8>>>,
    closed: bool,
    bytes_written: u64,
    write_limit: Option<u64>,
}

impl AsyncOutputStream {
    /// Create a new output stream.
    pub fn new() -> Self {
        Self {
            id: StreamId(next_id()),
            buffer: Arc::new(Mutex::new(Vec::new())),
            closed: false,
            bytes_written: 0,
            write_limit: None,
        }
    }

    /// Set a write limit.
    pub fn with_write_limit(mut self, limit: u64) -> Self {
        self.write_limit = Some(limit);
        self
    }

    /// Get the stream ID.
    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Check readiness for writing.
    pub fn readiness(&self) -> Readiness {
        if self.closed {
            return Readiness::Closed;
        }
        if let Some(limit) = self.write_limit {
            if self.bytes_written >= limit {
                return Readiness::Error;
            }
        }
        Readiness::Ready
    }

    /// Write data to the stream.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, StreamError> {
        if self.closed {
            return Err(StreamError::Closed);
        }

        // Check write limit
        if let Some(limit) = self.write_limit {
            if self.bytes_written + data.len() as u64 > limit {
                return Err(StreamError::LimitExceeded);
            }
        }

        let mut buffer = self.buffer.lock();
        buffer.extend_from_slice(data);
        self.bytes_written += data.len() as u64;

        Ok(data.len())
    }

    /// Close the stream.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Get the captured output.
    pub fn contents(&self) -> Vec<u8> {
        self.buffer.lock().clone()
    }

    /// Get total bytes written.
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

impl Default for AsyncOutputStream {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for stream operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    /// Stream has been closed.
    Closed,
    /// I/O limit exceeded.
    LimitExceeded,
    /// Operation would block.
    WouldBlock,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::Closed => write!(f, "stream closed"),
            StreamError::LimitExceeded => write!(f, "I/O limit exceeded"),
            StreamError::WouldBlock => write!(f, "operation would block"),
        }
    }
}

impl std::error::Error for StreamError {}

/// A pollable resource that can be waited on.
pub struct Pollable {
    id: PollableId,
    kind: PollableKind,
    ready: bool,
}

/// The kind of pollable resource.
pub enum PollableKind {
    /// An input stream ready for reading.
    InputStream(StreamId),
    /// An output stream ready for writing.
    OutputStream(StreamId),
    /// A timer that fires after a duration.
    Timer { deadline: Instant },
    /// An always-ready pollable (for immediate completion).
    Immediate,
}

impl Pollable {
    /// Create a pollable for an input stream.
    pub fn for_input(stream_id: StreamId) -> Self {
        Self { id: PollableId(next_id()), kind: PollableKind::InputStream(stream_id), ready: false }
    }

    /// Create a pollable for an output stream.
    pub fn for_output(stream_id: StreamId) -> Self {
        Self {
            id: PollableId(next_id()),
            kind: PollableKind::OutputStream(stream_id),
            ready: false,
        }
    }

    /// Create a timer pollable.
    pub fn timer(duration: Duration) -> Self {
        Self {
            id: PollableId(next_id()),
            kind: PollableKind::Timer { deadline: Instant::now() + duration },
            ready: false,
        }
    }

    /// Create an immediately-ready pollable.
    pub fn immediate() -> Self {
        Self { id: PollableId(next_id()), kind: PollableKind::Immediate, ready: true }
    }

    /// Get the pollable ID.
    pub fn id(&self) -> PollableId {
        self.id
    }

    /// Check if the pollable is ready.
    pub fn is_ready(&self) -> bool {
        match &self.kind {
            PollableKind::Immediate => true,
            PollableKind::Timer { deadline } => Instant::now() >= *deadline,
            _ => self.ready,
        }
    }

    /// Set the readiness state.
    pub fn set_ready(&mut self, ready: bool) {
        self.ready = ready;
    }
}

/// Poll set for waiting on multiple pollable resources.
pub struct PollSet {
    pollables: HashMap<PollableId, Pollable>,
}

impl PollSet {
    /// Create a new poll set.
    pub fn new() -> Self {
        Self { pollables: HashMap::new() }
    }

    /// Add a pollable to the set.
    pub fn add(&mut self, pollable: Pollable) -> PollableId {
        let id = pollable.id();
        self.pollables.insert(id, pollable);
        id
    }

    /// Remove a pollable from the set.
    pub fn remove(&mut self, id: PollableId) -> Option<Pollable> {
        self.pollables.remove(&id)
    }

    /// Poll all resources and return IDs of ready ones.
    pub fn poll(&self) -> Vec<PollableId> {
        self.pollables.iter().filter(|(_, p)| p.is_ready()).map(|(id, _)| *id).collect()
    }

    /// Poll with a timeout. Returns ready pollable IDs.
    pub fn poll_timeout(&self, timeout: Duration) -> Vec<PollableId> {
        let deadline = Instant::now() + timeout;

        loop {
            let ready = self.poll();
            if !ready.is_empty() || Instant::now() >= deadline {
                return ready;
            }
            // Sleep briefly to avoid busy-waiting
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Get the number of pollables in the set.
    pub fn len(&self) -> usize {
        self.pollables.len()
    }

    /// Check if the poll set is empty.
    pub fn is_empty(&self) -> bool {
        self.pollables.is_empty()
    }
}

impl Default for PollSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_stream() {
        let mut stream = AsyncInputStream::new(b"hello world".to_vec());
        assert_eq!(stream.readiness(), Readiness::Ready);

        let data = stream.read(5).unwrap();
        assert_eq!(data, b"hello");

        let data = stream.read(10).unwrap();
        assert_eq!(data, b" world");

        assert_eq!(stream.bytes_read(), 11);
    }

    #[test]
    fn test_input_stream_with_limit() {
        let mut stream = AsyncInputStream::new(b"hello world".to_vec()).with_read_limit(5);
        let data = stream.read(5).unwrap();
        assert_eq!(data, b"hello");

        let result = stream.read(5);
        assert_eq!(result, Err(StreamError::LimitExceeded));
    }

    #[test]
    fn test_output_stream() {
        let mut stream = AsyncOutputStream::new();
        assert_eq!(stream.readiness(), Readiness::Ready);

        stream.write(b"hello ").unwrap();
        stream.write(b"world").unwrap();

        assert_eq!(stream.contents(), b"hello world");
        assert_eq!(stream.bytes_written(), 11);
    }

    #[test]
    fn test_output_stream_with_limit() {
        let mut stream = AsyncOutputStream::new().with_write_limit(5);
        stream.write(b"hello").unwrap();

        let result = stream.write(b" world");
        assert_eq!(result, Err(StreamError::LimitExceeded));
    }

    #[test]
    fn test_pollable_timer() {
        let pollable = Pollable::timer(Duration::from_millis(1));
        assert!(!pollable.is_ready());

        std::thread::sleep(Duration::from_millis(5));
        assert!(pollable.is_ready());
    }

    #[test]
    fn test_pollable_immediate() {
        let pollable = Pollable::immediate();
        assert!(pollable.is_ready());
    }

    #[test]
    fn test_poll_set() {
        let mut poll_set = PollSet::new();

        let id1 = poll_set.add(Pollable::immediate());
        let _id2 = poll_set.add(Pollable::timer(Duration::from_secs(60)));

        let ready = poll_set.poll();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], id1);
    }

    #[test]
    fn test_empty_input_stream() {
        let mut stream = AsyncInputStream::empty();
        let data = stream.read(10).unwrap();
        assert!(data.is_empty());
        assert_eq!(stream.readiness(), Readiness::Closed);
    }
}
