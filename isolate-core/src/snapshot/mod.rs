//! Snapshot engine for fast sandbox state save/restore.
//!
//! This module provides functionality for:
//! - Creating snapshots of sandbox state (memory, globals, tables)
//! - Restoring sandboxes from snapshots with sub-millisecond warm starts
//! - Managing warm pools of pre-initialized sandboxes
//! - Copy-on-write memory optimization for memory-efficient snapshotting
//! - Incremental snapshots with page-level deduplication
//! - Snapshot serialization and persistence
//!
//! # Example
//!
//! ```rust,ignore
//! // Create a snapshot after initialization
//! let snapshot = sandbox.snapshot().await?;
//!
//! // Store in the snapshot engine
//! let snapshot_id = snapshot_engine.store(snapshot)?;
//!
//! // Later, restore from snapshot for instant cold start
//! let mut sandbox2 = Sandbox::restore(snapshot_id, &snapshot_engine, config).await?;
//! ```
//!
//! # Copy-on-Write Snapshots
//!
//! For efficient memory usage with many similar snapshots:
//!
//! ```rust,ignore
//! use isolate_core::snapshot::cow::{CowMemoryStore, CowSnapshot};
//!
//! // Create a CoW store
//! let store = CowMemoryStore::new(path, 65536, 10000)?;
//!
//! // Create a CoW snapshot (pages are deduplicated)
//! let snapshot = CowSnapshot::from_memory(id, &memory, 65536, &store);
//!
//! // Restore efficiently
//! let restored = snapshot.restore_memory(&store)?;
//! ```

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

pub mod auto_warm;
pub mod checkout_pool;
pub mod checkpoint;
pub mod clone_pool;
pub mod cow;
pub mod live_migration;
pub mod manager;
pub mod orchestrator;
mod pool;
pub mod s3_store;
pub mod instant_clone;
pub mod serialization;
pub mod storage;

pub use auto_warm::{AccessTracker, AutoWarmConfig, WarmingRecommendation};
pub use instant_clone::{
    InstantCloneEngine, CloneTemplate, CloneInstance, CrossNodeRestore, DiskSnapshotPersistence,
};
pub use live_migration::{
    FailoverPolicy, FailoverRegistry, FrozenState, LiveMigration, LiveMigrationConfig,
    LiveMigrationState, MigrationProgress,
};
pub use clone_pool::{ClonePool, ClonePoolConfig, ClonePoolStats};
pub use cow::{
    CowMemoryStore, CowSnapshot, CowSnapshotDiff, CowStats, PageHash, SnapshotVersioner,
};
pub use manager::{
    GcResult, RestoredState, SnapshotInfo, SnapshotManager, SnapshotManagerConfig,
    SnapshotManagerStats,
};
pub use pool::{WarmPool, WarmPoolConfig, WarmPoolStats};
pub use serialization::{SnapshotSerializer, SnapshotWriter};

use crate::config::ModuleHash;
use crate::error::{Error, Result};
use crate::sandbox::SandboxId;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub Uuid);

impl SnapshotId {
    /// Create a new random snapshot ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SnapshotId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Memory page state for copy-on-write optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryPage {
    /// Page is all zeros (no need to store).
    Zero,
    /// Page has data.
    Data(Vec<u8>),
    /// Page references a parent snapshot (for incremental snapshots).
    Reference { parent_id: SnapshotId, page_index: usize },
}

/// Global variable value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GlobalValue {
    I32(i32),
    I64(i64),
    F32(u32), // Store as bits for exact representation
    F64(u64), // Store as bits for exact representation
    V128([u8; 16]),
    FuncRef(Option<u32>),
    ExternRef(Option<u32>),
}

/// A snapshot of sandbox state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Snapshot ID.
    pub id: SnapshotId,
    /// Original sandbox ID.
    pub sandbox_id: SandboxId,
    /// Module hash.
    pub module_hash: ModuleHash,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Memory pages (sparse representation for CoW).
    pub memory_pages: HashMap<usize, MemoryPage>,
    /// Total memory size in bytes.
    pub memory_size: usize,
    /// Page size used for this snapshot.
    pub page_size: usize,
    /// Global variable values.
    pub globals: Vec<GlobalValue>,
    /// Table entries (function references).
    pub tables: HashMap<String, Vec<Option<u32>>>,
    /// Fuel remaining at snapshot time.
    pub fuel_remaining: Option<u64>,
    /// Checksum of the memory for integrity verification.
    pub memory_checksum: String,
    /// Parent snapshot (for incremental snapshots).
    pub parent_id: Option<SnapshotId>,
    /// Snapshot metadata.
    pub metadata: SnapshotMetadata,
}

/// Metadata about a snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Human-readable label.
    pub label: Option<String>,
    /// Tags for categorization.
    pub tags: Vec<String>,
    /// Custom key-value data.
    pub custom: HashMap<String, String>,
    /// Execution count at snapshot time.
    pub execution_count: u64,
    /// Total CPU time used before snapshot.
    pub cpu_time_used: std::time::Duration,
}

impl Snapshot {
    /// Default page size (64KB - same as WASM page).
    pub const DEFAULT_PAGE_SIZE: usize = 65536;

    /// Create a new empty snapshot.
    pub fn new(sandbox_id: SandboxId, module_hash: ModuleHash) -> Self {
        Self {
            id: SnapshotId::new(),
            sandbox_id,
            module_hash,
            created_at: Utc::now(),
            memory_pages: HashMap::new(),
            memory_size: 0,
            page_size: Self::DEFAULT_PAGE_SIZE,
            globals: Vec::new(),
            tables: HashMap::new(),
            fuel_remaining: None,
            memory_checksum: String::new(),
            parent_id: None,
            metadata: SnapshotMetadata::default(),
        }
    }

    /// Create a snapshot from raw memory data.
    pub fn from_memory(
        sandbox_id: SandboxId,
        module_hash: ModuleHash,
        memory: &[u8],
        globals: Vec<GlobalValue>,
    ) -> Self {
        let page_size = Self::DEFAULT_PAGE_SIZE;
        let mut memory_pages = HashMap::new();

        // Split memory into pages, using sparse representation
        for (page_idx, chunk) in memory.chunks(page_size).enumerate() {
            if chunk.iter().all(|&b| b == 0) {
                // All zeros - mark as zero page (saves memory)
                memory_pages.insert(page_idx, MemoryPage::Zero);
            } else {
                memory_pages.insert(page_idx, MemoryPage::Data(chunk.to_vec()));
            }
        }

        // Calculate checksum
        let mut hasher = Sha256::new();
        hasher.update(memory);
        let checksum = hex::encode(hasher.finalize());

        Self {
            id: SnapshotId::new(),
            sandbox_id,
            module_hash,
            created_at: Utc::now(),
            memory_pages,
            memory_size: memory.len(),
            page_size,
            globals,
            tables: HashMap::new(),
            fuel_remaining: None,
            memory_checksum: checksum,
            parent_id: None,
            metadata: SnapshotMetadata::default(),
        }
    }

    /// Create an incremental snapshot from a parent.
    pub fn incremental(
        sandbox_id: SandboxId,
        module_hash: ModuleHash,
        parent: &Snapshot,
        memory: &[u8],
        globals: Vec<GlobalValue>,
    ) -> Self {
        let page_size = parent.page_size;
        let mut memory_pages = HashMap::new();

        // Compare with parent and only store changed pages
        for (page_idx, chunk) in memory.chunks(page_size).enumerate() {
            let parent_page = parent.memory_pages.get(&page_idx);

            let is_changed = match parent_page {
                Some(MemoryPage::Zero) => !chunk.iter().all(|&b| b == 0),
                Some(MemoryPage::Data(parent_data)) => chunk != parent_data.as_slice(),
                Some(MemoryPage::Reference { .. }) => true, // Always store if parent has reference
                None => !chunk.iter().all(|&b| b == 0),
            };

            if is_changed {
                if chunk.iter().all(|&b| b == 0) {
                    memory_pages.insert(page_idx, MemoryPage::Zero);
                } else {
                    memory_pages.insert(page_idx, MemoryPage::Data(chunk.to_vec()));
                }
            } else {
                // Reference parent page
                memory_pages.insert(
                    page_idx,
                    MemoryPage::Reference { parent_id: parent.id, page_index: page_idx },
                );
            }
        }

        // Calculate checksum
        let mut hasher = Sha256::new();
        hasher.update(memory);
        let checksum = hex::encode(hasher.finalize());

        Self {
            id: SnapshotId::new(),
            sandbox_id,
            module_hash,
            created_at: Utc::now(),
            memory_pages,
            memory_size: memory.len(),
            page_size,
            globals,
            tables: HashMap::new(),
            fuel_remaining: None,
            memory_checksum: checksum,
            parent_id: Some(parent.id),
            metadata: SnapshotMetadata::default(),
        }
    }

    /// Restore full memory from this snapshot.
    /// For incremental snapshots, this requires the snapshot engine to resolve references.
    pub fn restore_memory(&self, snapshot_engine: Option<&SnapshotEngine>) -> Result<Vec<u8>> {
        let mut memory = vec![0u8; self.memory_size];

        for (page_idx, page) in &self.memory_pages {
            let offset = page_idx * self.page_size;
            if offset >= self.memory_size {
                continue;
            }

            let page_data = match page {
                MemoryPage::Zero => continue, // Already zero
                MemoryPage::Data(data) => data.clone(),
                MemoryPage::Reference { parent_id, page_index } => {
                    // Resolve from parent snapshot
                    let engine = snapshot_engine.ok_or_else(|| {
                        Error::Snapshot("Snapshot engine required for incremental restore".into())
                    })?;
                    let parent = engine.load(parent_id)?;
                    let parent_page = parent.memory_pages.get(page_index).ok_or_else(|| {
                        Error::Snapshot(format!("Parent page {} not found", page_index))
                    })?;

                    match parent_page {
                        MemoryPage::Zero => continue,
                        MemoryPage::Data(data) => data.clone(),
                        MemoryPage::Reference { .. } => {
                            // Recursively resolve (should be rare)
                            let parent_memory = parent.restore_memory(Some(engine))?;
                            let start = page_index * self.page_size;
                            let end = (start + self.page_size).min(parent_memory.len());
                            parent_memory[start..end].to_vec()
                        }
                    }
                }
            };

            let end = (offset + page_data.len()).min(self.memory_size);
            memory[offset..end].copy_from_slice(&page_data[..end - offset]);
        }

        // Verify checksum
        let mut hasher = Sha256::new();
        hasher.update(&memory);
        let checksum = hex::encode(hasher.finalize());
        if checksum != self.memory_checksum {
            return Err(Error::Snapshot(format!(
                "Memory checksum mismatch: expected {}, got {}",
                self.memory_checksum, checksum
            )));
        }

        Ok(memory)
    }

    /// Get the total size of the snapshot data (approximate, for storage management).
    pub fn size(&self) -> usize {
        let page_data_size: usize = self
            .memory_pages
            .values()
            .map(|p| match p {
                MemoryPage::Zero => 0,
                MemoryPage::Data(d) => d.len(),
                MemoryPage::Reference { .. } => 16, // Just the reference size
            })
            .sum();

        page_data_size + self.globals.len() * 16 // Rough estimate for globals
    }

    /// Get the compression ratio (stored size / original memory size).
    pub fn compression_ratio(&self) -> f64 {
        if self.memory_size == 0 {
            return 1.0;
        }
        self.size() as f64 / self.memory_size as f64
    }

    /// Check if this is an incremental snapshot.
    pub fn is_incremental(&self) -> bool {
        self.parent_id.is_some()
    }

    /// Set metadata label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.metadata.label = Some(label.into());
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.metadata.tags.push(tag.into());
        self
    }
}

/// Configuration for the snapshot engine.
#[derive(Debug, Clone)]
pub struct SnapshotEngineConfig {
    /// Storage directory for snapshots.
    pub storage_path: PathBuf,
    /// Maximum number of snapshots to keep per module.
    pub max_snapshots_per_module: usize,
    /// Maximum total storage size in bytes.
    pub max_storage_size: usize,
    /// Enable copy-on-write memory mapping.
    pub enable_cow: bool,
}

impl Default for SnapshotEngineConfig {
    fn default() -> Self {
        Self {
            storage_path: std::env::temp_dir().join("isolate-snapshots"),
            max_snapshots_per_module: 10,
            max_storage_size: 1024 * 1024 * 1024, // 1GB
            enable_cow: true,
        }
    }
}

/// Engine for managing snapshots.
pub struct SnapshotEngine {
    config: SnapshotEngineConfig,
    snapshots: dashmap::DashMap<SnapshotId, Snapshot>,
    by_module: dashmap::DashMap<ModuleHash, Vec<SnapshotId>>,
}

impl SnapshotEngine {
    /// Create a new snapshot engine.
    pub fn new(config: SnapshotEngineConfig) -> Result<Self> {
        // Create storage directory if it doesn't exist
        if !config.storage_path.exists() {
            std::fs::create_dir_all(&config.storage_path)?;
        }

        Ok(Self { config, snapshots: dashmap::DashMap::new(), by_module: dashmap::DashMap::new() })
    }

    /// Create a snapshot with default configuration.
    pub fn with_defaults() -> Result<Self> {
        Self::new(SnapshotEngineConfig::default())
    }

    /// Store a snapshot.
    pub fn store(&self, snapshot: Snapshot) -> Result<SnapshotId> {
        let id = snapshot.id;
        let module_hash = snapshot.module_hash.clone();

        // Check storage limits
        let current_count = self.by_module.get(&module_hash).map(|v| v.len()).unwrap_or(0);

        if current_count >= self.config.max_snapshots_per_module {
            // Remove oldest snapshot for this module
            if let Some(mut ids) = self.by_module.get_mut(&module_hash) {
                if let Some(old_id) = ids.first().cloned() {
                    self.snapshots.remove(&old_id);
                    ids.remove(0);
                }
            }
        }

        // Store snapshot
        self.snapshots.insert(id, snapshot);
        self.by_module.entry(module_hash).or_default().push(id);

        tracing::debug!(snapshot_id = %id, "Snapshot stored");
        Ok(id)
    }

    /// Load a snapshot by ID.
    pub fn load(&self, id: &SnapshotId) -> Result<Snapshot> {
        self.snapshots
            .get(id)
            .map(|s| s.clone())
            .ok_or_else(|| Error::SnapshotNotFound(id.to_string()))
    }

    /// Get snapshots for a module.
    pub fn get_for_module(&self, module_hash: &ModuleHash) -> Vec<SnapshotId> {
        self.by_module.get(module_hash).map(|v| v.clone()).unwrap_or_default()
    }

    /// Remove a snapshot.
    pub fn remove(&self, id: &SnapshotId) -> Result<()> {
        if let Some((_, snapshot)) = self.snapshots.remove(id) {
            if let Some(mut ids) = self.by_module.get_mut(&snapshot.module_hash) {
                ids.retain(|i| i != id);
            }
            tracing::debug!(snapshot_id = %id, "Snapshot removed");
        }
        Ok(())
    }

    /// Get the number of stored snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Get total storage size.
    pub fn storage_size(&self) -> usize {
        self.snapshots.iter().map(|s| s.size()).sum()
    }

    /// Clear all snapshots.
    pub fn clear(&self) {
        self.snapshots.clear();
        self.by_module.clear();
    }
}

impl Default for SnapshotEngine {
    fn default() -> Self {
        Self::with_defaults().expect("Failed to create default snapshot engine")
    }
}

/// Snapshot restore options.
#[derive(Debug, Clone, Default)]
pub struct RestoreOptions {
    /// Reset resource meters after restore.
    pub reset_meters: bool,
    /// Assign a new sandbox ID.
    pub new_sandbox_id: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxId;

    #[test]
    fn test_snapshot_id() {
        let id1 = SnapshotId::new();
        let id2 = SnapshotId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_snapshot_creation() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        let snapshot = Snapshot::new(sandbox_id, module_hash.clone());

        assert_eq!(snapshot.sandbox_id, sandbox_id);
        assert_eq!(snapshot.module_hash, module_hash);
        assert_eq!(snapshot.size(), 0);
    }

    #[test]
    fn test_snapshot_from_memory() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        // Create memory with some data
        let mut memory = vec![0u8; 128 * 1024]; // 128KB (2 pages)
        memory[0..4].copy_from_slice(b"test");
        memory[65536..65540].copy_from_slice(b"data");

        let snapshot = Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![]);

        // Should have 2 pages with data
        assert_eq!(snapshot.memory_size, 128 * 1024);
        assert_eq!(snapshot.memory_pages.len(), 2);

        // Verify we can restore
        let restored = snapshot.restore_memory(None).unwrap();
        assert_eq!(restored, memory);
    }

    #[test]
    fn test_snapshot_zero_page_compression() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        // Create mostly-zero memory
        let memory = vec![0u8; 1024 * 1024]; // 1MB of zeros

        let snapshot = Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![]);

        // All pages should be marked as zero
        for page in snapshot.memory_pages.values() {
            assert!(matches!(page, MemoryPage::Zero));
        }

        // Size should be very small (just metadata)
        assert!(snapshot.size() < 1000);
        assert!(snapshot.compression_ratio() < 0.01);
    }

    #[test]
    fn test_snapshot_incremental() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        // Create parent snapshot
        let mut memory1 = vec![0u8; 128 * 1024];
        memory1[0..4].copy_from_slice(b"test");
        let parent = Snapshot::from_memory(sandbox_id, module_hash.clone(), &memory1, vec![]);

        // Create incremental with only small change
        let mut memory2 = memory1.clone();
        memory2[65536..65540].copy_from_slice(b"new!");

        let incremental = Snapshot::incremental(sandbox_id, module_hash, &parent, &memory2, vec![]);

        assert!(incremental.is_incremental());
        assert_eq!(incremental.parent_id, Some(parent.id));

        // Should have references for unchanged pages
        let has_reference =
            incremental.memory_pages.values().any(|p| matches!(p, MemoryPage::Reference { .. }));
        assert!(has_reference);
    }

    #[test]
    fn test_snapshot_metadata() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        let snapshot = Snapshot::new(sandbox_id, module_hash)
            .with_label("init-state")
            .with_tag("production")
            .with_tag("v1.0");

        assert_eq!(snapshot.metadata.label, Some("init-state".to_string()));
        assert_eq!(snapshot.metadata.tags.len(), 2);
    }

    #[test]
    fn test_snapshot_engine() {
        let config = SnapshotEngineConfig {
            storage_path: std::env::temp_dir().join("isolate-test-snapshots"),
            max_snapshots_per_module: 2,
            ..Default::default()
        };

        let engine = SnapshotEngine::new(config).unwrap();
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        // Store snapshots
        let s1 = Snapshot::new(sandbox_id, module_hash.clone());
        let id1 = engine.store(s1).unwrap();

        let s2 = Snapshot::new(sandbox_id, module_hash.clone());
        let id2 = engine.store(s2).unwrap();

        assert_eq!(engine.snapshot_count(), 2);

        // Store a third - should remove the first
        let s3 = Snapshot::new(sandbox_id, module_hash.clone());
        engine.store(s3).unwrap();

        assert_eq!(engine.snapshot_count(), 2);
        assert!(engine.load(&id1).is_err()); // First one should be gone
        assert!(engine.load(&id2).is_ok());
    }

    #[test]
    fn test_snapshot_engine_get_for_module() {
        let engine = SnapshotEngine::with_defaults().unwrap();
        let sandbox_id = SandboxId::new();

        let hash1 = ModuleHash("module1".to_string());
        let hash2 = ModuleHash("module2".to_string());

        engine.store(Snapshot::new(sandbox_id, hash1.clone())).unwrap();
        engine.store(Snapshot::new(sandbox_id, hash1.clone())).unwrap();
        engine.store(Snapshot::new(sandbox_id, hash2.clone())).unwrap();

        assert_eq!(engine.get_for_module(&hash1).len(), 2);
        assert_eq!(engine.get_for_module(&hash2).len(), 1);
    }

    #[test]
    fn test_global_value_types() {
        let globals = vec![
            GlobalValue::I32(42),
            GlobalValue::I64(12345678901234),
            GlobalValue::F32(1.5f32.to_bits()),
            GlobalValue::F64(1.23456f64.to_bits()),
        ];

        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        let memory = vec![0u8; 65536];

        let snapshot = Snapshot::from_memory(sandbox_id, module_hash, &memory, globals.clone());
        assert_eq!(snapshot.globals.len(), 4);
    }
}
