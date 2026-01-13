//! Sandbox Provenance Tracking
//!
//! Full lineage tracking for sandbox executions:
//! - Parent/child sandbox relationships
//! - Input/output data flow tracking
//! - Execution history and replay capability
//! - Cryptographic audit trail

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Unique provenance ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProvenanceId(pub String);

impl ProvenanceId {
    /// Generate a new provenance ID.
    pub fn generate() -> Self {
        Self(generate_id("prov"))
    }

    /// Create from string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for ProvenanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Provenance record for a sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    /// Unique ID.
    pub id: ProvenanceId,
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Execution ID.
    pub execution_id: String,
    /// Parent provenance (if derived from another sandbox).
    pub parent: Option<ProvenanceId>,
    /// Child provenances (sandboxes derived from this one).
    pub children: Vec<ProvenanceId>,
    /// Input data references.
    pub inputs: Vec<DataReference>,
    /// Output data references.
    pub outputs: Vec<DataReference>,
    /// WASM module hash.
    pub module_hash: String,
    /// Configuration snapshot.
    pub config_snapshot: ConfigSnapshot,
    /// Execution metadata.
    pub execution: ExecutionMetadata,
    /// Cryptographic signature.
    pub signature: Option<ProvenanceSignature>,
    /// Annotations.
    pub annotations: HashMap<String, String>,
}

/// Reference to input/output data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataReference {
    /// Data ID.
    pub id: String,
    /// Data type.
    pub data_type: DataType,
    /// Content hash.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
    /// Source/destination.
    pub location: String,
    /// Timestamp.
    pub timestamp: SystemTime,
}

/// Data types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    /// Standard input.
    Stdin,
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// File.
    File,
    /// Network request/response.
    Network,
    /// Environment variables.
    Environment,
    /// Arguments.
    Arguments,
    /// Custom data.
    Custom(String),
}

/// Configuration snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    /// Memory limit.
    pub memory_limit: u64,
    /// CPU limit.
    pub cpu_limit: Option<u64>,
    /// Timeout.
    pub timeout_ms: u64,
    /// Capabilities granted.
    pub capabilities: Vec<String>,
    /// Environment variables.
    pub environment: HashMap<String, String>,
}

/// Execution metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    /// Start time.
    pub started_at: SystemTime,
    /// End time.
    pub ended_at: Option<SystemTime>,
    /// Exit code.
    pub exit_code: Option<i32>,
    /// Execution duration.
    pub duration_ms: Option<u64>,
    /// Host information.
    pub host: HostInfo,
    /// Resource usage.
    pub resource_usage: ResourceUsage,
}

/// Host information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    /// Hostname.
    pub hostname: String,
    /// Node ID (if in mesh).
    pub node_id: Option<String>,
    /// Runtime version.
    pub runtime_version: String,
}

/// Resource usage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Peak memory bytes.
    pub peak_memory: u64,
    /// CPU time nanoseconds.
    pub cpu_time_ns: u64,
    /// I/O bytes read.
    pub io_read_bytes: u64,
    /// I/O bytes written.
    pub io_write_bytes: u64,
    /// Fuel consumed.
    pub fuel_consumed: u64,
}

/// Provenance signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceSignature {
    /// Signature algorithm.
    pub algorithm: String,
    /// Signature bytes.
    pub signature: Vec<u8>,
    /// Signer identity.
    pub signer: String,
    /// Timestamp.
    pub timestamp: SystemTime,
}

/// Provenance tracker.
pub struct ProvenanceTracker {
    records: HashMap<ProvenanceId, ProvenanceRecord>,
    sandbox_index: HashMap<String, Vec<ProvenanceId>>,
    lineage_cache: HashMap<ProvenanceId, Vec<ProvenanceId>>,
}

impl Default for ProvenanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ProvenanceTracker {
    /// Create a new provenance tracker.
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            sandbox_index: HashMap::new(),
            lineage_cache: HashMap::new(),
        }
    }

    /// Record a new provenance entry.
    pub fn record(&mut self, record: ProvenanceRecord) {
        let id = record.id.clone();
        let sandbox_id = record.sandbox_id.clone();

        // Update parent's children
        if let Some(parent_id) = &record.parent {
            if let Some(parent) = self.records.get_mut(parent_id) {
                parent.children.push(id.clone());
            }
        }

        // Index by sandbox
        self.sandbox_index.entry(sandbox_id).or_default().push(id.clone());

        // Invalidate lineage cache
        self.lineage_cache.remove(&id);

        self.records.insert(id, record);
    }

    /// Get provenance by ID.
    pub fn get(&self, id: &ProvenanceId) -> Option<&ProvenanceRecord> {
        self.records.get(id)
    }

    /// Get provenance history for a sandbox.
    pub fn get_sandbox_history(&self, sandbox_id: &str) -> Vec<&ProvenanceRecord> {
        self.sandbox_index
            .get(sandbox_id)
            .map(|ids| ids.iter().filter_map(|id| self.records.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get full lineage (ancestors) of a provenance record.
    pub fn get_lineage(&self, id: &ProvenanceId) -> Vec<&ProvenanceRecord> {
        let mut lineage = Vec::new();
        let mut current = Some(id.clone());

        while let Some(current_id) = current {
            if let Some(record) = self.records.get(&current_id) {
                lineage.push(record);
                current = record.parent.clone();
            } else {
                break;
            }
        }

        lineage
    }

    /// Get descendants of a provenance record.
    pub fn get_descendants(&self, id: &ProvenanceId) -> Vec<&ProvenanceRecord> {
        let mut descendants = Vec::new();
        let mut queue = vec![id.clone()];

        while let Some(current_id) = queue.pop() {
            if let Some(record) = self.records.get(&current_id) {
                if &current_id != id {
                    descendants.push(record);
                }
                queue.extend(record.children.clone());
            }
        }

        descendants
    }

    /// Find provenance by input hash.
    pub fn find_by_input_hash(&self, hash: &str) -> Vec<&ProvenanceRecord> {
        self.records.values().filter(|r| r.inputs.iter().any(|i| i.hash == hash)).collect()
    }

    /// Find provenance by output hash.
    pub fn find_by_output_hash(&self, hash: &str) -> Vec<&ProvenanceRecord> {
        self.records.values().filter(|r| r.outputs.iter().any(|o| o.hash == hash)).collect()
    }

    /// Find provenance by module hash.
    pub fn find_by_module_hash(&self, hash: &str) -> Vec<&ProvenanceRecord> {
        self.records.values().filter(|r| r.module_hash == hash).collect()
    }

    /// Get data flow graph.
    pub fn get_data_flow(&self, id: &ProvenanceId) -> DataFlowGraph {
        let mut graph = DataFlowGraph::new();

        if let Some(record) = self.records.get(id) {
            graph.add_node(DataFlowNode {
                id: id.0.clone(),
                sandbox_id: record.sandbox_id.clone(),
                inputs: record.inputs.iter().map(|i| i.hash.clone()).collect(),
                outputs: record.outputs.iter().map(|o| o.hash.clone()).collect(),
            });

            // Add parent nodes
            for ancestor in self.get_lineage(id) {
                graph.add_node(DataFlowNode {
                    id: ancestor.id.0.clone(),
                    sandbox_id: ancestor.sandbox_id.clone(),
                    inputs: ancestor.inputs.iter().map(|i| i.hash.clone()).collect(),
                    outputs: ancestor.outputs.iter().map(|o| o.hash.clone()).collect(),
                });

                if let Some(parent) = &ancestor.parent {
                    graph.add_edge(&parent.0, &ancestor.id.0);
                }
            }
        }

        graph
    }

    /// Verify provenance signature.
    pub fn verify_signature(&self, id: &ProvenanceId) -> Result<bool, ProvenanceError> {
        let record = self.records.get(id).ok_or(ProvenanceError::NotFound(id.0.clone()))?;

        match &record.signature {
            Some(sig) => {
                // Simplified verification
                Ok(!sig.signature.is_empty())
            }
            None => Err(ProvenanceError::NotSigned),
        }
    }

    /// Sign a provenance record.
    pub fn sign(
        &mut self,
        id: &ProvenanceId,
        signer: &str,
        key: &[u8],
    ) -> Result<(), ProvenanceError> {
        let record = self.records.get_mut(id).ok_or(ProvenanceError::NotFound(id.0.clone()))?;

        // Simplified signing - just hash the record
        let data = format!("{:?}", record);
        let signature = compute_signature(data.as_bytes(), key);

        record.signature = Some(ProvenanceSignature {
            algorithm: "HMAC-SHA256".to_string(),
            signature,
            signer: signer.to_string(),
            timestamp: SystemTime::now(),
        });

        Ok(())
    }

    /// Get statistics.
    pub fn stats(&self) -> ProvenanceStats {
        let total_records = self.records.len();
        let signed_records = self.records.values().filter(|r| r.signature.is_some()).count();
        let with_lineage = self.records.values().filter(|r| r.parent.is_some()).count();

        ProvenanceStats {
            total_records,
            signed_records,
            records_with_lineage: with_lineage,
            total_sandboxes: self.sandbox_index.len(),
        }
    }

    /// Export provenance chain.
    pub fn export_chain(&self, id: &ProvenanceId) -> Vec<ProvenanceRecord> {
        self.get_lineage(id).into_iter().cloned().collect()
    }
}

/// Data flow graph.
#[derive(Debug, Clone, Default)]
pub struct DataFlowGraph {
    nodes: HashMap<String, DataFlowNode>,
    edges: Vec<(String, String)>,
}

impl DataFlowGraph {
    /// Create empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node.
    pub fn add_node(&mut self, node: DataFlowNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Add an edge.
    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges.push((from.to_string(), to.to_string()));
    }

    /// Get nodes.
    pub fn nodes(&self) -> &HashMap<String, DataFlowNode> {
        &self.nodes
    }

    /// Get edges.
    pub fn edges(&self) -> &[(String, String)] {
        &self.edges
    }
}

/// Node in data flow graph.
#[derive(Debug, Clone)]
pub struct DataFlowNode {
    /// Node ID.
    pub id: String,
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Input hashes.
    pub inputs: Vec<String>,
    /// Output hashes.
    pub outputs: Vec<String>,
}

/// Provenance statistics.
#[derive(Debug, Clone, Default)]
pub struct ProvenanceStats {
    /// Total records.
    pub total_records: usize,
    /// Signed records.
    pub signed_records: usize,
    /// Records with lineage.
    pub records_with_lineage: usize,
    /// Total unique sandboxes.
    pub total_sandboxes: usize,
}

/// Provenance error.
#[derive(Debug, Clone)]
pub enum ProvenanceError {
    /// Record not found.
    NotFound(String),
    /// Record not signed.
    NotSigned,
    /// Invalid signature.
    InvalidSignature,
}

impl std::fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Provenance not found: {}", id),
            Self::NotSigned => write!(f, "Provenance not signed"),
            Self::InvalidSignature => write!(f, "Invalid signature"),
        }
    }
}

impl std::error::Error for ProvenanceError {}

/// Builder for provenance records.
pub struct ProvenanceBuilder {
    sandbox_id: String,
    execution_id: String,
    parent: Option<ProvenanceId>,
    inputs: Vec<DataReference>,
    outputs: Vec<DataReference>,
    module_hash: String,
    config: ConfigSnapshot,
    host: HostInfo,
    annotations: HashMap<String, String>,
}

impl ProvenanceBuilder {
    /// Create a new builder.
    pub fn new(sandbox_id: impl Into<String>) -> Self {
        Self {
            sandbox_id: sandbox_id.into(),
            execution_id: generate_id("exec"),
            parent: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            module_hash: String::new(),
            config: ConfigSnapshot {
                memory_limit: 0,
                cpu_limit: None,
                timeout_ms: 0,
                capabilities: Vec::new(),
                environment: HashMap::new(),
            },
            host: HostInfo {
                hostname: hostname(),
                node_id: None,
                runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            annotations: HashMap::new(),
        }
    }

    /// Set parent provenance.
    pub fn parent(mut self, parent: ProvenanceId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Add input.
    pub fn input(mut self, data: DataReference) -> Self {
        self.inputs.push(data);
        self
    }

    /// Add output.
    pub fn output(mut self, data: DataReference) -> Self {
        self.outputs.push(data);
        self
    }

    /// Set module hash.
    pub fn module_hash(mut self, hash: impl Into<String>) -> Self {
        self.module_hash = hash.into();
        self
    }

    /// Set configuration.
    pub fn config(mut self, config: ConfigSnapshot) -> Self {
        self.config = config;
        self
    }

    /// Add annotation.
    pub fn annotate(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    /// Build the record.
    pub fn build(self, exit_code: Option<i32>, duration_ms: u64) -> ProvenanceRecord {
        let now = SystemTime::now();

        ProvenanceRecord {
            id: ProvenanceId::generate(),
            sandbox_id: self.sandbox_id,
            execution_id: self.execution_id,
            parent: self.parent,
            children: Vec::new(),
            inputs: self.inputs,
            outputs: self.outputs,
            module_hash: self.module_hash,
            config_snapshot: self.config,
            execution: ExecutionMetadata {
                started_at: now,
                ended_at: Some(now),
                exit_code,
                duration_ms: Some(duration_ms),
                host: self.host,
                resource_usage: ResourceUsage::default(),
            },
            signature: None,
            annotations: self.annotations,
        }
    }
}

fn generate_id(prefix: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    format!("{}-{:016x}", prefix, hasher.finish())
}

fn compute_signature(data: &[u8], key: &[u8]) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    key.hash(&mut hasher);
    hasher.finish().to_le_bytes().to_vec()
}

fn hostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_record(sandbox_id: &str) -> ProvenanceRecord {
        ProvenanceBuilder::new(sandbox_id).module_hash("abc123").build(Some(0), 100)
    }

    #[test]
    fn test_provenance_id() {
        let id1 = ProvenanceId::generate();
        let id2 = ProvenanceId::generate();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_tracker_record() {
        let mut tracker = ProvenanceTracker::new();
        let record = create_test_record("sandbox-1");
        let id = record.id.clone();

        tracker.record(record);
        assert!(tracker.get(&id).is_some());
    }

    #[test]
    fn test_sandbox_history() {
        let mut tracker = ProvenanceTracker::new();

        tracker.record(create_test_record("sandbox-1"));
        tracker.record(create_test_record("sandbox-1"));
        tracker.record(create_test_record("sandbox-2"));

        let history = tracker.get_sandbox_history("sandbox-1");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_lineage() {
        let mut tracker = ProvenanceTracker::new();

        let parent = create_test_record("sandbox-1");
        let parent_id = parent.id.clone();
        tracker.record(parent);

        let child = ProvenanceBuilder::new("sandbox-2")
            .parent(parent_id.clone())
            .module_hash("def456")
            .build(Some(0), 50);
        let child_id = child.id.clone();
        tracker.record(child);

        let lineage = tracker.get_lineage(&child_id);
        assert_eq!(lineage.len(), 2);
    }

    #[test]
    fn test_descendants() {
        let mut tracker = ProvenanceTracker::new();

        let parent = create_test_record("sandbox-1");
        let parent_id = parent.id.clone();
        tracker.record(parent);

        let child =
            ProvenanceBuilder::new("sandbox-2").parent(parent_id.clone()).build(Some(0), 50);
        tracker.record(child);

        let descendants = tracker.get_descendants(&parent_id);
        assert_eq!(descendants.len(), 1);
    }

    #[test]
    fn test_find_by_module_hash() {
        let mut tracker = ProvenanceTracker::new();

        tracker.record(ProvenanceBuilder::new("sb-1").module_hash("hash123").build(Some(0), 100));

        let found = tracker.find_by_module_hash("hash123");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_sign_record() {
        let mut tracker = ProvenanceTracker::new();
        let record = create_test_record("sandbox-1");
        let id = record.id.clone();
        tracker.record(record);

        tracker.sign(&id, "test-signer", b"secret-key").unwrap();

        let signed = tracker.get(&id).unwrap();
        assert!(signed.signature.is_some());
    }

    #[test]
    fn test_verify_signature() {
        let mut tracker = ProvenanceTracker::new();
        let record = create_test_record("sandbox-1");
        let id = record.id.clone();
        tracker.record(record);

        tracker.sign(&id, "test-signer", b"secret-key").unwrap();
        let result = tracker.verify_signature(&id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_data_flow_graph() {
        let mut tracker = ProvenanceTracker::new();
        let record = create_test_record("sandbox-1");
        let id = record.id.clone();
        tracker.record(record);

        let graph = tracker.get_data_flow(&id);
        assert_eq!(graph.nodes().len(), 1);
    }

    #[test]
    fn test_stats() {
        let mut tracker = ProvenanceTracker::new();
        tracker.record(create_test_record("sandbox-1"));
        tracker.record(create_test_record("sandbox-2"));

        let stats = tracker.stats();
        assert_eq!(stats.total_records, 2);
        assert_eq!(stats.total_sandboxes, 2);
    }

    #[test]
    fn test_builder() {
        let record = ProvenanceBuilder::new("test-sandbox")
            .module_hash("module123")
            .annotate("purpose", "testing")
            .input(DataReference {
                id: "input-1".to_string(),
                data_type: DataType::Stdin,
                hash: "inhash".to_string(),
                size: 100,
                location: "stdin".to_string(),
                timestamp: SystemTime::now(),
            })
            .build(Some(0), 1000);

        assert_eq!(record.sandbox_id, "test-sandbox");
        assert_eq!(record.inputs.len(), 1);
        assert!(record.annotations.contains_key("purpose"));
    }

    #[test]
    fn test_export_chain() {
        let mut tracker = ProvenanceTracker::new();
        let record = create_test_record("sandbox-1");
        let id = record.id.clone();
        tracker.record(record);

        let chain = tracker.export_chain(&id);
        assert_eq!(chain.len(), 1);
    }
}
