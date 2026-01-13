//! Copy-on-Write memory management for efficient snapshotting.
//!
//! This module provides memory-efficient snapshot storage using
//! copy-on-write semantics, page deduplication, and memory-mapped files.

use super::SnapshotId;
use crate::error::{Error, Result};

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Page hash for deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PageHash([u8; 32]);

impl PageHash {
    /// Compute hash from page data.
    pub fn from_data(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        Self(hash)
    }

    /// Get the hash bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Reference-counted page data for deduplication.
#[derive(Debug)]
struct PageData {
    /// The actual page data.
    data: Vec<u8>,
    /// Reference count.
    ref_count: AtomicUsize,
}

impl PageData {
    fn new(data: Vec<u8>) -> Self {
        Self { data, ref_count: AtomicUsize::new(1) }
    }

    fn increment(&self) {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
    }

    fn decrement(&self) -> usize {
        self.ref_count.fetch_sub(1, Ordering::Relaxed)
    }
}

/// Copy-on-Write memory store with deduplication.
pub struct CowMemoryStore {
    /// Page storage by hash.
    pages: DashMap<PageHash, Arc<PageData>>,
    /// Page size.
    page_size: usize,
    /// Storage directory for overflow.
    storage_path: PathBuf,
    /// Total pages stored.
    total_pages: AtomicUsize,
    /// Total bytes saved by deduplication.
    bytes_saved: AtomicU64,
    /// Maximum in-memory pages before spilling to disk.
    max_memory_pages: usize,
}

impl CowMemoryStore {
    /// Create a new CoW memory store.
    pub fn new(storage_path: PathBuf, page_size: usize, max_memory_pages: usize) -> Result<Self> {
        if !storage_path.exists() {
            std::fs::create_dir_all(&storage_path)?;
        }

        Ok(Self {
            pages: DashMap::new(),
            page_size,
            storage_path,
            total_pages: AtomicUsize::new(0),
            bytes_saved: AtomicU64::new(0),
            max_memory_pages,
        })
    }

    /// Store a page and return its hash.
    pub fn store_page(&self, data: &[u8]) -> PageHash {
        let hash = PageHash::from_data(data);

        // Check if we already have this page
        if let Some(existing) = self.pages.get(&hash) {
            existing.increment();
            self.bytes_saved.fetch_add(data.len() as u64, Ordering::Relaxed);
            return hash;
        }

        // Store new page
        let page_data = Arc::new(PageData::new(data.to_vec()));
        self.pages.insert(hash.clone(), page_data);
        self.total_pages.fetch_add(1, Ordering::Relaxed);

        // Check if we need to spill to disk
        if self.pages.len() > self.max_memory_pages {
            self.spill_to_disk();
        }

        hash
    }

    /// Load a page by hash.
    pub fn load_page(&self, hash: &PageHash) -> Result<Vec<u8>> {
        // Try in-memory first
        if let Some(page) = self.pages.get(hash) {
            return Ok(page.data.clone());
        }

        // Try disk
        self.load_from_disk(hash)
    }

    /// Release a page reference.
    pub fn release_page(&self, hash: &PageHash) {
        if let Some(page) = self.pages.get(hash) {
            if page.decrement() == 1 {
                // Last reference, remove
                drop(page);
                self.pages.remove(hash);
                self.total_pages.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Get storage statistics.
    pub fn stats(&self) -> CowStats {
        CowStats {
            total_pages: self.total_pages.load(Ordering::Relaxed),
            unique_pages: self.pages.len(),
            bytes_saved: self.bytes_saved.load(Ordering::Relaxed),
            page_size: self.page_size,
        }
    }

    fn spill_to_disk(&self) {
        // Find pages with low ref counts to spill
        let mut candidates: Vec<_> = self
            .pages
            .iter()
            .filter(|e| e.ref_count.load(Ordering::Relaxed) == 1)
            .map(|e| (e.key().clone(), e.value().clone()))
            .take(100) // Spill up to 100 pages at a time
            .collect();

        for (hash, page_data) in candidates.drain(..) {
            if self.write_to_disk(&hash, &page_data.data).is_ok() {
                self.pages.remove(&hash);
            }
        }
    }

    fn write_to_disk(&self, hash: &PageHash, data: &[u8]) -> Result<()> {
        let path = self.page_path(hash);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&path)?;
        file.write_all(data)?;
        Ok(())
    }

    fn load_from_disk(&self, hash: &PageHash) -> Result<Vec<u8>> {
        let path = self.page_path(hash);
        let mut file = File::open(&path).map_err(|_| {
            Error::Snapshot(format!("Page not found: {:?}", hex::encode(hash.as_bytes())))
        })?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    }

    fn page_path(&self, hash: &PageHash) -> PathBuf {
        let hex = hex::encode(hash.as_bytes());
        self.storage_path.join(&hex[0..2]).join(&hex[2..4]).join(&hex)
    }
}

/// Statistics for the CoW memory store.
#[derive(Debug, Clone)]
pub struct CowStats {
    /// Total logical pages stored.
    pub total_pages: usize,
    /// Unique physical pages (after deduplication).
    pub unique_pages: usize,
    /// Bytes saved by deduplication.
    pub bytes_saved: u64,
    /// Page size.
    pub page_size: usize,
}

impl CowStats {
    /// Get the deduplication ratio.
    pub fn dedup_ratio(&self) -> f64 {
        if self.total_pages == 0 {
            return 1.0;
        }
        self.unique_pages as f64 / self.total_pages as f64
    }
}

/// A CoW snapshot that shares pages with other snapshots.
#[derive(Debug, Clone)]
pub struct CowSnapshot {
    /// Snapshot ID.
    pub id: SnapshotId,
    /// Page hashes indexed by page number.
    pub page_hashes: HashMap<usize, PageHash>,
    /// Pages that are known to be zero.
    pub zero_pages: Vec<usize>,
    /// Total memory size.
    pub memory_size: usize,
    /// Page size.
    pub page_size: usize,
}

impl CowSnapshot {
    /// Create a CoW snapshot from memory.
    pub fn from_memory(
        id: SnapshotId,
        memory: &[u8],
        page_size: usize,
        store: &CowMemoryStore,
    ) -> Self {
        let mut page_hashes = HashMap::new();
        let mut zero_pages = Vec::new();

        for (page_idx, chunk) in memory.chunks(page_size).enumerate() {
            if chunk.iter().all(|&b| b == 0) {
                zero_pages.push(page_idx);
            } else {
                let hash = store.store_page(chunk);
                page_hashes.insert(page_idx, hash);
            }
        }

        Self { id, page_hashes, zero_pages, memory_size: memory.len(), page_size }
    }

    /// Restore memory from this CoW snapshot.
    pub fn restore_memory(&self, store: &CowMemoryStore) -> Result<Vec<u8>> {
        let mut memory = vec![0u8; self.memory_size];

        for (page_idx, hash) in &self.page_hashes {
            let offset = page_idx * self.page_size;
            if offset >= self.memory_size {
                continue;
            }

            let page_data = store.load_page(hash)?;
            let end = (offset + page_data.len()).min(self.memory_size);
            memory[offset..end].copy_from_slice(&page_data[..end - offset]);
        }

        Ok(memory)
    }

    /// Create a diff between this snapshot and another memory state.
    pub fn diff(&self, new_memory: &[u8], store: &CowMemoryStore) -> CowSnapshotDiff {
        let mut modified_pages = HashMap::new();
        let mut added_pages = HashMap::new();
        let mut removed_pages = Vec::new();

        let num_pages = (new_memory.len() + self.page_size - 1) / self.page_size;

        for page_idx in 0..num_pages {
            let offset = page_idx * self.page_size;
            let end = (offset + self.page_size).min(new_memory.len());
            let new_chunk = &new_memory[offset..end];
            let is_new_zero = new_chunk.iter().all(|&b| b == 0);

            let was_zero = self.zero_pages.contains(&page_idx);
            let old_hash = self.page_hashes.get(&page_idx);

            match (old_hash, was_zero, is_new_zero) {
                // Was zero, still zero
                (None, true, true) => {}
                // Was zero, now has data
                (None, true, false) => {
                    let hash = store.store_page(new_chunk);
                    added_pages.insert(page_idx, hash);
                }
                // Had data, now zero
                (Some(_), false, true) => {
                    removed_pages.push(page_idx);
                }
                // Had data, still has data - check if changed
                (Some(old), false, false) => {
                    let new_hash = PageHash::from_data(new_chunk);
                    if &new_hash != old {
                        let hash = store.store_page(new_chunk);
                        modified_pages.insert(page_idx, hash);
                    }
                }
                _ => {}
            }
        }

        CowSnapshotDiff { base_id: self.id, modified_pages, added_pages, removed_pages }
    }

    /// Apply a diff to create a new snapshot.
    pub fn apply_diff(&self, diff: &CowSnapshotDiff) -> Self {
        let mut page_hashes = self.page_hashes.clone();
        let mut zero_pages = self.zero_pages.clone();

        // Apply modifications
        for (page_idx, hash) in &diff.modified_pages {
            page_hashes.insert(*page_idx, hash.clone());
        }

        // Apply additions
        for (page_idx, hash) in &diff.added_pages {
            page_hashes.insert(*page_idx, hash.clone());
            zero_pages.retain(|&p| p != *page_idx);
        }

        // Apply removals
        for page_idx in &diff.removed_pages {
            page_hashes.remove(page_idx);
            if !zero_pages.contains(page_idx) {
                zero_pages.push(*page_idx);
            }
        }

        Self {
            id: SnapshotId::new(),
            page_hashes,
            zero_pages,
            memory_size: self.memory_size,
            page_size: self.page_size,
        }
    }

    /// Get the number of stored pages.
    pub fn page_count(&self) -> usize {
        self.page_hashes.len()
    }

    /// Get the approximate size in bytes.
    pub fn size(&self) -> usize {
        self.page_hashes.len() * self.page_size
    }
}

/// A diff between two CoW snapshots.
#[derive(Debug, Clone)]
pub struct CowSnapshotDiff {
    /// Base snapshot ID.
    pub base_id: SnapshotId,
    /// Modified pages.
    pub modified_pages: HashMap<usize, PageHash>,
    /// Added pages.
    pub added_pages: HashMap<usize, PageHash>,
    /// Removed pages (became zero).
    pub removed_pages: Vec<usize>,
}

impl CowSnapshotDiff {
    /// Get the number of changes.
    pub fn change_count(&self) -> usize {
        self.modified_pages.len() + self.added_pages.len() + self.removed_pages.len()
    }

    /// Check if the diff is empty.
    pub fn is_empty(&self) -> bool {
        self.change_count() == 0
    }
}

/// Snapshot version tracker for maintaining snapshot history.
#[derive(Debug)]
pub struct SnapshotVersioner {
    /// Current version.
    current_version: AtomicU64,
    /// Version history.
    history: DashMap<u64, SnapshotId>,
    /// Maximum history size.
    max_history: usize,
}

impl SnapshotVersioner {
    /// Create a new versioner.
    pub fn new(max_history: usize) -> Self {
        Self { current_version: AtomicU64::new(0), history: DashMap::new(), max_history }
    }

    /// Record a new version.
    pub fn record(&self, snapshot_id: SnapshotId) -> u64 {
        let version = self.current_version.fetch_add(1, Ordering::SeqCst);
        self.history.insert(version, snapshot_id);

        // Prune old versions - keep only the last max_history versions
        // If we have version 4 and max_history is 3, keep versions 2, 3, 4
        if version >= self.max_history as u64 {
            let oldest_to_keep = version - self.max_history as u64 + 1;
            self.history.retain(|&v, _| v >= oldest_to_keep);
        }

        version
    }

    /// Get snapshot ID for a version.
    pub fn get(&self, version: u64) -> Option<SnapshotId> {
        self.history.get(&version).map(|v| *v)
    }

    /// Get the current version.
    pub fn current_version(&self) -> u64 {
        self.current_version.load(Ordering::SeqCst)
    }

    /// Get all versions.
    pub fn versions(&self) -> Vec<u64> {
        self.history.iter().map(|e| *e.key()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (CowMemoryStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = CowMemoryStore::new(
            temp_dir.path().to_path_buf(),
            4096, // 4KB pages for testing
            1000,
        )
        .unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_page_hash() {
        let data1 = b"hello world";
        let data2 = b"hello world";
        let data3 = b"different data";

        let hash1 = PageHash::from_data(data1);
        let hash2 = PageHash::from_data(data2);
        let hash3 = PageHash::from_data(data3);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_cow_store_deduplication() {
        let (store, _temp) = create_test_store();

        let data = vec![1u8; 4096];

        // Store same page twice
        let hash1 = store.store_page(&data);
        let hash2 = store.store_page(&data);

        assert_eq!(hash1, hash2);

        let stats = store.stats();
        assert_eq!(stats.unique_pages, 1);
        assert_eq!(stats.bytes_saved, 4096);
    }

    #[test]
    fn test_cow_snapshot_creation() {
        let (store, _temp) = create_test_store();

        // Create memory with some data
        let mut memory = vec![0u8; 16384]; // 4 pages
        memory[0..4].copy_from_slice(b"page");
        memory[4096..4100].copy_from_slice(b"two!");

        let snapshot = CowSnapshot::from_memory(SnapshotId::new(), &memory, 4096, &store);

        assert_eq!(snapshot.page_count(), 2);
        assert_eq!(snapshot.zero_pages.len(), 2);
    }

    #[test]
    fn test_cow_snapshot_restore() {
        let (store, _temp) = create_test_store();

        let mut memory = vec![0u8; 8192];
        memory[0..5].copy_from_slice(b"hello");
        memory[4096..4101].copy_from_slice(b"world");

        let snapshot = CowSnapshot::from_memory(SnapshotId::new(), &memory, 4096, &store);

        let restored = snapshot.restore_memory(&store).unwrap();
        assert_eq!(restored, memory);
    }

    #[test]
    fn test_cow_snapshot_diff() {
        let (store, _temp) = create_test_store();

        // Original memory
        let mut memory1 = vec![0u8; 8192];
        memory1[0..4].copy_from_slice(b"test");

        let snapshot1 = CowSnapshot::from_memory(SnapshotId::new(), &memory1, 4096, &store);

        // Modified memory
        let mut memory2 = memory1.clone();
        memory2[0..7].copy_from_slice(b"changed");
        memory2[4096..4100].copy_from_slice(b"new!");

        let diff = snapshot1.diff(&memory2, &store);

        assert_eq!(diff.modified_pages.len(), 1); // First page modified
        assert_eq!(diff.added_pages.len(), 1); // Second page added
        assert!(diff.removed_pages.is_empty());
    }

    #[test]
    fn test_cow_snapshot_apply_diff() {
        let (store, _temp) = create_test_store();

        // Original memory
        let mut memory1 = vec![0u8; 8192];
        memory1[0..4].copy_from_slice(b"test");

        let snapshot1 = CowSnapshot::from_memory(SnapshotId::new(), &memory1, 4096, &store);

        // Modified memory
        let mut memory2 = memory1.clone();
        memory2[0..7].copy_from_slice(b"changed");

        let diff = snapshot1.diff(&memory2, &store);
        let snapshot2 = snapshot1.apply_diff(&diff);

        let restored = snapshot2.restore_memory(&store).unwrap();
        assert_eq!(&restored[0..7], b"changed");
    }

    #[test]
    fn test_snapshot_versioner() {
        let versioner = SnapshotVersioner::new(5);

        let id1 = SnapshotId::new();
        let id2 = SnapshotId::new();

        let v1 = versioner.record(id1);
        let v2 = versioner.record(id2);

        assert_eq!(v1, 0);
        assert_eq!(v2, 1);
        assert_eq!(versioner.get(0), Some(id1));
        assert_eq!(versioner.get(1), Some(id2));
    }

    #[test]
    fn test_snapshot_versioner_pruning() {
        let versioner = SnapshotVersioner::new(3);

        // Add 5 versions
        for _ in 0..5 {
            versioner.record(SnapshotId::new());
        }

        // Should only have last 3
        assert!(versioner.get(0).is_none());
        assert!(versioner.get(1).is_none());
        assert!(versioner.get(2).is_some());
        assert!(versioner.get(3).is_some());
        assert!(versioner.get(4).is_some());
    }
}
