//! Cross-node output streaming for distributed sandbox execution.
//!
//! Enables real-time streaming of sandbox stdout/stderr across mesh nodes,
//! allowing clients connected to any node to receive output from sandboxes
//! running on remote nodes.

use super::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A chunk of output from a sandbox, streamed across nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputChunk {
    /// Sandbox execution ID.
    pub execution_id: Uuid,
    /// Source node where the sandbox is running.
    pub source_node: NodeId,
    /// Sequence number for ordering.
    pub sequence: u64,
    /// Output stream type.
    pub stream: OutputStream,
    /// Output data.
    pub data: Vec<u8>,
    /// Timestamp when this chunk was produced.
    pub timestamp_ms: u64,
    /// Whether this is the final chunk.
    pub is_final: bool,
}

/// Which output stream a chunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Configuration for cross-node output streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Maximum chunk size in bytes.
    pub max_chunk_size: usize,
    /// Buffer size per stream.
    pub buffer_size: usize,
    /// Flush interval (how often to send partial chunks).
    pub flush_interval: Duration,
    /// Maximum streams per node.
    pub max_streams_per_node: usize,
    /// Enable compression for chunks above this size.
    pub compression_threshold: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_chunk_size: 64 * 1024, // 64KB
            buffer_size: 256 * 1024,   // 256KB
            flush_interval: Duration::from_millis(100),
            max_streams_per_node: 1000,
            compression_threshold: 4096,
        }
    }
}

/// Manages cross-node output streams for sandbox executions.
pub struct OutputStreamManager {
    config: StreamConfig,
    #[allow(dead_code)]
    local_node: NodeId,
    /// Active streams indexed by execution ID.
    streams: HashMap<Uuid, StreamState>,
    /// Statistics.
    stats: StreamStats,
}

/// State of an active output stream.
#[derive(Debug, Clone)]
struct StreamState {
    #[allow(dead_code)]
    execution_id: Uuid,
    source_node: NodeId,
    stdout_buffer: Vec<u8>,
    stderr_buffer: Vec<u8>,
    sequence: u64,
    started_at: Instant,
    total_bytes: u64,
    chunk_count: u64,
    completed: bool,
}

/// Statistics for the stream manager.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamStats {
    /// Total active streams.
    pub active_streams: usize,
    /// Total chunks sent.
    pub chunks_sent: u64,
    /// Total chunks received from remote nodes.
    pub chunks_received: u64,
    /// Total bytes streamed.
    pub bytes_streamed: u64,
    /// Number of completed streams.
    pub completed_streams: u64,
    /// Average latency per chunk in microseconds.
    pub avg_chunk_latency_us: u64,
}

impl OutputStreamManager {
    /// Create a new output stream manager.
    pub fn new(local_node: NodeId, config: StreamConfig) -> Self {
        Self { config, local_node, streams: HashMap::new(), stats: StreamStats::default() }
    }

    /// Start a new output stream for a sandbox execution.
    pub fn start_stream(&mut self, execution_id: Uuid, source_node: NodeId) -> bool {
        if self.streams.len() >= self.config.max_streams_per_node {
            return false;
        }

        self.streams.insert(
            execution_id,
            StreamState {
                execution_id,
                source_node,
                stdout_buffer: Vec::new(),
                stderr_buffer: Vec::new(),
                sequence: 0,
                started_at: Instant::now(),
                total_bytes: 0,
                chunk_count: 0,
                completed: false,
            },
        );

        self.stats.active_streams = self.streams.len();
        true
    }

    /// Write data to a stream's buffer.
    pub fn write(
        &mut self,
        execution_id: &Uuid,
        stream: OutputStream,
        data: &[u8],
    ) -> Vec<OutputChunk> {
        let mut chunks = Vec::new();
        let Some(state) = self.streams.get_mut(execution_id) else {
            return chunks;
        };

        let buffer = match stream {
            OutputStream::Stdout => &mut state.stdout_buffer,
            OutputStream::Stderr => &mut state.stderr_buffer,
        };

        buffer.extend_from_slice(data);
        state.total_bytes += data.len() as u64;

        // Flush chunks if buffer exceeds max chunk size
        while buffer.len() >= self.config.max_chunk_size {
            let chunk_data: Vec<u8> = buffer.drain(..self.config.max_chunk_size).collect();
            state.sequence += 1;
            state.chunk_count += 1;

            chunks.push(OutputChunk {
                execution_id: *execution_id,
                source_node: state.source_node,
                sequence: state.sequence,
                stream,
                data: chunk_data,
                timestamp_ms: state.started_at.elapsed().as_millis() as u64,
                is_final: false,
            });
        }

        self.stats.chunks_sent += chunks.len() as u64;
        self.stats.bytes_streamed += data.len() as u64;
        chunks
    }

    /// Flush remaining buffered data and mark stream as complete.
    pub fn finish_stream(&mut self, execution_id: &Uuid) -> Vec<OutputChunk> {
        let mut chunks = Vec::new();
        let Some(state) = self.streams.get_mut(execution_id) else {
            return chunks;
        };

        // Flush remaining stdout
        if !state.stdout_buffer.is_empty() {
            state.sequence += 1;
            let data = std::mem::take(&mut state.stdout_buffer);
            chunks.push(OutputChunk {
                execution_id: *execution_id,
                source_node: state.source_node,
                sequence: state.sequence,
                stream: OutputStream::Stdout,
                data,
                timestamp_ms: state.started_at.elapsed().as_millis() as u64,
                is_final: false,
            });
        }

        // Flush remaining stderr
        if !state.stderr_buffer.is_empty() {
            state.sequence += 1;
            let data = std::mem::take(&mut state.stderr_buffer);
            chunks.push(OutputChunk {
                execution_id: *execution_id,
                source_node: state.source_node,
                sequence: state.sequence,
                stream: OutputStream::Stderr,
                data,
                timestamp_ms: state.started_at.elapsed().as_millis() as u64,
                is_final: false,
            });
        }

        // Send final marker
        state.sequence += 1;
        chunks.push(OutputChunk {
            execution_id: *execution_id,
            source_node: state.source_node,
            sequence: state.sequence,
            stream: OutputStream::Stdout,
            data: Vec::new(),
            timestamp_ms: state.started_at.elapsed().as_millis() as u64,
            is_final: true,
        });

        state.completed = true;
        self.stats.completed_streams += 1;
        self.stats.chunks_sent += chunks.len() as u64;
        self.stats.active_streams = self.streams.values().filter(|s| !s.completed).count();

        chunks
    }

    /// Receive a chunk from a remote node.
    pub fn receive_chunk(&mut self, chunk: &OutputChunk) {
        self.stats.chunks_received += 1;
        self.stats.bytes_streamed += chunk.data.len() as u64;
    }

    /// Remove completed streams older than the given duration.
    pub fn cleanup(&mut self, max_age: Duration) {
        self.streams.retain(|_, state| !state.completed || state.started_at.elapsed() < max_age);
        self.stats.active_streams = self.streams.values().filter(|s| !s.completed).count();
    }

    /// Get stream statistics.
    pub fn stats(&self) -> &StreamStats {
        &self.stats
    }

    /// Get the number of active streams.
    pub fn active_stream_count(&self) -> usize {
        self.streams.values().filter(|s| !s.completed).count()
    }

    /// Check if a stream exists.
    pub fn has_stream(&self, execution_id: &Uuid) -> bool {
        self.streams.contains_key(execution_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node() -> NodeId {
        NodeId::new(1)
    }

    #[test]
    fn test_stream_lifecycle() {
        let mut manager = OutputStreamManager::new(test_node(), StreamConfig::default());
        let exec_id = Uuid::new_v4();

        assert!(manager.start_stream(exec_id, test_node()));
        assert!(manager.has_stream(&exec_id));
        assert_eq!(manager.active_stream_count(), 1);

        let chunks = manager.write(&exec_id, OutputStream::Stdout, b"hello world");
        // Small data doesn't produce chunks (buffered)
        assert!(chunks.is_empty());

        let final_chunks = manager.finish_stream(&exec_id);
        assert!(!final_chunks.is_empty());
        assert!(final_chunks.last().unwrap().is_final);
        assert_eq!(manager.stats().completed_streams, 1);
    }

    #[test]
    fn test_stream_chunking() {
        let config = StreamConfig { max_chunk_size: 10, ..Default::default() };
        let mut manager = OutputStreamManager::new(test_node(), config);
        let exec_id = Uuid::new_v4();

        manager.start_stream(exec_id, test_node());

        // Write more than chunk size
        let chunks =
            manager.write(&exec_id, OutputStream::Stdout, b"hello world this is a long message");
        assert!(!chunks.is_empty());

        for chunk in &chunks {
            assert_eq!(chunk.data.len(), 10);
            assert_eq!(chunk.stream, OutputStream::Stdout);
            assert!(!chunk.is_final);
        }
    }

    #[test]
    fn test_stream_max_limit() {
        let config = StreamConfig { max_streams_per_node: 2, ..Default::default() };
        let mut manager = OutputStreamManager::new(test_node(), config);

        assert!(manager.start_stream(Uuid::new_v4(), test_node()));
        assert!(manager.start_stream(Uuid::new_v4(), test_node()));
        assert!(!manager.start_stream(Uuid::new_v4(), test_node()));
    }

    #[test]
    fn test_receive_chunk() {
        let mut manager = OutputStreamManager::new(test_node(), StreamConfig::default());
        let chunk = OutputChunk {
            execution_id: Uuid::new_v4(),
            source_node: NodeId::new(2),
            sequence: 1,
            stream: OutputStream::Stdout,
            data: b"remote data".to_vec(),
            timestamp_ms: 100,
            is_final: false,
        };

        manager.receive_chunk(&chunk);
        assert_eq!(manager.stats().chunks_received, 1);
    }

    #[test]
    fn test_cleanup() {
        let mut manager = OutputStreamManager::new(test_node(), StreamConfig::default());
        let exec_id = Uuid::new_v4();

        manager.start_stream(exec_id, test_node());
        manager.finish_stream(&exec_id);

        manager.cleanup(Duration::from_secs(0));
        assert!(!manager.has_stream(&exec_id));
    }

    #[test]
    fn test_output_stream_enum() {
        assert_eq!(OutputStream::Stdout, OutputStream::Stdout);
        assert_ne!(OutputStream::Stdout, OutputStream::Stderr);
    }
}
