//! Persistent snapshot storage backends.
//!
//! Provides pluggable storage backends for snapshot persistence:
//! - Filesystem backend with atomic writes
//! - In-memory backend for testing
//! - Garbage collection for orphaned snapshots

use super::serialization::SnapshotSerializer;
use super::{Snapshot, SnapshotId};
use crate::config::ModuleHash;
use crate::error::{Error, Result};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

/// Metadata for a stored snapshot (lightweight, without page data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// Snapshot ID.
    pub id: SnapshotId,
    /// Module hash this snapshot belongs to.
    pub module_hash: ModuleHash,
    /// When the snapshot was created.
    pub created_at: DateTime<Utc>,
    /// Size in bytes on disk.
    pub size_bytes: u64,
    /// Parent snapshot ID (for incremental snapshots).
    pub parent_id: Option<SnapshotId>,
    /// Number of times this snapshot has been restored.
    pub restore_count: u64,
    /// Last time this snapshot was accessed.
    pub last_accessed: DateTime<Utc>,
    /// Labels for organization.
    pub labels: HashMap<String, String>,
}

/// Trait for snapshot storage backends.
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Store a snapshot. Returns the storage size in bytes.
    async fn store(&self, snapshot: &Snapshot) -> Result<u64>;

    /// Load a snapshot by ID.
    async fn load(&self, id: SnapshotId) -> Result<Snapshot>;

    /// Delete a snapshot by ID.
    async fn delete(&self, id: SnapshotId) -> Result<()>;

    /// Check if a snapshot exists.
    async fn exists(&self, id: SnapshotId) -> Result<bool>;

    /// List all snapshot entries.
    async fn list(&self) -> Result<Vec<SnapshotEntry>>;

    /// List snapshots for a specific module.
    async fn list_for_module(&self, module_hash: &ModuleHash) -> Result<Vec<SnapshotEntry>>;

    /// Get storage statistics.
    async fn stats(&self) -> Result<StorageStats>;
}

/// Storage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total number of snapshots.
    pub total_snapshots: u64,
    /// Total storage size in bytes.
    pub total_size_bytes: u64,
    /// Number of unique modules.
    pub unique_modules: u64,
    /// Total restore operations.
    pub total_restores: u64,
}

/// Filesystem-based snapshot storage with atomic writes.
pub struct FilesystemStore {
    /// Root directory for snapshot storage.
    root: PathBuf,
    /// Serializer for snapshot data.
    serializer: SnapshotSerializer,
    /// Index of stored snapshots.
    index: Arc<RwLock<HashMap<SnapshotId, SnapshotEntry>>>,
    /// Statistics counters.
    total_restores: AtomicU64,
}

impl FilesystemStore {
    /// Create a new filesystem store at the given path.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join("snapshots"))?;
        std::fs::create_dir_all(root.join("tmp"))?;

        let store = Self {
            root,
            serializer: SnapshotSerializer::new(),
            index: Arc::new(RwLock::new(HashMap::new())),
            total_restores: AtomicU64::new(0),
        };

        // Load existing index if present
        store.load_index();

        Ok(store)
    }

    /// Get the path for a snapshot file.
    fn snapshot_path(&self, id: SnapshotId) -> PathBuf {
        self.root.join("snapshots").join(format!("{}.snapshot", id.0))
    }

    /// Get a temporary file path for atomic writes.
    fn tmp_path(&self, id: SnapshotId) -> PathBuf {
        self.root.join("tmp").join(format!("{}.tmp", id.0))
    }

    /// Index file path.
    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    /// Load the index from disk.
    fn load_index(&self) {
        let index_path = self.index_path();
        if index_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&index_path) {
                if let Ok(entries) = serde_json::from_str::<Vec<SnapshotEntry>>(&data) {
                    let mut index = self.index.write();
                    for entry in entries {
                        index.insert(entry.id, entry);
                    }
                }
            }
        }
    }

    /// Save the index to disk.
    fn save_index(&self) -> Result<()> {
        let index = self.index.read();
        let entries: Vec<&SnapshotEntry> = index.values().collect();
        let data = serde_json::to_string_pretty(&entries)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize index: {}", e)))?;
        std::fs::write(self.index_path(), data)?;
        Ok(())
    }
}

#[async_trait]
impl SnapshotStore for FilesystemStore {
    async fn store(&self, snapshot: &Snapshot) -> Result<u64> {
        let data = self.serializer.serialize(snapshot)?;
        let size = data.len() as u64;

        // Atomic write: write to tmp, then rename
        let tmp_path = self.tmp_path(snapshot.id);
        let final_path = self.snapshot_path(snapshot.id);

        std::fs::write(&tmp_path, &data)?;
        std::fs::rename(&tmp_path, &final_path)?;

        // Update index
        let entry = SnapshotEntry {
            id: snapshot.id,
            module_hash: snapshot.module_hash.clone(),
            created_at: snapshot.created_at,
            size_bytes: size,
            parent_id: snapshot.parent_id,
            restore_count: 0,
            last_accessed: Utc::now(),
            labels: HashMap::new(),
        };

        self.index.write().insert(snapshot.id, entry);
        self.save_index()?;

        tracing::info!(
            snapshot_id = %snapshot.id.0,
            size_bytes = size,
            "Snapshot stored to filesystem"
        );

        Ok(size)
    }

    async fn load(&self, id: SnapshotId) -> Result<Snapshot> {
        let path = self.snapshot_path(id);
        if !path.exists() {
            return Err(Error::SnapshotNotFound(id.0.to_string()));
        }

        let snapshot = self.serializer.deserialize_from_file(&path)?;

        // Update access time and restore count
        if let Some(entry) = self.index.write().get_mut(&id) {
            entry.last_accessed = Utc::now();
            entry.restore_count += 1;
        }
        self.total_restores.fetch_add(1, Ordering::Relaxed);

        Ok(snapshot)
    }

    async fn delete(&self, id: SnapshotId) -> Result<()> {
        let path = self.snapshot_path(id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        self.index.write().remove(&id);
        self.save_index()?;
        Ok(())
    }

    async fn exists(&self, id: SnapshotId) -> Result<bool> {
        Ok(self.index.read().contains_key(&id))
    }

    async fn list(&self) -> Result<Vec<SnapshotEntry>> {
        Ok(self.index.read().values().cloned().collect())
    }

    async fn list_for_module(&self, module_hash: &ModuleHash) -> Result<Vec<SnapshotEntry>> {
        Ok(self.index.read().values().filter(|e| e.module_hash == *module_hash).cloned().collect())
    }

    async fn stats(&self) -> Result<StorageStats> {
        let index = self.index.read();
        let total_size: u64 = index.values().map(|e| e.size_bytes).sum();
        let unique_modules =
            index.values().map(|e| &e.module_hash).collect::<std::collections::HashSet<_>>().len();

        Ok(StorageStats {
            total_snapshots: index.len() as u64,
            total_size_bytes: total_size,
            unique_modules: unique_modules as u64,
            total_restores: self.total_restores.load(Ordering::Relaxed),
        })
    }
}

/// In-memory snapshot store for testing.
pub struct MemoryStore {
    snapshots: Arc<RwLock<HashMap<SnapshotId, Vec<u8>>>>,
    entries: Arc<RwLock<HashMap<SnapshotId, SnapshotEntry>>>,
    serializer: SnapshotSerializer,
}

impl MemoryStore {
    /// Create a new in-memory store.
    pub fn new() -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            entries: Arc::new(RwLock::new(HashMap::new())),
            serializer: SnapshotSerializer::new(),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SnapshotStore for MemoryStore {
    async fn store(&self, snapshot: &Snapshot) -> Result<u64> {
        let data = self.serializer.serialize(snapshot)?;
        let size = data.len() as u64;

        let entry = SnapshotEntry {
            id: snapshot.id,
            module_hash: snapshot.module_hash.clone(),
            created_at: snapshot.created_at,
            size_bytes: size,
            parent_id: snapshot.parent_id,
            restore_count: 0,
            last_accessed: Utc::now(),
            labels: HashMap::new(),
        };

        self.snapshots.write().insert(snapshot.id, data);
        self.entries.write().insert(snapshot.id, entry);
        Ok(size)
    }

    async fn load(&self, id: SnapshotId) -> Result<Snapshot> {
        let data = self
            .snapshots
            .read()
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::SnapshotNotFound(id.0.to_string()))?;
        self.serializer.deserialize(&data)
    }

    async fn delete(&self, id: SnapshotId) -> Result<()> {
        self.snapshots.write().remove(&id);
        self.entries.write().remove(&id);
        Ok(())
    }

    async fn exists(&self, id: SnapshotId) -> Result<bool> {
        Ok(self.snapshots.read().contains_key(&id))
    }

    async fn list(&self) -> Result<Vec<SnapshotEntry>> {
        Ok(self.entries.read().values().cloned().collect())
    }

    async fn list_for_module(&self, module_hash: &ModuleHash) -> Result<Vec<SnapshotEntry>> {
        Ok(self
            .entries
            .read()
            .values()
            .filter(|e| e.module_hash == *module_hash)
            .cloned()
            .collect())
    }

    async fn stats(&self) -> Result<StorageStats> {
        let entries = self.entries.read();
        let total_size: u64 = entries.values().map(|e| e.size_bytes).sum();
        Ok(StorageStats {
            total_snapshots: entries.len() as u64,
            total_size_bytes: total_size,
            unique_modules: 0,
            total_restores: 0,
        })
    }
}

/// Garbage collector for snapshot storage.
pub struct SnapshotGarbageCollector {
    /// Maximum age for snapshots (snapshots older than this are candidates for deletion).
    max_age: Duration,
    /// Maximum total storage size in bytes.
    max_total_size: u64,
    /// Maximum snapshots per module.
    max_per_module: usize,
    /// Whether to preserve snapshots with children (incremental chains).
    preserve_parents: bool,
}

impl SnapshotGarbageCollector {
    /// Create a new garbage collector with default settings.
    pub fn new() -> Self {
        Self {
            max_age: Duration::from_secs(24 * 3600), // 24 hours
            max_total_size: 10 * 1024 * 1024 * 1024, // 10 GB
            max_per_module: 5,
            preserve_parents: true,
        }
    }

    /// Set maximum snapshot age.
    pub fn with_max_age(mut self, age: Duration) -> Self {
        self.max_age = age;
        self
    }

    /// Set maximum total storage size.
    pub fn with_max_total_size(mut self, size: u64) -> Self {
        self.max_total_size = size;
        self
    }

    /// Set maximum snapshots per module.
    pub fn with_max_per_module(mut self, max: usize) -> Self {
        self.max_per_module = max;
        self
    }

    /// Run garbage collection on the given store.
    pub async fn collect(&self, store: &dyn SnapshotStore) -> Result<GcResult> {
        let entries = store.list().await?;
        let mut to_delete: Vec<SnapshotId> = Vec::new();
        let now = Utc::now();

        // Collect parent IDs that should be preserved
        let parent_ids: std::collections::HashSet<SnapshotId> = if self.preserve_parents {
            entries.iter().filter_map(|e| e.parent_id).collect()
        } else {
            std::collections::HashSet::new()
        };

        // Phase 1: Delete expired snapshots
        for entry in &entries {
            let age =
                now.signed_duration_since(entry.created_at).to_std().unwrap_or(Duration::ZERO);
            if age > self.max_age && !parent_ids.contains(&entry.id) {
                to_delete.push(entry.id);
            }
        }

        // Phase 2: Enforce per-module limits
        let mut by_module: HashMap<ModuleHash, Vec<&SnapshotEntry>> = HashMap::new();
        for entry in &entries {
            by_module.entry(entry.module_hash.clone()).or_default().push(entry);
        }

        for (_module, mut module_entries) in by_module {
            if module_entries.len() > self.max_per_module {
                // Sort by last accessed (oldest first)
                module_entries.sort_by_key(|e| e.last_accessed);
                let to_remove = module_entries.len() - self.max_per_module;
                for entry in module_entries.iter().take(to_remove) {
                    if !parent_ids.contains(&entry.id) && !to_delete.contains(&entry.id) {
                        to_delete.push(entry.id);
                    }
                }
            }
        }

        // Phase 3: Enforce total size limit
        let stats = store.stats().await?;
        if stats.total_size_bytes > self.max_total_size {
            let mut remaining: Vec<&SnapshotEntry> =
                entries.iter().filter(|e| !to_delete.contains(&e.id)).collect();
            remaining.sort_by_key(|e| e.last_accessed);

            let mut current_size = stats.total_size_bytes;
            for entry in &remaining {
                if current_size <= self.max_total_size {
                    break;
                }
                if !parent_ids.contains(&entry.id) {
                    to_delete.push(entry.id);
                    current_size = current_size.saturating_sub(entry.size_bytes);
                }
            }
        }

        // Execute deletions
        let mut deleted = 0;
        let mut freed_bytes = 0u64;
        for id in &to_delete {
            if let Some(entry) = entries.iter().find(|e| e.id == *id) {
                freed_bytes += entry.size_bytes;
            }
            if store.delete(*id).await.is_ok() {
                deleted += 1;
            }
        }

        Ok(GcResult { deleted, freed_bytes, remaining: entries.len() - deleted })
    }
}

impl Default for SnapshotGarbageCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a garbage collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcResult {
    /// Number of snapshots deleted.
    pub deleted: usize,
    /// Bytes freed.
    pub freed_bytes: u64,
    /// Remaining snapshots.
    pub remaining: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxId;
    use crate::snapshot::Snapshot;

    fn make_test_snapshot(module: &str) -> Snapshot {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash(module.to_string());
        let memory = vec![0u8; 65536];
        Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![])
    }

    #[tokio::test]
    async fn test_memory_store() {
        let store = MemoryStore::new();
        let snapshot = make_test_snapshot("test_module");
        let id = snapshot.id;

        // Store
        let size = store.store(&snapshot).await.unwrap();
        assert!(size > 0);

        // Exists
        assert!(store.exists(id).await.unwrap());

        // Load
        let loaded = store.load(id).await.unwrap();
        assert_eq!(loaded.id, id);

        // List
        let entries = store.list().await.unwrap();
        assert_eq!(entries.len(), 1);

        // Delete
        store.delete(id).await.unwrap();
        assert!(!store.exists(id).await.unwrap());
    }

    #[tokio::test]
    async fn test_filesystem_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(dir.path()).unwrap();
        let snapshot = make_test_snapshot("test_module");
        let id = snapshot.id;

        let size = store.store(&snapshot).await.unwrap();
        assert!(size > 0);
        assert!(store.exists(id).await.unwrap());

        let loaded = store.load(id).await.unwrap();
        assert_eq!(loaded.id, id);

        store.delete(id).await.unwrap();
        assert!(!store.exists(id).await.unwrap());
    }

    #[tokio::test]
    async fn test_gc_max_per_module() {
        let store = MemoryStore::new();

        // Store 5 snapshots for the same module
        for _ in 0..5 {
            let snapshot = make_test_snapshot("same_module");
            store.store(&snapshot).await.unwrap();
        }

        let gc = SnapshotGarbageCollector::new().with_max_per_module(3);
        let result = gc.collect(&store).await.unwrap();

        assert_eq!(result.deleted, 2);
        assert_eq!(result.remaining, 3);
    }

    #[tokio::test]
    async fn test_gc_preserves_different_modules() {
        let store = MemoryStore::new();

        store.store(&make_test_snapshot("module_a")).await.unwrap();
        store.store(&make_test_snapshot("module_b")).await.unwrap();
        store.store(&make_test_snapshot("module_c")).await.unwrap();

        let gc = SnapshotGarbageCollector::new()
            .with_max_per_module(5)
            .with_max_age(Duration::from_secs(3600));
        let result = gc.collect(&store).await.unwrap();

        assert_eq!(result.deleted, 0);
        assert_eq!(result.remaining, 3);
    }
}
