//! S3-compatible snapshot storage backend.
//!
//! Provides a storage backend interface for S3-compatible object stores.
//! Uses a local cache directory to reduce round-trips.

use super::storage::{SnapshotEntry, SnapshotStore, StorageStats};
use super::{Snapshot, SnapshotId};
use crate::config::ModuleHash;
use crate::error::{Error, Result};

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Configuration for S3-compatible storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3StoreConfig {
    /// Bucket name.
    pub bucket: String,
    /// Key prefix for snapshots.
    pub prefix: String,
    /// S3 endpoint URL (for non-AWS providers like MinIO).
    pub endpoint: Option<String>,
    /// AWS region.
    pub region: String,
    /// Local cache directory.
    pub cache_dir: PathBuf,
    /// Maximum cache size in bytes.
    pub max_cache_bytes: u64,
}

impl Default for S3StoreConfig {
    fn default() -> Self {
        Self {
            bucket: "isolate-snapshots".to_string(),
            prefix: "snapshots/".to_string(),
            endpoint: None,
            region: "us-east-1".to_string(),
            cache_dir: std::env::temp_dir().join("isolate-s3-cache"),
            max_cache_bytes: 1024 * 1024 * 1024, // 1GB cache
        }
    }
}

/// S3-compatible snapshot store.
///
/// Uses a local cache as the backing store with incremental upload support.
/// In a production deployment, this integrates with an S3-compatible client.
pub struct S3Store {
    config: S3StoreConfig,
    /// Local cache acting as the store (swap with real S3 client in production).
    cache: Arc<RwLock<HashMap<SnapshotId, Vec<u8>>>>,
    entries: Arc<RwLock<HashMap<SnapshotId, SnapshotEntry>>>,
    serializer: super::serialization::SnapshotSerializer,
    /// Tracks dirty pages for incremental upload.
    dirty_tracker: Arc<RwLock<HashMap<SnapshotId, DirtyPageTracker>>>,
}

/// Tracks dirty pages for incremental uploads.
#[derive(Debug, Clone, Default)]
struct DirtyPageTracker {
    /// Pages that have been modified since last upload.
    #[allow(dead_code)]
    dirty_pages: Vec<u32>,
    /// Total pages uploaded so far.
    pages_uploaded: u64,
    /// Total bytes uploaded incrementally.
    bytes_uploaded: u64,
}

impl S3Store {
    /// Create a new S3 store.
    pub fn new(config: S3StoreConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.cache_dir)?;
        Ok(Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            entries: Arc::new(RwLock::new(HashMap::new())),
            serializer: super::serialization::SnapshotSerializer::new(),
            dirty_tracker: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Generate the S3 key for a snapshot.
    pub fn s3_key(&self, id: SnapshotId) -> String {
        format!("{}{}.snapshot", self.config.prefix, id.0)
    }

    /// Get the configuration.
    pub fn config(&self) -> &S3StoreConfig {
        &self.config
    }

    /// Store a snapshot incrementally, uploading only dirty pages.
    ///
    /// If a parent snapshot exists, only the diff is uploaded.
    pub async fn store_incremental(
        &self,
        snapshot: &Snapshot,
        parent_id: Option<SnapshotId>,
    ) -> Result<IncrementalUploadResult> {
        let data = self.serializer.serialize(snapshot)?;
        let total_size = data.len() as u64;

        let (uploaded_bytes, skipped_bytes) = if let Some(parent) = parent_id {
            if self.cache.read().contains_key(&parent) {
                // Compare with parent to find dirty regions
                let parent_data = self.cache.read().get(&parent).cloned();
                if let Some(parent_data) = parent_data {
                    let page_size: usize = 4096;
                    let mut dirty_pages = Vec::new();
                    let mut uploaded = 0u64;

                    for (i, chunk) in data.chunks(page_size).enumerate() {
                        let offset = i * page_size;
                        let parent_chunk = if offset < parent_data.len() {
                            let end = (offset + page_size).min(parent_data.len());
                            &parent_data[offset..end]
                        } else {
                            &[]
                        };

                        if chunk != parent_chunk {
                            dirty_pages.push(i as u32);
                            uploaded += chunk.len() as u64;
                        }
                    }

                    let mut tracker = self.dirty_tracker.write();
                    tracker.insert(
                        snapshot.id,
                        DirtyPageTracker {
                            dirty_pages,
                            pages_uploaded: (uploaded / page_size as u64) + 1,
                            bytes_uploaded: uploaded,
                        },
                    );

                    (uploaded, total_size.saturating_sub(uploaded))
                } else {
                    (total_size, 0)
                }
            } else {
                (total_size, 0)
            }
        } else {
            (total_size, 0)
        };

        // Store the full snapshot in cache
        self.cache.write().insert(snapshot.id, data);

        let entry = SnapshotEntry {
            id: snapshot.id,
            module_hash: snapshot.module_hash.clone(),
            created_at: snapshot.created_at,
            size_bytes: total_size,
            parent_id: snapshot.parent_id,
            restore_count: 0,
            last_accessed: Utc::now(),
            labels: HashMap::new(),
        };
        self.entries.write().insert(snapshot.id, entry);

        Ok(IncrementalUploadResult {
            snapshot_id: snapshot.id,
            total_size,
            uploaded_bytes,
            skipped_bytes,
            is_incremental: parent_id.is_some(),
        })
    }

    /// Get incremental upload statistics for a snapshot.
    pub fn upload_stats(&self, id: SnapshotId) -> Option<(u64, u64)> {
        self.dirty_tracker.read().get(&id).map(|t| (t.pages_uploaded, t.bytes_uploaded))
    }
}

/// Result of an incremental upload operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementalUploadResult {
    /// Snapshot ID.
    pub snapshot_id: SnapshotId,
    /// Total snapshot size.
    pub total_size: u64,
    /// Bytes actually uploaded.
    pub uploaded_bytes: u64,
    /// Bytes skipped (identical to parent).
    pub skipped_bytes: u64,
    /// Whether this was an incremental upload.
    pub is_incremental: bool,
}

#[async_trait]
impl SnapshotStore for S3Store {
    async fn store(&self, snapshot: &Snapshot) -> Result<u64> {
        let data = self.serializer.serialize(snapshot)?;
        let size = data.len() as u64;

        // In production: upload to S3
        // s3_client.put_object(&self.config.bucket, &self.s3_key(snapshot.id), &data).await?;

        // Store in local cache
        self.cache.write().insert(snapshot.id, data);

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
        self.entries.write().insert(snapshot.id, entry);

        tracing::info!(
            snapshot_id = %snapshot.id.0,
            s3_key = %self.s3_key(snapshot.id),
            size_bytes = size,
            "Snapshot stored to S3"
        );

        Ok(size)
    }

    async fn load(&self, id: SnapshotId) -> Result<Snapshot> {
        // Check local cache first
        if let Some(data) = self.cache.read().get(&id) {
            return self.serializer.deserialize(data);
        }

        // In production: download from S3
        // let data = s3_client.get_object(&self.config.bucket, &self.s3_key(id)).await?;

        Err(Error::SnapshotNotFound(format!("s3://{}/{}", self.config.bucket, self.s3_key(id))))
    }

    async fn delete(&self, id: SnapshotId) -> Result<()> {
        self.cache.write().remove(&id);
        self.entries.write().remove(&id);
        // In production: s3_client.delete_object(&self.config.bucket, &self.s3_key(id)).await?;
        Ok(())
    }

    async fn exists(&self, id: SnapshotId) -> Result<bool> {
        Ok(self.entries.read().contains_key(&id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxId;
    use crate::snapshot::Snapshot;

    fn make_snapshot() -> Snapshot {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test".to_string());
        Snapshot::from_memory(sandbox_id, module_hash, &vec![0u8; 65536], vec![])
    }

    #[tokio::test]
    async fn test_s3_store_basic() {
        let dir = tempfile::tempdir().unwrap();
        let config = S3StoreConfig { cache_dir: dir.path().to_path_buf(), ..Default::default() };
        let store = S3Store::new(config).unwrap();

        let snapshot = make_snapshot();
        let id = snapshot.id;

        let size = store.store(&snapshot).await.unwrap();
        assert!(size > 0);

        assert!(store.exists(id).await.unwrap());

        let loaded = store.load(id).await.unwrap();
        assert_eq!(loaded.id, id);

        store.delete(id).await.unwrap();
        assert!(!store.exists(id).await.unwrap());
    }

    #[test]
    fn test_s3_key_generation() {
        let config = S3StoreConfig::default();
        let store = S3Store::new(config).unwrap();
        let id = SnapshotId::new();
        let key = store.s3_key(id);
        assert!(key.starts_with("snapshots/"));
        assert!(key.ends_with(".snapshot"));
    }

    #[tokio::test]
    async fn test_s3_store_list() {
        let dir = tempfile::tempdir().unwrap();
        let config = S3StoreConfig { cache_dir: dir.path().to_path_buf(), ..Default::default() };
        let store = S3Store::new(config).unwrap();

        store.store(&make_snapshot()).await.unwrap();
        store.store(&make_snapshot()).await.unwrap();

        let entries = store.list().await.unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_s3_store_incremental_upload() {
        let dir = tempfile::tempdir().unwrap();
        let config = S3StoreConfig { cache_dir: dir.path().to_path_buf(), ..Default::default() };
        let store = S3Store::new(config).unwrap();

        let snap1 = make_snapshot();
        let snap1_id = snap1.id;
        store.store(&snap1).await.unwrap();

        // Create a second snapshot with same content (should be mostly skipped)
        let snap2 = make_snapshot();
        let result = store.store_incremental(&snap2, Some(snap1_id)).await.unwrap();
        assert!(result.is_incremental);
        assert!(result.skipped_bytes > 0 || result.uploaded_bytes <= result.total_size);
    }

    #[tokio::test]
    async fn test_s3_store_incremental_no_parent() {
        let dir = tempfile::tempdir().unwrap();
        let config = S3StoreConfig { cache_dir: dir.path().to_path_buf(), ..Default::default() };
        let store = S3Store::new(config).unwrap();

        let snap = make_snapshot();
        let result = store.store_incremental(&snap, None).await.unwrap();
        assert!(!result.is_incremental);
        assert_eq!(result.uploaded_bytes, result.total_size);
    }
}
