//! Embedded key-value store for sandbox state.
//!
//! Provides per-sandbox key-value storage with configurable size quotas,
//! namespace isolation, and optional persistence.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::sandbox_kv::{SandboxKvStore, KvConfig};
//!
//! let store = SandboxKvStore::new(KvConfig::default());
//!
//! // Create a namespace for a sandbox
//! let ns = store.namespace("sandbox-123");
//! ns.set("key", b"value").unwrap();
//! assert_eq!(ns.get("key").unwrap(), Some(b"value".to_vec()));
//! ```

#![allow(missing_docs)]
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Configuration for the KV store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvConfig {
    /// Maximum number of keys per namespace.
    pub max_keys: usize,
    /// Maximum value size in bytes.
    pub max_value_size: usize,
    /// Maximum total storage per namespace in bytes.
    pub max_namespace_size: usize,
    /// Maximum number of namespaces.
    pub max_namespaces: usize,
}

impl Default for KvConfig {
    fn default() -> Self {
        Self {
            max_keys: 10_000,
            max_value_size: 1024 * 1024,          // 1MB per value
            max_namespace_size: 64 * 1024 * 1024, // 64MB per namespace
            max_namespaces: 1000,
        }
    }
}

/// Errors from KV operations.
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    #[error("Key not found: {0}")]
    NotFound(String),
    #[error("Value too large: {size} bytes (max {max})")]
    ValueTooLarge { size: usize, max: usize },
    #[error("Too many keys: {count} (max {max})")]
    TooManyKeys { count: usize, max: usize },
    #[error("Namespace storage exceeded: {used} bytes (max {max})")]
    StorageExceeded { used: usize, max: usize },
    #[error("Too many namespaces: {count} (max {max})")]
    TooManyNamespaces { count: usize, max: usize },
}

/// A KV entry with metadata.
#[derive(Debug, Clone)]
struct KvEntry {
    value: Vec<u8>,
    #[allow(dead_code)]
    created_at: std::time::Instant,
    updated_at: std::time::Instant,
}

/// Stats for a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceStats {
    pub key_count: usize,
    pub total_bytes: u64,
}

/// A namespace within the KV store.
struct NamespaceInner {
    entries: DashMap<String, KvEntry>,
    total_bytes: AtomicU64,
    config: KvConfig,
}

/// Handle to a namespace, used for KV operations.
pub struct KvNamespace {
    inner: Arc<NamespaceInner>,
    name: String,
}

impl KvNamespace {
    /// Get a value by key.
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, KvError> {
        Ok(self.inner.entries.get(key).map(|e| e.value.clone()))
    }

    /// Set a value.
    pub fn set(&self, key: &str, value: &[u8]) -> Result<(), KvError> {
        let config = &self.inner.config;

        if value.len() > config.max_value_size {
            return Err(KvError::ValueTooLarge { size: value.len(), max: config.max_value_size });
        }

        let now = std::time::Instant::now();
        let new_entry = KvEntry { value: value.to_vec(), created_at: now, updated_at: now };

        // Check if updating an existing key
        if let Some(mut existing) = self.inner.entries.get_mut(key) {
            let old_size = existing.value.len() as u64;
            let new_size = value.len() as u64;

            // Check total size after update
            let current = self.inner.total_bytes.load(Ordering::Relaxed);
            let projected = current - old_size + new_size;
            if projected > config.max_namespace_size as u64 {
                return Err(KvError::StorageExceeded {
                    used: projected as usize,
                    max: config.max_namespace_size,
                });
            }

            self.inner.total_bytes.fetch_sub(old_size, Ordering::Relaxed);
            existing.value = value.to_vec();
            existing.updated_at = now;
            self.inner.total_bytes.fetch_add(new_size, Ordering::Relaxed);
        } else {
            // New key
            if self.inner.entries.len() >= config.max_keys {
                return Err(KvError::TooManyKeys {
                    count: self.inner.entries.len(),
                    max: config.max_keys,
                });
            }

            let new_size = value.len() as u64;
            let current = self.inner.total_bytes.load(Ordering::Relaxed);
            if current + new_size > config.max_namespace_size as u64 {
                return Err(KvError::StorageExceeded {
                    used: (current + new_size) as usize,
                    max: config.max_namespace_size,
                });
            }

            self.inner.entries.insert(key.to_string(), new_entry);
            self.inner.total_bytes.fetch_add(new_size, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Delete a key. Returns true if the key existed.
    pub fn delete(&self, key: &str) -> bool {
        if let Some((_, entry)) = self.inner.entries.remove(key) {
            self.inner.total_bytes.fetch_sub(entry.value.len() as u64, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Check if a key exists.
    pub fn contains(&self, key: &str) -> bool {
        self.inner.entries.contains_key(key)
    }

    /// List all keys.
    pub fn keys(&self) -> Vec<String> {
        self.inner.entries.iter().map(|e| e.key().clone()).collect()
    }

    /// Get namespace statistics.
    pub fn stats(&self) -> NamespaceStats {
        NamespaceStats {
            key_count: self.inner.entries.len(),
            total_bytes: self.inner.total_bytes.load(Ordering::Relaxed),
        }
    }

    /// Clear all entries in this namespace.
    pub fn clear(&self) {
        self.inner.entries.clear();
        self.inner.total_bytes.store(0, Ordering::Relaxed);
    }

    /// Get the namespace name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A multi-namespace key-value store for sandbox state.
pub struct SandboxKvStore {
    namespaces: DashMap<String, Arc<NamespaceInner>>,
    config: KvConfig,
}

impl SandboxKvStore {
    /// Create a new KV store.
    pub fn new(config: KvConfig) -> Self {
        Self { namespaces: DashMap::new(), config }
    }

    /// Get or create a namespace.
    pub fn namespace(&self, name: &str) -> Result<KvNamespace, KvError> {
        if let Some(inner) = self.namespaces.get(name) {
            return Ok(KvNamespace { inner: inner.value().clone(), name: name.to_string() });
        }

        if self.namespaces.len() >= self.config.max_namespaces {
            return Err(KvError::TooManyNamespaces {
                count: self.namespaces.len(),
                max: self.config.max_namespaces,
            });
        }

        let inner = Arc::new(NamespaceInner {
            entries: DashMap::new(),
            total_bytes: AtomicU64::new(0),
            config: self.config.clone(),
        });
        self.namespaces.insert(name.to_string(), inner.clone());

        Ok(KvNamespace { inner, name: name.to_string() })
    }

    /// Remove a namespace and all its data.
    pub fn remove_namespace(&self, name: &str) -> bool {
        self.namespaces.remove(name).is_some()
    }

    /// List all namespace names.
    pub fn namespace_names(&self) -> Vec<String> {
        self.namespaces.iter().map(|e| e.key().clone()).collect()
    }

    /// Get the total number of namespaces.
    pub fn namespace_count(&self) -> usize {
        self.namespaces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_get_set() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns = store.namespace("test").unwrap();
        ns.set("key", b"value").unwrap();
        assert_eq!(ns.get("key").unwrap(), Some(b"value".to_vec()));
    }

    #[test]
    fn test_delete() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns = store.namespace("test").unwrap();
        ns.set("key", b"value").unwrap();
        assert!(ns.delete("key"));
        assert_eq!(ns.get("key").unwrap(), None);
        assert!(!ns.delete("nonexistent"));
    }

    #[test]
    fn test_overwrite() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns = store.namespace("test").unwrap();
        ns.set("key", b"old").unwrap();
        ns.set("key", b"new").unwrap();
        assert_eq!(ns.get("key").unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn test_keys_and_contains() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns = store.namespace("test").unwrap();
        ns.set("a", b"1").unwrap();
        ns.set("b", b"2").unwrap();

        assert!(ns.contains("a"));
        assert!(!ns.contains("c"));

        let mut keys = ns.keys();
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn test_value_too_large() {
        let store = SandboxKvStore::new(KvConfig { max_value_size: 10, ..KvConfig::default() });
        let ns = store.namespace("test").unwrap();
        assert!(ns.set("key", &[0u8; 11]).is_err());
    }

    #[test]
    fn test_too_many_keys() {
        let store = SandboxKvStore::new(KvConfig { max_keys: 2, ..KvConfig::default() });
        let ns = store.namespace("test").unwrap();
        ns.set("a", b"1").unwrap();
        ns.set("b", b"2").unwrap();
        assert!(matches!(ns.set("c", b"3"), Err(KvError::TooManyKeys { .. })));
    }

    #[test]
    fn test_storage_exceeded() {
        let store = SandboxKvStore::new(KvConfig { max_namespace_size: 20, ..KvConfig::default() });
        let ns = store.namespace("test").unwrap();
        ns.set("a", &[0u8; 10]).unwrap();
        ns.set("b", &[0u8; 10]).unwrap();
        assert!(matches!(ns.set("c", &[0u8; 5]), Err(KvError::StorageExceeded { .. })));
    }

    #[test]
    fn test_namespace_isolation() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns1 = store.namespace("ns1").unwrap();
        let ns2 = store.namespace("ns2").unwrap();

        ns1.set("key", b"from-ns1").unwrap();
        ns2.set("key", b"from-ns2").unwrap();

        assert_eq!(ns1.get("key").unwrap(), Some(b"from-ns1".to_vec()));
        assert_eq!(ns2.get("key").unwrap(), Some(b"from-ns2".to_vec()));
    }

    #[test]
    fn test_too_many_namespaces() {
        let store = SandboxKvStore::new(KvConfig { max_namespaces: 2, ..KvConfig::default() });
        store.namespace("a").unwrap();
        store.namespace("b").unwrap();
        assert!(matches!(store.namespace("c"), Err(KvError::TooManyNamespaces { .. })));
    }

    #[test]
    fn test_remove_namespace() {
        let store = SandboxKvStore::new(KvConfig::default());
        store.namespace("x").unwrap();
        assert_eq!(store.namespace_count(), 1);
        assert!(store.remove_namespace("x"));
        assert_eq!(store.namespace_count(), 0);
    }

    #[test]
    fn test_clear_namespace() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns = store.namespace("test").unwrap();
        ns.set("a", b"1").unwrap();
        ns.set("b", b"2").unwrap();
        ns.clear();
        assert_eq!(ns.stats().key_count, 0);
        assert_eq!(ns.stats().total_bytes, 0);
    }

    #[test]
    fn test_stats() {
        let store = SandboxKvStore::new(KvConfig::default());
        let ns = store.namespace("test").unwrap();
        ns.set("key", b"12345").unwrap();

        let stats = ns.stats();
        assert_eq!(stats.key_count, 1);
        assert_eq!(stats.total_bytes, 5);
    }
}
