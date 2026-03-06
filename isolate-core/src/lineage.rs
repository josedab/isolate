//! Execution lineage tracking for sandbox runs.
//!
//! Records the provenance chain of every execution: which module,
//! with what input, produced what output, and how many resources
//! were consumed. Useful for audit trails and debugging.

use crate::config::ModuleHash;
use crate::resource::ResourceUsage;
use crate::sandbox::SandboxId;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// Unique identifier for an execution run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub Uuid);

impl RunId {
    /// Create a new random run ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A record of a single sandbox execution with full provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Unique ID for this execution.
    pub run_id: RunId,
    /// Sandbox that performed the execution.
    pub sandbox_id: SandboxId,
    /// SHA-256 hash of the WASM module.
    pub module_hash: ModuleHash,
    /// SHA-256 hash of the input bytes.
    pub input_hash: String,
    /// SHA-256 hash of the combined stdout + stderr output.
    pub output_hash: String,
    /// Exit code of the execution.
    pub exit_code: i32,
    /// When the execution started.
    pub started_at: SystemTime,
    /// Wall-clock duration of the execution.
    pub duration: Duration,
    /// Resource consumption during this execution.
    pub resource_usage: ResourceUsage,
}

impl ExecutionTrace {
    /// Create a new execution trace.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sandbox_id: SandboxId,
        module_hash: ModuleHash,
        input: &[u8],
        stdout: &[u8],
        stderr: &[u8],
        exit_code: i32,
        started_at: SystemTime,
        duration: Duration,
        resource_usage: ResourceUsage,
    ) -> Self {
        Self {
            run_id: RunId::new(),
            sandbox_id,
            module_hash,
            input_hash: sha256_hex(input),
            output_hash: sha256_hex(&[stdout, stderr].concat()),
            exit_code,
            started_at,
            duration,
            resource_usage,
        }
    }
}

/// Collects execution traces for a sandbox.
#[derive(Debug, Clone, Default)]
pub struct ExecutionLog {
    traces: Vec<ExecutionTrace>,
}

impl ExecutionLog {
    /// Create a new empty execution log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an execution trace.
    pub fn record(&mut self, trace: ExecutionTrace) {
        self.traces.push(trace);
    }

    /// Get all recorded traces.
    pub fn traces(&self) -> &[ExecutionTrace] {
        &self.traces
    }

    /// Get the most recent trace.
    pub fn last(&self) -> Option<&ExecutionTrace> {
        self.traces.last()
    }

    /// Number of recorded executions.
    pub fn len(&self) -> usize {
        self.traces.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }
}

/// An execution log that persists traces to a JSON-lines file on disk.
///
/// Each trace is appended as a single JSON line, making the file appendable
/// and streamable. Suitable for audit trails and post-hoc analysis.
///
/// The file handle is held open and protected by a Mutex to prevent
/// concurrent writes from interleaving partial JSON lines.
///
/// # Examples
///
/// ```no_run
/// use isolate_core::lineage::PersistentExecutionLog;
/// use std::path::PathBuf;
///
/// let mut log = PersistentExecutionLog::new(PathBuf::from("/tmp/traces.jsonl")).unwrap();
/// // Traces are appended to the file as they are recorded.
/// ```
pub struct PersistentExecutionLog {
    inner: ExecutionLog,
    path: std::path::PathBuf,
    writer: std::sync::Mutex<std::io::BufWriter<std::fs::File>>,
}

impl PersistentExecutionLog {
    /// Create a new persistent log that appends to the given file.
    ///
    /// Creates the file (and parent directories) if it doesn't exist.
    /// Existing files are appended to, not overwritten.
    pub fn new(path: std::path::PathBuf) -> std::result::Result<Self, std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            inner: ExecutionLog::new(),
            path,
            writer: std::sync::Mutex::new(std::io::BufWriter::new(file)),
        })
    }

    /// Record a trace to both memory and the file on disk.
    ///
    /// The trace is serialized as a single JSON line and appended atomically
    /// (protected by a Mutex to prevent interleaving under concurrent access).
    pub fn record(&mut self, trace: ExecutionTrace) -> std::result::Result<(), std::io::Error> {
        use std::io::Write;
        let line = serde_json::to_string(&trace).map_err(std::io::Error::other)?;
        // Lock the writer to ensure the full line is written atomically
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("lineage writer lock poisoned"))?;
        writeln!(writer, "{}", line)?;
        writer.flush()?;
        drop(writer);
        self.inner.record(trace);
        Ok(())
    }

    /// Get all in-memory traces.
    pub fn traces(&self) -> &[ExecutionTrace] {
        self.inner.traces()
    }

    /// Load traces from an existing file into memory.
    ///
    /// Reads all JSON lines from the file and adds them to the in-memory log.
    /// Malformed lines are silently skipped.
    pub fn load(&mut self) -> std::result::Result<usize, std::io::Error> {
        use std::io::BufRead;
        if !self.path.exists() {
            return Ok(0);
        }
        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        let mut count = 0;
        let mut skipped = 0;
        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ExecutionTrace>(&line) {
                Ok(trace) => {
                    self.inner.record(trace);
                    count += 1;
                }
                Err(e) => {
                    skipped += 1;
                    tracing::warn!(
                        path = %self.path.display(),
                        line = line_num + 1,
                        error = %e,
                        "Skipped malformed execution trace"
                    );
                }
            }
        }
        if skipped > 0 {
            tracing::warn!(
                path = %self.path.display(),
                skipped,
                loaded = count,
                "Lineage log contained malformed entries"
            );
        }
        Ok(count)
    }

    /// Get the number of recorded traces.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the file path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_id() {
        let id1 = RunId::new();
        let id2 = RunId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_execution_trace() {
        let trace = ExecutionTrace::new(
            SandboxId::new(),
            ModuleHash::from_bytes(&[0x00, 0x61, 0x73, 0x6d]),
            b"input",
            b"output",
            b"",
            0,
            SystemTime::now(),
            Duration::from_millis(42),
            ResourceUsage::default(),
        );

        assert_eq!(trace.exit_code, 0);
        assert!(!trace.input_hash.is_empty());
        assert!(!trace.output_hash.is_empty());
        // Different inputs produce different hashes
        let trace2 = ExecutionTrace::new(
            SandboxId::new(),
            ModuleHash::from_bytes(&[0x00, 0x61, 0x73, 0x6d]),
            b"different input",
            b"output",
            b"",
            0,
            SystemTime::now(),
            Duration::from_millis(42),
            ResourceUsage::default(),
        );
        assert_ne!(trace.input_hash, trace2.input_hash);
    }

    #[test]
    fn test_execution_log() {
        let mut log = ExecutionLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);

        let trace = ExecutionTrace::new(
            SandboxId::new(),
            ModuleHash::from_bytes(&[0x00, 0x61, 0x73, 0x6d]),
            b"",
            b"hello",
            b"",
            0,
            SystemTime::now(),
            Duration::from_millis(10),
            ResourceUsage::default(),
        );

        log.record(trace);
        assert_eq!(log.len(), 1);
        assert!(!log.is_empty());
        assert!(log.last().is_some());
        assert_eq!(log.traces()[0].exit_code, 0);
    }

    fn make_trace(exit_code: i32) -> ExecutionTrace {
        ExecutionTrace::new(
            SandboxId::new(),
            ModuleHash::from_bytes(&[0x00, 0x61, 0x73, 0x6d]),
            b"input",
            b"output",
            b"",
            exit_code,
            SystemTime::now(),
            Duration::from_millis(10),
            ResourceUsage::default(),
        )
    }

    #[test]
    fn test_persistent_log_write_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traces.jsonl");

        // Write two traces
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(0)).unwrap();
            log.record(make_trace(1)).unwrap();
            assert_eq!(log.len(), 2);
        }

        // Load them back in a new instance
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            assert_eq!(log.len(), 0); // not loaded yet
            let count = log.load().unwrap();
            assert_eq!(count, 2);
            assert_eq!(log.traces()[0].exit_code, 0);
            assert_eq!(log.traces()[1].exit_code, 1);
        }
    }

    #[test]
    fn test_persistent_log_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traces.jsonl");

        // Write one trace
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(0)).unwrap();
        }
        // Append another
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(42)).unwrap();
        }
        // Load all — should have 2
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            let count = log.load().unwrap();
            assert_eq!(count, 2);
        }
    }

    #[test]
    fn test_persistent_log_load_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.jsonl");
        std::fs::write(&path, "").unwrap();

        let mut log = PersistentExecutionLog::new(path).unwrap();
        let count = log.load().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_persistent_log_load_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.jsonl");

        let mut log = PersistentExecutionLog::new(path).unwrap();
        let count = log.load().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_persistent_log_skips_corrupt_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.jsonl");

        // Write a valid trace, a corrupt line, and another valid trace
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(0)).unwrap();
        }
        // Manually append corrupt line
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{not valid json}}").unwrap();
        writeln!(file, "truncated").unwrap();
        drop(file);
        // Append another valid trace
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(42)).unwrap();
        }

        // Load — should get 2 valid, skip 2 corrupt
        let mut log = PersistentExecutionLog::new(path).unwrap();
        let count = log.load().unwrap();
        assert_eq!(count, 2, "Should load 2 valid traces, skipping 2 corrupt lines");
        assert_eq!(log.traces()[0].exit_code, 0);
        assert_eq!(log.traces()[1].exit_code, 42);
    }

    #[test]
    fn test_persistent_log_skips_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blanks.jsonl");

        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(0)).unwrap();
        }
        // Insert blank lines
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file).unwrap(); // blank
        writeln!(file, "   ").unwrap(); // whitespace only
        drop(file);
        {
            let mut log = PersistentExecutionLog::new(path.clone()).unwrap();
            log.record(make_trace(1)).unwrap();
        }

        let mut log = PersistentExecutionLog::new(path).unwrap();
        let count = log.load().unwrap();
        assert_eq!(count, 2, "Should skip blank lines and load 2 valid traces");
    }
}
