//! KV store implementation.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// KV store error types.
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    /// Key not found.
    #[error("Key not found: {0}")]
    NotFound(String),

    /// Namespace storage quota exceeded.
    #[error("Namespace quota exceeded: used {used} bytes, limit {limit} bytes")]
    QuotaExceeded { used: usize, limit: usize },

    /// Key size exceeds maximum.
    #[error("Key too large: {size} bytes, max {max} bytes")]
    KeyTooLarge { size: usize, max: usize },

    /// Value size exceeds maximum.
    #[error("Value too large: {size} bytes, max {max} bytes")]
    ValueTooLarge { size: usize, max: usize },

    /// Compare-and-swap version mismatch.
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u64, actual: u64 },

    /// Namespace not found.
    #[error("Namespace not found: {0}")]
    NamespaceNotFound(String),

    /// Maximum number of keys exceeded.
    #[error("Maximum key count exceeded: limit {0}")]
    MaxKeysExceeded(usize),
}

/// Configuration for the KV store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvConfig {
    /// Maximum total storage per namespace in bytes.
    pub max_namespace_bytes: usize,
    /// Maximum key size in bytes.
    pub max_key_size: usize,
    /// Maximum value size in bytes.
    pub max_value_size: usize,
    /// Maximum number of keys per namespace.
    pub max_keys_per_namespace: usize,
    /// Default TTL for entries (None = no expiration).
    pub default_ttl: Option<Duration>,
    /// How often to run eviction of expired entries.
    pub eviction_interval: Duration,
}

impl Default for KvConfig {
    fn default() -> Self {
        Self {
            max_namespace_bytes: 64 * 1024 * 1024, // 64 MB per namespace
            max_key_size: 1024,                    // 1 KB keys
            max_value_size: 1024 * 1024,           // 1 MB values
            max_keys_per_namespace: 10_000,
            default_ttl: None,
            eviction_interval: Duration::from_secs(60),
        }
    }
}

/// Options for set operations.
#[derive(Debug, Clone, Default)]
pub struct SetOptions {
    /// Time-to-live for this entry.
    pub ttl: Option<Duration>,
    /// Only set if the current version matches (compare-and-swap).
    pub expected_version: Option<u64>,
    /// Only set if the key does not already exist.
    pub if_not_exists: bool,
}

/// Unique namespace identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NamespaceId(String);

impl NamespaceId {
    /// Create a new namespace ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the namespace ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NamespaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A stored key-value entry.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The key.
    key: String,
    /// The value data.
    data: Vec<u8>,
    /// Entry version (incremented on each update).
    version: u64,
    /// When the entry was created.
    created_at: Instant,
    /// When the entry was last updated.
    updated_at: Instant,
    /// When the entry expires (None = never).
    expires_at: Option<Instant>,
}

impl Entry {
    /// Get the key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Get the value data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get the entry version.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Check if the entry has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at.map(|exp| Instant::now() >= exp).unwrap_or(false)
    }

    /// Get time-to-live remaining.
    pub fn ttl_remaining(&self) -> Option<Duration> {
        self.expires_at.and_then(|exp| {
            let now = Instant::now();
            if now < exp {
                Some(exp - now)
            } else {
                None
            }
        })
    }

    /// Get the size in bytes (key + value).
    pub fn size_bytes(&self) -> usize {
        self.key.len() + self.data.len()
    }
}

/// Statistics for a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceStats {
    /// Total number of keys.
    pub key_count: usize,
    /// Total bytes used (keys + values).
    pub total_bytes: usize,
    /// Number of expired keys awaiting eviction.
    pub expired_count: usize,
    /// Total get operations.
    pub get_ops: u64,
    /// Total set operations.
    pub set_ops: u64,
    /// Total delete operations.
    pub delete_ops: u64,
}

/// Internal namespace storage.
struct NamespaceInner {
    id: NamespaceId,
    entries: HashMap<String, Entry>,
    config: KvConfig,
    total_bytes: usize,
    get_ops: u64,
    set_ops: u64,
    delete_ops: u64,
}

impl NamespaceInner {
    fn new(id: NamespaceId, config: KvConfig) -> Self {
        Self {
            id,
            entries: HashMap::new(),
            config,
            total_bytes: 0,
            get_ops: 0,
            set_ops: 0,
            delete_ops: 0,
        }
    }

    fn evict_expired(&mut self) {
        let expired_keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(key, _)| key.clone())
            .collect();

        for key in expired_keys {
            if let Some(entry) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes());
            }
        }
    }

    fn get(&mut self, key: &str) -> Result<Option<Entry>, KvError> {
        self.get_ops += 1;

        if let Some(entry) = self.entries.get(key) {
            if entry.is_expired() {
                if let Some(entry) = self.entries.remove(key) {
                    self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes());
                }
                return Ok(None);
            }
            Ok(Some(entry.clone()))
        } else {
            Ok(None)
        }
    }

    fn set(&mut self, key: String, value: Vec<u8>, options: SetOptions) -> Result<Entry, KvError> {
        self.set_ops += 1;

        // Validate key size
        if key.len() > self.config.max_key_size {
            return Err(KvError::KeyTooLarge { size: key.len(), max: self.config.max_key_size });
        }

        // Validate value size
        if value.len() > self.config.max_value_size {
            return Err(KvError::ValueTooLarge {
                size: value.len(),
                max: self.config.max_value_size,
            });
        }

        // Check if_not_exists
        if options.if_not_exists {
            if let Some(existing) = self.entries.get(&key) {
                if !existing.is_expired() {
                    return Ok(existing.clone());
                }
            }
        }

        // Check compare-and-swap
        if let Some(expected_version) = options.expected_version {
            if let Some(existing) = self.entries.get(&key) {
                if existing.version != expected_version {
                    return Err(KvError::VersionMismatch {
                        expected: expected_version,
                        actual: existing.version,
                    });
                }
            } else {
                return Err(KvError::NotFound(key));
            }
        }

        let new_entry_size = key.len() + value.len();
        let old_entry_size = self.entries.get(&key).map(|e| e.size_bytes()).unwrap_or(0);
        let new_total = self.total_bytes - old_entry_size + new_entry_size;

        // Check quota
        if new_total > self.config.max_namespace_bytes {
            return Err(KvError::QuotaExceeded {
                used: new_total,
                limit: self.config.max_namespace_bytes,
            });
        }

        // Check max keys
        if !self.entries.contains_key(&key)
            && self.entries.len() >= self.config.max_keys_per_namespace
        {
            return Err(KvError::MaxKeysExceeded(self.config.max_keys_per_namespace));
        }

        let now = Instant::now();
        let version = self.entries.get(&key).map(|e| e.version + 1).unwrap_or(1);

        let ttl = options.ttl.or(self.config.default_ttl);
        let expires_at = ttl.map(|t| now + t);

        let entry = Entry {
            key: key.clone(),
            data: value,
            version,
            created_at: self.entries.get(&key).map(|e| e.created_at).unwrap_or(now),
            updated_at: now,
            expires_at,
        };

        self.total_bytes = new_total;
        self.entries.insert(key, entry.clone());

        Ok(entry)
    }

    fn delete(&mut self, key: &str) -> Result<bool, KvError> {
        self.delete_ops += 1;

        if let Some(entry) = self.entries.remove(key) {
            self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn list_keys(&self, prefix: Option<&str>) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| !entry.is_expired())
            .filter(|(key, _)| prefix.map(|p| key.starts_with(p)).unwrap_or(true))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn stats(&self) -> NamespaceStats {
        let expired_count = self.entries.values().filter(|entry| entry.is_expired()).count();

        NamespaceStats {
            key_count: self.entries.len() - expired_count,
            total_bytes: self.total_bytes,
            expired_count,
            get_ops: self.get_ops,
            set_ops: self.set_ops,
            delete_ops: self.delete_ops,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.total_bytes = 0;
    }
}

/// A namespaced view into the KV store.
pub struct Namespace {
    inner: Arc<RwLock<NamespaceInner>>,
}

impl Namespace {
    /// Get the namespace ID.
    pub fn id(&self) -> NamespaceId {
        self.inner.read().id.clone()
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Result<Option<Entry>, KvError> {
        self.inner.write().get(key)
    }

    /// Set a key-value pair.
    pub fn set(
        &self,
        key: impl Into<String>,
        value: impl AsRef<[u8]>,
        ttl: Option<Duration>,
    ) -> Result<Entry, KvError> {
        let options = SetOptions { ttl, ..Default::default() };
        self.inner.write().set(key.into(), value.as_ref().to_vec(), options)
    }

    /// Set a key-value pair with advanced options.
    pub fn set_with_options(
        &self,
        key: impl Into<String>,
        value: impl AsRef<[u8]>,
        options: SetOptions,
    ) -> Result<Entry, KvError> {
        self.inner.write().set(key.into(), value.as_ref().to_vec(), options)
    }

    /// Delete a key.
    pub fn delete(&self, key: &str) -> Result<bool, KvError> {
        self.inner.write().delete(key)
    }

    /// Check if a key exists (and is not expired).
    pub fn exists(&self, key: &str) -> bool {
        self.inner.write().get(key).ok().flatten().is_some()
    }

    /// List keys matching an optional prefix.
    pub fn list_keys(&self, prefix: Option<&str>) -> Vec<String> {
        self.inner.read().list_keys(prefix)
    }

    /// Get namespace statistics.
    pub fn stats(&self) -> NamespaceStats {
        self.inner.read().stats()
    }

    /// Remove all entries from this namespace.
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Remove expired entries.
    pub fn evict_expired(&self) {
        self.inner.write().evict_expired();
    }
}

/// The main key-value store providing namespaced storage.
pub struct KvStore {
    namespaces: Arc<RwLock<HashMap<NamespaceId, Arc<RwLock<NamespaceInner>>>>>,
    config: KvConfig,
}

impl KvStore {
    /// Create a new KV store with the given configuration.
    pub fn new(config: KvConfig) -> Self {
        Self { namespaces: Arc::new(RwLock::new(HashMap::new())), config }
    }

    /// Get or create a namespace.
    pub fn namespace(&self, id: impl Into<String>) -> Namespace {
        let ns_id = NamespaceId::new(id);
        let mut namespaces = self.namespaces.write();

        let inner = namespaces
            .entry(ns_id.clone())
            .or_insert_with(|| {
                Arc::new(RwLock::new(NamespaceInner::new(ns_id, self.config.clone())))
            })
            .clone();

        Namespace { inner }
    }

    /// Delete a namespace and all its data.
    pub fn delete_namespace(&self, id: &str) -> bool {
        let ns_id = NamespaceId::new(id);
        self.namespaces.write().remove(&ns_id).is_some()
    }

    /// List all namespace IDs.
    pub fn list_namespaces(&self) -> Vec<NamespaceId> {
        self.namespaces.read().keys().cloned().collect()
    }

    /// Get aggregate statistics across all namespaces.
    pub fn total_stats(&self) -> NamespaceStats {
        let namespaces = self.namespaces.read();
        let mut total = NamespaceStats {
            key_count: 0,
            total_bytes: 0,
            expired_count: 0,
            get_ops: 0,
            set_ops: 0,
            delete_ops: 0,
        };

        for ns in namespaces.values() {
            let stats = ns.read().stats();
            total.key_count += stats.key_count;
            total.total_bytes += stats.total_bytes;
            total.expired_count += stats.expired_count;
            total.get_ops += stats.get_ops;
            total.set_ops += stats.set_ops;
            total.delete_ops += stats.delete_ops;
        }

        total
    }

    /// Evict expired entries across all namespaces.
    pub fn evict_all_expired(&self) {
        let namespaces = self.namespaces.read();
        for ns in namespaces.values() {
            ns.write().evict_expired();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_set_get() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        let entry = ns.set("key1", b"value1", None).unwrap();
        assert_eq!(entry.version(), 1);
        assert_eq!(entry.data(), b"value1");

        let retrieved = ns.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.data(), b"value1");
        assert_eq!(retrieved.version(), 1);
    }

    #[test]
    fn test_get_nonexistent() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        let result = ns.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_version_increment() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("key", b"v1", None).unwrap();
        let entry = ns.set("key", b"v2", None).unwrap();
        assert_eq!(entry.version(), 2);
    }

    #[test]
    fn test_delete() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("key", b"value", None).unwrap();
        assert!(ns.delete("key").unwrap());
        assert!(ns.get("key").unwrap().is_none());
        assert!(!ns.delete("key").unwrap());
    }

    #[test]
    fn test_ttl_expiration() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        // Set with zero TTL (expires immediately)
        ns.set("key", b"value", Some(Duration::from_nanos(1))).unwrap();
        std::thread::sleep(Duration::from_millis(1));

        let result = ns.get("key").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_compare_and_swap() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("key", b"v1", None).unwrap();

        // CAS with correct version succeeds
        let options = SetOptions { expected_version: Some(1), ..Default::default() };
        let entry = ns.set_with_options("key", b"v2", options).unwrap();
        assert_eq!(entry.version(), 2);

        // CAS with wrong version fails
        let options = SetOptions { expected_version: Some(1), ..Default::default() };
        let result = ns.set_with_options("key", b"v3", options);
        assert!(matches!(result, Err(KvError::VersionMismatch { .. })));
    }

    #[test]
    fn test_if_not_exists() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("key", b"v1", None).unwrap();

        let options = SetOptions { if_not_exists: true, ..Default::default() };
        let entry = ns.set_with_options("key", b"v2", options).unwrap();
        assert_eq!(entry.data(), b"v1"); // Returns existing, not updated
    }

    #[test]
    fn test_quota_exceeded() {
        let config = KvConfig { max_namespace_bytes: 100, ..Default::default() };
        let store = KvStore::new(config);
        let ns = store.namespace("test");

        let result = ns.set("key", vec![0u8; 200], None);
        assert!(matches!(result, Err(KvError::QuotaExceeded { .. })));
    }

    #[test]
    fn test_key_too_large() {
        let config = KvConfig { max_key_size: 10, ..Default::default() };
        let store = KvStore::new(config);
        let ns = store.namespace("test");

        let result = ns.set("a_very_long_key_name", b"value", None);
        assert!(matches!(result, Err(KvError::KeyTooLarge { .. })));
    }

    #[test]
    fn test_namespace_isolation() {
        let store = KvStore::new(KvConfig::default());

        let ns1 = store.namespace("tenant-1");
        let ns2 = store.namespace("tenant-2");

        ns1.set("key", b"value-1", None).unwrap();
        ns2.set("key", b"value-2", None).unwrap();

        assert_eq!(ns1.get("key").unwrap().unwrap().data(), b"value-1");
        assert_eq!(ns2.get("key").unwrap().unwrap().data(), b"value-2");
    }

    #[test]
    fn test_list_keys() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("user:1", b"alice", None).unwrap();
        ns.set("user:2", b"bob", None).unwrap();
        ns.set("item:1", b"widget", None).unwrap();

        let user_keys = ns.list_keys(Some("user:"));
        assert_eq!(user_keys.len(), 2);

        let all_keys = ns.list_keys(None);
        assert_eq!(all_keys.len(), 3);
    }

    #[test]
    fn test_stats() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("k1", b"v1", None).unwrap();
        ns.set("k2", b"v2", None).unwrap();
        ns.get("k1").unwrap();
        ns.delete("k2").unwrap();

        let stats = ns.stats();
        assert_eq!(stats.key_count, 1);
        assert_eq!(stats.set_ops, 2);
        assert_eq!(stats.get_ops, 1);
        assert_eq!(stats.delete_ops, 1);
    }

    #[test]
    fn test_delete_namespace() {
        let store = KvStore::new(KvConfig::default());
        store.namespace("test").set("key", b"value", None).unwrap();

        assert!(store.delete_namespace("test"));
        assert!(!store.delete_namespace("test"));
    }

    #[test]
    fn test_list_namespaces() {
        let store = KvStore::new(KvConfig::default());
        store.namespace("ns-1");
        store.namespace("ns-2");

        let namespaces = store.list_namespaces();
        assert_eq!(namespaces.len(), 2);
    }

    #[test]
    fn test_clear() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");

        ns.set("k1", b"v1", None).unwrap();
        ns.set("k2", b"v2", None).unwrap();
        ns.clear();

        assert_eq!(ns.stats().key_count, 0);
    }

    #[test]
    fn test_max_keys_exceeded() {
        let config = KvConfig { max_keys_per_namespace: 2, ..Default::default() };
        let store = KvStore::new(config);
        let ns = store.namespace("test");

        ns.set("k1", b"v1", None).unwrap();
        ns.set("k2", b"v2", None).unwrap();
        let result = ns.set("k3", b"v3", None);
        assert!(matches!(result, Err(KvError::MaxKeysExceeded(2))));
    }
}
