//! Instant clone engine for sub-100μs CoW sandbox cloning, disk persistence,
//! and cross-node snapshot restore.

use super::cow::PageHash;
use super::{GlobalValue, MemoryPage, Snapshot, SnapshotId, SnapshotMetadata};
use crate::config::ModuleHash;
use crate::sandbox::SandboxId;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// CloneTemplate
// ---------------------------------------------------------------------------

/// Unique identifier for a clone template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TemplateId(pub Uuid);

impl TemplateId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TemplateId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TemplateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Pre-resolved page entry – either inline data or a zero page.
/// All references are resolved at template creation time so cloning
/// never needs lazy resolution.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ResolvedPage {
    Zero,
    Data(Arc<Vec<u8>>),
}

/// Optimized representation of a snapshot for fast cloning.
///
/// All page references are pre-resolved and shared via `Arc`, so
/// creating a [`CloneInstance`] from a template is a cheap metadata
/// copy + Arc clone per page (well under 100 μs for typical workloads).
#[derive(Debug, Clone)]
pub struct CloneTemplate {
    /// Unique template id.
    pub template_id: TemplateId,
    /// Source snapshot this template was built from.
    pub source_snapshot_id: SnapshotId,
    /// Module hash for the source module.
    pub module_hash: ModuleHash,
    /// Timestamp when the template was created.
    pub created_at: DateTime<Utc>,
    /// Pre-resolved, reference-counted base pages.
    pages: HashMap<usize, Arc<Vec<u8>>>,
    /// Total memory size in bytes.
    pub memory_size: usize,
    /// Page size.
    pub page_size: usize,
    /// Globals snapshot.
    pub globals: Vec<GlobalValue>,
    /// Table entries.
    pub tables: HashMap<String, Vec<Option<u32>>>,
    /// Remaining fuel.
    pub fuel_remaining: Option<u64>,
    /// Original memory checksum.
    pub memory_checksum: String,
    /// Snapshot metadata carried forward.
    pub metadata: SnapshotMetadata,
}

impl CloneTemplate {
    /// Build a template from a *fully-resolved* snapshot (no `Reference` pages).
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let mut pages = HashMap::new();
        for (&idx, page) in &snapshot.memory_pages {
            if let MemoryPage::Data(data) = page {
                pages.insert(idx, Arc::new(data.clone()));
            }
            // Zero and Reference pages are not stored – zeros are implicit,
            // and references should already have been resolved before calling
            // this constructor.
        }

        Self {
            template_id: TemplateId::new(),
            source_snapshot_id: snapshot.id,
            module_hash: snapshot.module_hash.clone(),
            created_at: Utc::now(),
            pages,
            memory_size: snapshot.memory_size,
            page_size: snapshot.page_size,
            globals: snapshot.globals.clone(),
            tables: snapshot.tables.clone(),
            fuel_remaining: snapshot.fuel_remaining,
            memory_checksum: snapshot.memory_checksum.clone(),
            metadata: snapshot.metadata.clone(),
        }
    }

    /// Number of non-zero base pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

// ---------------------------------------------------------------------------
// CloneInstance
// ---------------------------------------------------------------------------

/// A cloned sandbox instance backed by shared base pages with private
/// copy-on-write dirty pages.
#[derive(Debug)]
pub struct CloneInstance {
    /// The template this instance was cloned from.
    template_id: TemplateId,
    /// Shared (read-only) base pages from the template.
    base_pages: HashMap<usize, Arc<Vec<u8>>>,
    /// Private dirty pages – written on first modification (CoW).
    dirty_pages: HashMap<usize, Vec<u8>>,
    /// Total memory size.
    memory_size: usize,
    /// Page size.
    page_size: usize,
    /// Cloned globals (mutable per-instance).
    pub globals: Vec<GlobalValue>,
    /// Cloned tables.
    pub tables: HashMap<String, Vec<Option<u32>>>,
    /// Fuel remaining.
    pub fuel_remaining: Option<u64>,
    /// Module hash.
    pub module_hash: ModuleHash,
    /// Original memory checksum (from template).
    memory_checksum: String,
    /// Metadata carried forward.
    pub metadata: SnapshotMetadata,
}

impl CloneInstance {
    /// Create a clone instance from a template.
    fn from_template(template: &CloneTemplate) -> Self {
        Self {
            template_id: template.template_id,
            base_pages: template.pages.clone(), // Arc clones – cheap
            dirty_pages: HashMap::new(),
            memory_size: template.memory_size,
            page_size: template.page_size,
            globals: template.globals.clone(),
            tables: template.tables.clone(),
            fuel_remaining: template.fuel_remaining,
            module_hash: template.module_hash.clone(),
            memory_checksum: template.memory_checksum.clone(),
            metadata: template.metadata.clone(),
        }
    }

    /// Read a page by index. Returns data from the private dirty page if
    /// present, otherwise from the shared base, otherwise a zero page.
    pub fn read_page(&self, idx: usize) -> Vec<u8> {
        if let Some(dirty) = self.dirty_pages.get(&idx) {
            return dirty.clone();
        }
        if let Some(base) = self.base_pages.get(&idx) {
            return (**base).clone();
        }
        vec![0u8; self.page_size]
    }

    /// Write a page by index. On first write, the page is copied from the
    /// shared base into the private dirty set (CoW semantics).
    pub fn write_page(&mut self, idx: usize, data: Vec<u8>) {
        self.dirty_pages.insert(idx, data);
    }

    /// Number of pages that have been modified since cloning.
    pub fn dirty_page_count(&self) -> usize {
        self.dirty_pages.len()
    }

    /// The template this instance was cloned from.
    pub fn template_id(&self) -> TemplateId {
        self.template_id
    }

    /// Serialize this clone instance back into a full [`Snapshot`].
    pub fn to_snapshot(&self) -> Snapshot {
        let mut memory_pages = HashMap::new();
        let total_pages = (self.memory_size + self.page_size - 1) / self.page_size;

        for idx in 0..total_pages {
            if let Some(dirty) = self.dirty_pages.get(&idx) {
                memory_pages.insert(idx, MemoryPage::Data(dirty.clone()));
            } else if let Some(base) = self.base_pages.get(&idx) {
                memory_pages.insert(idx, MemoryPage::Data((**base).clone()));
            } else {
                memory_pages.insert(idx, MemoryPage::Zero);
            }
        }

        Snapshot {
            id: SnapshotId::new(),
            sandbox_id: SandboxId::new(),
            module_hash: self.module_hash.clone(),
            created_at: Utc::now(),
            memory_pages,
            memory_size: self.memory_size,
            page_size: self.page_size,
            globals: self.globals.clone(),
            tables: self.tables.clone(),
            fuel_remaining: self.fuel_remaining,
            memory_checksum: self.memory_checksum.clone(),
            parent_id: None,
            metadata: self.metadata.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// InstantCloneEngine
// ---------------------------------------------------------------------------

/// Engine that manages clone templates and produces clone instances in
/// sub-100 μs by leveraging pre-resolved, reference-counted pages.
pub struct InstantCloneEngine {
    templates: HashMap<TemplateId, CloneTemplate>,
    clone_counter: AtomicUsize,
}

impl InstantCloneEngine {
    /// Create a new empty engine.
    pub fn new() -> Self {
        Self { templates: HashMap::new(), clone_counter: AtomicUsize::new(0) }
    }

    /// Pre-process a snapshot into a clone template and register it.
    pub fn create_template(&mut self, snapshot: &Snapshot) -> TemplateId {
        let template = CloneTemplate::from_snapshot(snapshot);
        let id = template.template_id;
        self.templates.insert(id, template);
        id
    }

    /// Create a new [`CloneInstance`] from a registered template.
    /// This is the hot path – target is < 100 μs.
    pub fn clone(&self, template_id: &TemplateId) -> Option<CloneInstance> {
        let template = self.templates.get(template_id)?;
        self.clone_counter.fetch_add(1, Ordering::Relaxed);
        Some(CloneInstance::from_template(template))
    }

    /// Total number of clones produced since engine creation.
    pub fn clone_count(&self) -> usize {
        self.clone_counter.load(Ordering::Relaxed)
    }

    /// Number of registered templates.
    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    /// Remove a template by id.
    pub fn remove_template(&mut self, id: &TemplateId) -> bool {
        self.templates.remove(id).is_some()
    }

    /// Retrieve a reference to a template.
    pub fn get_template(&self, id: &TemplateId) -> Option<&CloneTemplate> {
        self.templates.get(id)
    }
}

impl Default for InstantCloneEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CrossNodeRestore
// ---------------------------------------------------------------------------

/// Address of a remote node for cross-node snapshot transfer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeAddress {
    pub host: String,
    pub port: u16,
}

impl NodeAddress {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self { host: host.into(), port }
    }
}

impl std::fmt::Display for NodeAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// Request to restore a snapshot on a remote node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub snapshot_id: SnapshotId,
    pub target_node: NodeAddress,
    pub module_hash: ModuleHash,
    pub transfer_manifest: TransferManifest,
}

/// Response from a successful cross-node restore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResponse {
    pub sandbox_id: SandboxId,
    pub node: NodeAddress,
    pub restored_at: DateTime<Utc>,
}

/// Manifest describing which pages need to be transferred.
/// Only dirty / non-zero pages are included to minimize network traffic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferManifest {
    pub snapshot_id: SnapshotId,
    pub page_indices: Vec<usize>,
    pub total_bytes: usize,
    pub page_hashes: HashMap<usize, Vec<u8>>,
}

impl TransferManifest {
    /// Build a manifest from a snapshot, including only data pages.
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let mut page_indices = Vec::new();
        let mut total_bytes: usize = 0;
        let mut page_hashes = HashMap::new();

        for (&idx, page) in &snapshot.memory_pages {
            if let MemoryPage::Data(data) = page {
                page_indices.push(idx);
                total_bytes += data.len();
                let hash = PageHash::from_data(data);
                page_hashes.insert(idx, hash.as_bytes().to_vec());
            }
        }
        page_indices.sort();

        Self { snapshot_id: snapshot.id, page_indices, total_bytes, page_hashes }
    }

    /// Number of pages that need transfer.
    pub fn page_count(&self) -> usize {
        self.page_indices.len()
    }
}

/// Configuration / helper for cross-node snapshot restore.
#[derive(Debug, Clone)]
pub struct CrossNodeRestore {
    pub local_node: NodeAddress,
    pub known_peers: Vec<NodeAddress>,
}

impl CrossNodeRestore {
    pub fn new(local_node: NodeAddress) -> Self {
        Self { local_node, known_peers: Vec::new() }
    }

    pub fn add_peer(&mut self, peer: NodeAddress) {
        if !self.known_peers.contains(&peer) {
            self.known_peers.push(peer);
        }
    }

    /// Build a [`RestoreRequest`] for sending a snapshot to a target node.
    pub fn build_request(&self, snapshot: &Snapshot, target: NodeAddress) -> RestoreRequest {
        RestoreRequest {
            snapshot_id: snapshot.id,
            target_node: target,
            module_hash: snapshot.module_hash.clone(),
            transfer_manifest: TransferManifest::from_snapshot(snapshot),
        }
    }
}

// ---------------------------------------------------------------------------
// DiskSnapshotPersistence
// ---------------------------------------------------------------------------

/// Manages snapshot persistence to disk using simple file-based storage.
/// Each snapshot is serialised to a single file named by its [`SnapshotId`].
pub struct DiskSnapshotPersistence {
    base_path: PathBuf,
}

impl DiskSnapshotPersistence {
    /// Create a new persistence manager rooted at `base_path`.
    /// The directory is created if it does not exist.
    pub fn new(base_path: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&base_path)?;
        Ok(Self { base_path })
    }

    fn snapshot_path(&self, id: &SnapshotId) -> PathBuf {
        self.base_path.join(format!("{}.snap", id))
    }

    /// Persist a snapshot to disk.
    pub fn persist(&self, snapshot: &Snapshot) -> std::io::Result<PathBuf> {
        let path = self.snapshot_path(&snapshot.id);
        let data = serde_json::to_vec(snapshot)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, &data)?;
        Ok(path)
    }

    /// Load a snapshot from disk by id.
    pub fn load(&self, id: &SnapshotId) -> std::io::Result<Snapshot> {
        let path = self.snapshot_path(id);
        let data = std::fs::read(&path)?;
        serde_json::from_slice(&data).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    /// List all persisted snapshot ids.
    pub fn list(&self) -> std::io::Result<Vec<SnapshotId>> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".snap") {
                if let Ok(uuid) = Uuid::parse_str(stem) {
                    ids.push(SnapshotId(uuid));
                }
            }
        }
        Ok(ids)
    }

    /// Delete a persisted snapshot.
    pub fn delete(&self, id: &SnapshotId) -> std::io::Result<()> {
        let path = self.snapshot_path(id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ModuleHash;
    use crate::sandbox::SandboxId;

    /// Helper: build a small snapshot with some non-zero data.
    fn make_snapshot(data_pages: &[(usize, &[u8])]) -> Snapshot {
        let mut snap = Snapshot::new(SandboxId::new(), ModuleHash("mod1".into()));
        snap.page_size = 64; // small pages for tests
        snap.memory_size = 256; // 4 pages
        for &(idx, bytes) in data_pages {
            snap.memory_pages.insert(idx, MemoryPage::Data(bytes.to_vec()));
        }
        // zero pages for the rest
        for idx in 0..4 {
            snap.memory_pages.entry(idx).or_insert(MemoryPage::Zero);
        }
        snap
    }

    // -- CloneTemplate tests --

    #[test]
    fn test_template_from_snapshot_preserves_data_pages() {
        let snap = make_snapshot(&[(0, &[1; 64]), (2, &[2; 64])]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        assert_eq!(tmpl.page_count(), 2);
        assert!(tmpl.pages.contains_key(&0));
        assert!(tmpl.pages.contains_key(&2));
    }

    #[test]
    fn test_template_zero_pages_not_stored() {
        let snap = make_snapshot(&[]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        assert_eq!(tmpl.page_count(), 0);
    }

    #[test]
    fn test_template_metadata_carried() {
        let mut snap = make_snapshot(&[]);
        snap.metadata.label = Some("test-label".into());
        let tmpl = CloneTemplate::from_snapshot(&snap);
        assert_eq!(tmpl.metadata.label.as_deref(), Some("test-label"));
        assert_eq!(tmpl.source_snapshot_id, snap.id);
    }

    // -- CloneInstance tests --

    #[test]
    fn test_clone_read_base_page() {
        let snap = make_snapshot(&[(1, &[0xAB; 64])]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        let inst = CloneInstance::from_template(&tmpl);
        assert_eq!(inst.read_page(1), vec![0xAB; 64]);
    }

    #[test]
    fn test_clone_read_zero_page() {
        let snap = make_snapshot(&[]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        let inst = CloneInstance::from_template(&tmpl);
        assert_eq!(inst.read_page(0), vec![0u8; 64]);
    }

    #[test]
    fn test_clone_write_cow() {
        let snap = make_snapshot(&[(0, &[1; 64])]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        let mut inst = CloneInstance::from_template(&tmpl);

        assert_eq!(inst.dirty_page_count(), 0);
        inst.write_page(0, vec![9; 64]);
        assert_eq!(inst.dirty_page_count(), 1);
        assert_eq!(inst.read_page(0), vec![9; 64]);
    }

    #[test]
    fn test_clone_write_does_not_affect_base() {
        let snap = make_snapshot(&[(0, &[1; 64])]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        let mut inst1 = CloneInstance::from_template(&tmpl);
        let inst2 = CloneInstance::from_template(&tmpl);

        inst1.write_page(0, vec![0xFF; 64]);
        // inst2 still sees original
        assert_eq!(inst2.read_page(0), vec![1; 64]);
    }

    #[test]
    fn test_clone_to_snapshot() {
        let snap = make_snapshot(&[(0, &[1; 64]), (1, &[2; 64])]);
        let tmpl = CloneTemplate::from_snapshot(&snap);
        let mut inst = CloneInstance::from_template(&tmpl);
        inst.write_page(1, vec![0xFF; 64]);

        let restored = inst.to_snapshot();
        assert_eq!(restored.memory_pages.len(), 4);
        let MemoryPage::Data(d) = &restored.memory_pages[&1] else {
            unreachable!("expected Data page at index 1");
        };
        assert_eq!(d, &vec![0xFF; 64]);
    }

    // -- InstantCloneEngine tests --

    #[test]
    fn test_engine_create_template() {
        let mut engine = InstantCloneEngine::new();
        let snap = make_snapshot(&[(0, &[5; 64])]);
        let tid = engine.create_template(&snap);
        assert_eq!(engine.template_count(), 1);
        assert!(engine.get_template(&tid).is_some());
    }

    #[test]
    fn test_engine_clone_increments_counter() {
        let mut engine = InstantCloneEngine::new();
        let snap = make_snapshot(&[(0, &[5; 64])]);
        let tid = engine.create_template(&snap);
        assert_eq!(engine.clone_count(), 0);

        let _c1 = engine.clone(&tid).unwrap();
        let _c2 = engine.clone(&tid).unwrap();
        assert_eq!(engine.clone_count(), 2);
    }

    #[test]
    fn test_engine_clone_missing_template() {
        let engine = InstantCloneEngine::new();
        assert!(engine.clone(&TemplateId::new()).is_none());
    }

    #[test]
    fn test_engine_remove_template() {
        let mut engine = InstantCloneEngine::new();
        let snap = make_snapshot(&[]);
        let tid = engine.create_template(&snap);
        assert!(engine.remove_template(&tid));
        assert_eq!(engine.template_count(), 0);
        assert!(!engine.remove_template(&tid));
    }

    // -- CrossNodeRestore tests --

    #[test]
    fn test_transfer_manifest_only_data_pages() {
        let snap = make_snapshot(&[(1, &[7; 64])]);
        let manifest = TransferManifest::from_snapshot(&snap);
        assert_eq!(manifest.page_count(), 1);
        assert_eq!(manifest.page_indices, vec![1]);
        assert_eq!(manifest.total_bytes, 64);
    }

    #[test]
    fn test_cross_node_build_request() {
        let snap = make_snapshot(&[(0, &[1; 64])]);
        let mut cnr = CrossNodeRestore::new(NodeAddress::new("127.0.0.1", 8000));
        let target = NodeAddress::new("10.0.0.2", 9000);
        cnr.add_peer(target.clone());

        let req = cnr.build_request(&snap, target.clone());
        assert_eq!(req.target_node, target);
        assert_eq!(req.snapshot_id, snap.id);
    }

    #[test]
    fn test_node_address_display() {
        let addr = NodeAddress::new("host.example.com", 443);
        assert_eq!(addr.to_string(), "host.example.com:443");
    }

    // -- DiskSnapshotPersistence tests --

    #[test]
    fn test_disk_persist_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = DiskSnapshotPersistence::new(dir.path().to_path_buf()).unwrap();

        let snap = make_snapshot(&[(0, &[42; 64])]);
        let id = snap.id;
        store.persist(&snap).unwrap();

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.id, id);
        let MemoryPage::Data(d) = &loaded.memory_pages[&0] else {
            unreachable!("expected Data page at index 0");
        };
        assert_eq!(d, &vec![42; 64]);
    }

    #[test]
    fn test_disk_list_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let store = DiskSnapshotPersistence::new(dir.path().to_path_buf()).unwrap();

        let s1 = make_snapshot(&[]);
        let s2 = make_snapshot(&[]);
        store.persist(&s1).unwrap();
        store.persist(&s2).unwrap();

        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_disk_delete_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = DiskSnapshotPersistence::new(dir.path().to_path_buf()).unwrap();

        let snap = make_snapshot(&[]);
        let id = snap.id;
        store.persist(&snap).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        store.delete(&id).unwrap();
        assert_eq!(store.list().unwrap().len(), 0);
    }
}
