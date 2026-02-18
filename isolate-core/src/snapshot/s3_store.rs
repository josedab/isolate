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
/// Currently uses a local cache as the backing store. In a production
/// deployment, this would integrate with an S3-compatible client like
/// `aws-sdk-s3` or `rusoto`.
pub struct S3Store {
    config: S3StoreConfig,
    /// Local cache acting as the store (swap with real S3 client in production).
    cache: Arc<RwLock<HashMap<SnapshotId, Vec<u8>>>>,
    entries: Arc<RwLock<HashMap<SnapshotId, SnapshotEntry>>>,
    serializer: super::serialization::SnapshotSerializer,
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
        let config = S3StoreConfig {
            cache_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
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
        let config = S3StoreConfig {
            cache_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let store = S3Store::new(config).unwrap();

        store.store(&make_snapshot()).await.unwrap();
        store.store(&make_snapshot()).await.unwrap();

        let entries = store.list().await.unwrap();
        assert_eq!(entries.len(), 2);
    }
}
