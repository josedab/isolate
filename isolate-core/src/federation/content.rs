//! Content-addressed storage using SHA-256.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

/// Content identifier (SHA-256 hash of content).
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentId(String);

impl ContentId {
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        Self(hex::encode(hasher.finalize()))
    }

    pub fn new(cid: impl Into<String>) -> Self {
        Self(cid.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A stored module with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredModule {
    pub cid: ContentId,
    pub name: String,
    pub size_bytes: usize,
    pub stored_at: u64,
}

/// In-memory content-addressed store.
#[derive(Clone)]
pub struct ContentStore {
    inner: Arc<ContentStoreInner>,
}

struct ContentStoreInner {
    modules: RwLock<HashMap<ContentId, (Vec<u8>, StoredModule)>>,
}

impl ContentStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ContentStoreInner {
                modules: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Store content and return its CID.
    pub fn store(&self, data: &[u8], name: &str) -> ContentId {
        let cid = ContentId::from_bytes(data);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let meta = StoredModule {
            cid: cid.clone(),
            name: name.to_string(),
            size_bytes: data.len(),
            stored_at: ts,
        };

        self.inner.modules.write().insert(cid.clone(), (data.to_vec(), meta));
        cid
    }

    /// Retrieve content by CID.
    pub fn retrieve(&self, cid: &ContentId) -> Option<Vec<u8>> {
        self.inner.modules.read().get(cid).map(|(data, _)| data.clone())
    }

    /// Get metadata for a CID.
    pub fn metadata(&self, cid: &ContentId) -> Option<StoredModule> {
        self.inner.modules.read().get(cid).map(|(_, meta)| meta.clone())
    }

    /// Check if content exists.
    pub fn contains(&self, cid: &ContentId) -> bool {
        self.inner.modules.read().contains_key(cid)
    }

    /// Remove content.
    pub fn remove(&self, cid: &ContentId) -> bool {
        self.inner.modules.write().remove(cid).is_some()
    }

    /// List all stored CIDs.
    pub fn list(&self) -> Vec<ContentId> {
        self.inner.modules.read().keys().cloned().collect()
    }

    /// Total stored size in bytes.
    pub fn total_size(&self) -> usize {
        self.inner.modules.read().values().map(|(data, _)| data.len()).sum()
    }

    pub fn count(&self) -> usize {
        self.inner.modules.read().len()
    }
}

impl Default for ContentStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Verify that data matches its CID.
pub fn verify_integrity(cid: &ContentId, data: &[u8]) -> bool {
    let computed = ContentId::from_bytes(data);
    *cid == computed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_id_deterministic() {
        let cid1 = ContentId::from_bytes(b"hello world");
        let cid2 = ContentId::from_bytes(b"hello world");
        assert_eq!(cid1, cid2);
        assert_eq!(cid1.as_str().len(), 64);
    }

    #[test]
    fn test_different_content_different_cid() {
        let cid1 = ContentId::from_bytes(b"hello");
        let cid2 = ContentId::from_bytes(b"world");
        assert_ne!(cid1, cid2);
    }

    #[test]
    fn test_store_and_retrieve() {
        let store = ContentStore::new();
        let data = b"(module binary)";
        let cid = store.store(data, "test.wasm");

        let retrieved = store.retrieve(&cid).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_metadata() {
        let store = ContentStore::new();
        let cid = store.store(b"data", "mod.wasm");
        let meta = store.metadata(&cid).unwrap();
        assert_eq!(meta.name, "mod.wasm");
        assert_eq!(meta.size_bytes, 4);
    }

    #[test]
    fn test_integrity_verification() {
        let data = b"module content";
        let cid = ContentId::from_bytes(data);
        assert!(verify_integrity(&cid, data));
        assert!(!verify_integrity(&cid, b"tampered"));
    }

    #[test]
    fn test_remove_content() {
        let store = ContentStore::new();
        let cid = store.store(b"data", "x.wasm");
        assert!(store.contains(&cid));
        assert!(store.remove(&cid));
        assert!(!store.contains(&cid));
    }

    #[test]
    fn test_total_size() {
        let store = ContentStore::new();
        store.store(b"aaa", "a.wasm");
        store.store(b"bbbbb", "b.wasm");
        assert_eq!(store.total_size(), 8);
        assert_eq!(store.count(), 2);
    }
}
