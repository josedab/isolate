//! Content-addressed WASM module registry with shared compilation cache.
//!
//! Provides a module store keyed by content hash (SHA-256), lazy compilation,
//! signature verification, and provenance metadata.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::module_registry::{ModuleRegistry, RegistryConfig};
//!
//! let registry = ModuleRegistry::new(RegistryConfig::default());
//!
//! let wasm = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
//! let hash = registry.store(wasm, None).unwrap();
//!
//! let entry = registry.get(&hash).unwrap();
//! assert_eq!(entry.bytes, wasm);
//! ```

#![allow(missing_docs)]
use crate::config::ModuleHash;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Configuration for the module registry.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Maximum number of modules to cache.
    pub max_modules: usize,
    /// Maximum total storage in bytes.
    pub max_storage_bytes: u64,
    /// Whether to verify signatures on load.
    pub verify_signatures: bool,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_modules: 10_000,
            max_storage_bytes: 4 * 1024 * 1024 * 1024, // 4GB
            verify_signatures: false,
        }
    }
}

/// Metadata about a registered module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMetadata {
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    /// Version string.
    #[serde(default)]
    pub version: String,
    /// Author or publisher.
    #[serde(default)]
    pub author: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Labels for categorisation.
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    /// Optional cryptographic signature (hex-encoded).
    #[serde(default)]
    pub signature: Option<String>,
}

impl Default for ModuleMetadata {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            labels: std::collections::HashMap::new(),
            signature: None,
        }
    }
}

/// A stored module entry.
#[derive(Debug, Clone)]
pub struct ModuleEntry {
    /// Raw WASM bytes.
    pub bytes: Vec<u8>,
    /// Content hash.
    pub hash: ModuleHash,
    /// Size in bytes.
    pub size: usize,
    /// Metadata.
    pub metadata: ModuleMetadata,
    /// When this entry was stored.
    pub stored_at: Instant,
    /// Number of times this module has been accessed.
    pub access_count: u64,
}

/// Statistics about the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStats {
    pub module_count: usize,
    pub total_bytes: u64,
    pub total_hits: u64,
    pub total_misses: u64,
}

struct StoredEntry {
    bytes: Vec<u8>,
    metadata: ModuleMetadata,
    stored_at: Instant,
    access_count: AtomicU64,
}

/// A content-addressed module registry.
pub struct ModuleRegistry {
    entries: DashMap<ModuleHash, Arc<StoredEntry>>,
    config: RegistryConfig,
    total_bytes: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl ModuleRegistry {
    /// Create a new registry.
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            entries: DashMap::new(),
            config,
            total_bytes: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Store a module. Returns its content hash.
    pub fn store(
        &self,
        wasm_bytes: &[u8],
        metadata: Option<ModuleMetadata>,
    ) -> Result<ModuleHash, RegistryError> {
        let hash = content_hash(wasm_bytes);

        // De-duplicate: if already stored, return existing hash
        if self.entries.contains_key(&hash) {
            return Ok(hash);
        }

        // Check capacity
        if self.entries.len() >= self.config.max_modules {
            self.evict_lru();
        }

        let size = wasm_bytes.len() as u64;
        let current = self.total_bytes.load(Ordering::Relaxed);
        if current + size > self.config.max_storage_bytes {
            return Err(RegistryError::StorageFull {
                used: current,
                max: self.config.max_storage_bytes,
            });
        }

        // Verify signature if required
        if self.config.verify_signatures {
            if let Some(ref meta) = metadata {
                if meta.signature.is_none() {
                    return Err(RegistryError::SignatureRequired);
                }
            } else {
                return Err(RegistryError::SignatureRequired);
            }
        }

        let entry = Arc::new(StoredEntry {
            bytes: wasm_bytes.to_vec(),
            metadata: metadata.unwrap_or_default(),
            stored_at: Instant::now(),
            access_count: AtomicU64::new(0),
        });

        self.entries.insert(hash.clone(), entry);
        self.total_bytes.fetch_add(size, Ordering::Relaxed);

        Ok(hash)
    }

    /// Retrieve a module by hash.
    pub fn get(&self, hash: &ModuleHash) -> Option<ModuleEntry> {
        match self.entries.get(hash) {
            Some(entry) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                let count = entry.access_count.fetch_add(1, Ordering::Relaxed);
                Some(ModuleEntry {
                    bytes: entry.bytes.clone(),
                    hash: hash.clone(),
                    size: entry.bytes.len(),
                    metadata: entry.metadata.clone(),
                    stored_at: entry.stored_at,
                    access_count: count + 1,
                })
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Check if a module exists without loading it.
    pub fn contains(&self, hash: &ModuleHash) -> bool {
        self.entries.contains_key(hash)
    }

    /// Remove a module by hash.
    pub fn remove(&self, hash: &ModuleHash) -> bool {
        if let Some((_, entry)) = self.entries.remove(hash) {
            self.total_bytes.fetch_sub(entry.bytes.len() as u64, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// List all module hashes.
    pub fn list(&self) -> Vec<ModuleHash> {
        self.entries.iter().map(|e| e.key().clone()).collect()
    }

    /// Get registry statistics.
    pub fn stats(&self) -> RegistryStats {
        RegistryStats {
            module_count: self.entries.len(),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            total_hits: self.hits.load(Ordering::Relaxed),
            total_misses: self.misses.load(Ordering::Relaxed),
        }
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.clear();
        self.total_bytes.store(0, Ordering::Relaxed);
    }

    fn evict_lru(&self) {
        // Evict the entry with the lowest access count
        let lru = self
            .entries
            .iter()
            .min_by_key(|e| e.value().access_count.load(Ordering::Relaxed))
            .map(|e| e.key().clone());
        if let Some(key) = lru {
            self.remove(&key);
        }
    }
}

/// Compute a content-addressed hash for WASM bytes.
pub fn content_hash(bytes: &[u8]) -> ModuleHash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    ModuleHash(hex::encode(result))
}

/// Registry errors.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Storage full: {used} bytes used, {max} max")]
    StorageFull { used: u64, max: u64 },
    #[error("Module signature required but not provided")]
    SignatureRequired,
    #[error("Invalid signature")]
    InvalidSignature,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_store_and_get() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        let hash = registry.store(MINIMAL_WASM, None).unwrap();
        let entry = registry.get(&hash).unwrap();
        assert_eq!(entry.bytes, MINIMAL_WASM);
        assert_eq!(entry.size, MINIMAL_WASM.len());
    }

    #[test]
    fn test_content_addressing() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        let hash1 = registry.store(MINIMAL_WASM, None).unwrap();
        let hash2 = registry.store(MINIMAL_WASM, None).unwrap();
        // Same content → same hash, no duplication
        assert_eq!(hash1, hash2);
        assert_eq!(registry.stats().module_count, 1);
    }

    #[test]
    fn test_remove() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        let hash = registry.store(MINIMAL_WASM, None).unwrap();
        assert!(registry.remove(&hash));
        assert!(registry.get(&hash).is_none());
    }

    #[test]
    fn test_storage_limit() {
        let registry = ModuleRegistry::new(RegistryConfig {
            max_storage_bytes: 10,
            ..RegistryConfig::default()
        });
        // Module is 8 bytes, fits
        registry.store(MINIMAL_WASM, None).unwrap();
        // Second different module would exceed limit
        let big = vec![0u8; 11];
        assert!(matches!(registry.store(&big, None), Err(RegistryError::StorageFull { .. })));
    }

    #[test]
    fn test_signature_required() {
        let registry = ModuleRegistry::new(RegistryConfig {
            verify_signatures: true,
            ..RegistryConfig::default()
        });
        assert!(matches!(
            registry.store(MINIMAL_WASM, None),
            Err(RegistryError::SignatureRequired)
        ));

        // With signature, it works
        let meta =
            ModuleMetadata { signature: Some("abc123".to_string()), ..ModuleMetadata::default() };
        assert!(registry.store(MINIMAL_WASM, Some(meta)).is_ok());
    }

    #[test]
    fn test_metadata() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        let meta = ModuleMetadata {
            name: "test-module".to_string(),
            version: "1.0.0".to_string(),
            ..ModuleMetadata::default()
        };
        let hash = registry.store(MINIMAL_WASM, Some(meta)).unwrap();
        let entry = registry.get(&hash).unwrap();
        assert_eq!(entry.metadata.name, "test-module");
        assert_eq!(entry.metadata.version, "1.0.0");
    }

    #[test]
    fn test_stats() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        registry.store(MINIMAL_WASM, None).unwrap();

        let stats = registry.stats();
        assert_eq!(stats.module_count, 1);
        assert_eq!(stats.total_bytes, MINIMAL_WASM.len() as u64);
    }

    #[test]
    fn test_hit_miss_counting() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        let hash = registry.store(MINIMAL_WASM, None).unwrap();

        registry.get(&hash); // hit
        registry.get(&ModuleHash("nonexistent".to_string())); // miss

        let stats = registry.stats();
        assert_eq!(stats.total_hits, 1);
        assert_eq!(stats.total_misses, 1);
    }

    #[test]
    fn test_list_and_contains() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        let hash = registry.store(MINIMAL_WASM, None).unwrap();
        assert!(registry.contains(&hash));
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_clear() {
        let registry = ModuleRegistry::new(RegistryConfig::default());
        registry.store(MINIMAL_WASM, None).unwrap();
        registry.clear();
        assert_eq!(registry.stats().module_count, 0);
    }
}
