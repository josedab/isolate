//! Pre-compiled module registry with content-addressable storage.
//!
//! Provides a local registry for caching pre-compiled `.cwasm` artifacts
//! (Wasmtime serialized modules) to eliminate compilation latency for known
//! modules.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::engine::registry::{ModuleRegistry, RegistryConfig};
//!
//! let config = RegistryConfig::new("/tmp/isolate-registry");
//! let registry = ModuleRegistry::new(config)?;
//!
//! // Store a pre-compiled module
//! registry.store(&engine, &wasm_bytes)?;
//!
//! // Load from cache (near-instant)
//! let module = registry.load(&engine, &hash)?;
//! ```

use crate::config::ModuleHash;
use crate::error::{Error, Result};

use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use wasmtime::{Engine, Module};

/// Configuration for the module registry.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Directory for storing compiled module artifacts.
    pub cache_dir: PathBuf,
    /// Maximum number of in-memory cached entries.
    pub max_memory_entries: usize,
    /// Maximum total disk usage in bytes (0 = unlimited).
    pub max_disk_bytes: u64,
    /// TTL for cached entries (None = no expiry).
    pub entry_ttl: Option<Duration>,
    /// Whether to verify module integrity on load.
    pub verify_on_load: bool,
}

impl RegistryConfig {
    /// Create a new registry config with the given cache directory.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            max_memory_entries: 256,
            max_disk_bytes: 1024 * 1024 * 1024, // 1 GB
            entry_ttl: None,
            verify_on_load: true,
        }
    }

    /// Set max in-memory entries.
    pub fn max_memory_entries(mut self, n: usize) -> Self {
        self.max_memory_entries = n;
        self
    }

    /// Set max disk usage.
    pub fn max_disk_bytes(mut self, n: u64) -> Self {
        self.max_disk_bytes = n;
        self
    }

    /// Set entry TTL.
    pub fn entry_ttl(mut self, ttl: Duration) -> Self {
        self.entry_ttl = Some(ttl);
        self
    }
}

/// Metadata about a cached module entry.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Content hash (SHA-256 of original WASM bytes).
    pub hash: ModuleHash,
    /// Size of the original WASM module in bytes.
    pub original_size: u64,
    /// Size of the compiled artifact in bytes.
    pub compiled_size: u64,
    /// When this entry was stored.
    pub stored_at: SystemTime,
    /// Wasmtime version used for compilation.
    pub engine_version: String,
    /// Number of times this entry has been loaded.
    pub hit_count: u64,
}

/// In-memory cache entry wrapping a deserialized Module.
struct MemoryCacheEntry {
    module: Module,
    _stored_at: SystemTime,
    hit_count: AtomicU64,
}

/// Pre-compiled module registry with disk and memory caching layers.
pub struct ModuleRegistry {
    config: RegistryConfig,
    memory_cache: Arc<DashMap<String, Arc<MemoryCacheEntry>>>,
    stats: RegistryStats,
}

/// Registry statistics.
#[derive(Debug, Clone, Default)]
pub struct RegistryStats {
    /// Cache hits (memory).
    pub memory_hits: Arc<AtomicU64>,
    /// Cache hits (disk).
    pub disk_hits: Arc<AtomicU64>,
    /// Cache misses.
    pub misses: Arc<AtomicU64>,
    /// Total stores.
    pub stores: Arc<AtomicU64>,
    /// Evictions.
    pub evictions: Arc<AtomicU64>,
}

impl ModuleRegistry {
    /// Create a new module registry.
    pub fn new(config: RegistryConfig) -> Result<Self> {
        // Ensure cache directory exists
        std::fs::create_dir_all(&config.cache_dir)
            .map_err(|e| Error::Engine(format!("Failed to create registry cache dir: {}", e)))?;

        Ok(Self { config, memory_cache: Arc::new(DashMap::new()), stats: RegistryStats::default() })
    }

    /// Create an in-memory-only registry (no disk persistence).
    pub fn in_memory(max_entries: usize) -> Self {
        Self {
            config: RegistryConfig {
                cache_dir: PathBuf::from("/dev/null"),
                max_memory_entries: max_entries,
                max_disk_bytes: 0,
                entry_ttl: None,
                verify_on_load: false,
            },
            memory_cache: Arc::new(DashMap::new()),
            stats: RegistryStats::default(),
        }
    }

    /// Store a WASM module's pre-compiled artifact.
    ///
    /// Compiles the module, serializes the artifact, and stores it in both
    /// memory and disk caches.
    pub fn store(&self, engine: &Engine, wasm_bytes: &[u8]) -> Result<RegistryEntry> {
        let hash = ModuleHash::from_bytes(wasm_bytes);

        // Compile the module
        let module =
            Module::new(engine, wasm_bytes).map_err(|e| Error::Compilation(e.to_string()))?;

        // Serialize for disk storage
        let compiled_bytes = module
            .serialize()
            .map_err(|e| Error::Engine(format!("Failed to serialize module: {}", e)))?;

        let now = SystemTime::now();

        // Store on disk
        if self.config.max_disk_bytes > 0 {
            let artifact_path = self.artifact_path(&hash.0)?;
            if let Some(parent) = artifact_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&artifact_path, &compiled_bytes)
                .map_err(|e| Error::Engine(format!("Failed to write compiled artifact: {}", e)))?;
        }

        // Store in memory cache
        self.evict_if_needed();
        let entry =
            Arc::new(MemoryCacheEntry { module, _stored_at: now, hit_count: AtomicU64::new(0) });
        self.memory_cache.insert(hash.0.clone(), entry);

        self.stats.stores.fetch_add(1, Ordering::Relaxed);

        Ok(RegistryEntry {
            hash,
            original_size: wasm_bytes.len() as u64,
            compiled_size: compiled_bytes.len() as u64,
            stored_at: now,
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            hit_count: 0,
        })
    }

    /// Store a pre-compiled module directly (already compiled Module).
    pub fn store_compiled(&self, hash: ModuleHash, module: Module) -> Result<()> {
        self.evict_if_needed();
        let entry = Arc::new(MemoryCacheEntry {
            module,
            _stored_at: SystemTime::now(),
            hit_count: AtomicU64::new(0),
        });
        self.memory_cache.insert(hash.0, entry);
        self.stats.stores.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Load a pre-compiled module by hash.
    ///
    /// Checks memory cache first, then disk cache. Returns None if not found.
    pub fn load(&self, engine: &Engine, hash: &str) -> Result<Option<Module>> {
        // Check memory cache
        if let Some(entry) = self.memory_cache.get(hash) {
            entry.hit_count.fetch_add(1, Ordering::Relaxed);
            self.stats.memory_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(Some(entry.module.clone()));
        }

        // Check disk cache
        if self.config.max_disk_bytes > 0 {
            let artifact_path = self.artifact_path(hash)?;
            if artifact_path.exists() {
                if let Some(ttl) = self.config.entry_ttl {
                    if let Ok(metadata) = std::fs::metadata(&artifact_path) {
                        if let Ok(modified) = metadata.modified() {
                            if modified.elapsed().unwrap_or(Duration::MAX) > ttl {
                                std::fs::remove_file(&artifact_path).ok();
                                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                                return Ok(None);
                            }
                        }
                    }
                }

                let compiled_bytes = std::fs::read(&artifact_path).map_err(|e| {
                    Error::Engine(format!("Failed to read compiled artifact: {}", e))
                })?;

                // SAFETY: We trust artifacts produced by our own `precompile` method.
                // The serialized module bytes were written to a path we control and
                // are deserialized with the same Wasmtime engine version that produced them.
                // On version mismatch, deserialization fails and we remove the stale artifact.
                let module = unsafe {
                    Module::deserialize(engine, &compiled_bytes).map_err(|e| {
                        // Stale artifact (e.g., wasmtime version mismatch) → remove it
                        std::fs::remove_file(&artifact_path).ok();
                        Error::Engine(format!("Failed to deserialize module: {}", e))
                    })?
                };

                // Promote to memory cache
                self.evict_if_needed();
                let cache_entry = Arc::new(MemoryCacheEntry {
                    module: module.clone(),
                    _stored_at: SystemTime::now(),
                    hit_count: AtomicU64::new(1),
                });
                self.memory_cache.insert(hash.to_string(), cache_entry);

                self.stats.disk_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(Some(module));
            }
        }

        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        Ok(None)
    }

    /// Check if a module exists in the registry.
    pub fn contains(&self, hash: &str) -> bool {
        if self.memory_cache.contains_key(hash) {
            return true;
        }
        if self.config.max_disk_bytes > 0 {
            return self.artifact_path(hash).map(|p| p.exists()).unwrap_or(false);
        }
        false
    }

    /// Remove a module from the registry.
    pub fn remove(&self, hash: &str) -> bool {
        let mem_removed = self.memory_cache.remove(hash).is_some();
        let disk_removed = if self.config.max_disk_bytes > 0 {
            self.artifact_path(hash).map(|p| std::fs::remove_file(p).is_ok()).unwrap_or(false)
        } else {
            false
        };
        mem_removed || disk_removed
    }

    /// Get registry statistics.
    pub fn stats(&self) -> RegistryStatsSnapshot {
        RegistryStatsSnapshot {
            memory_entries: self.memory_cache.len(),
            memory_hits: self.stats.memory_hits.load(Ordering::Relaxed),
            disk_hits: self.stats.disk_hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            stores: self.stats.stores.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
        }
    }

    /// Clear all cached entries (memory and disk).
    pub fn clear(&self) {
        self.memory_cache.clear();
        if self.config.max_disk_bytes > 0 {
            if let Ok(entries) = std::fs::read_dir(&self.config.cache_dir) {
                for entry in entries.flatten() {
                    if entry.path().extension().is_some_and(|e| e == "cwasm") {
                        std::fs::remove_file(entry.path()).ok();
                    }
                }
            }
        }
    }

    /// List all entries in the registry.
    pub fn list(&self) -> Vec<String> {
        let mut hashes: Vec<String> = self.memory_cache.iter().map(|e| e.key().clone()).collect();

        if self.config.max_disk_bytes > 0 {
            if let Ok(entries) = std::fs::read_dir(&self.config.cache_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "cwasm") {
                        if let Some(stem) = path.file_stem() {
                            let h = stem.to_string_lossy().to_string();
                            if !hashes.contains(&h) {
                                hashes.push(h);
                            }
                        }
                    }
                }
            }
        }

        hashes
    }

    fn artifact_path(&self, hash: &str) -> Result<PathBuf> {
        // Validate hash contains only safe characters (hex digits, underscores, hyphens)
        if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit() || c == '_' || c == '-') {
            return Err(Error::Execution(
                "invalid hash format: must contain only hex digits, underscores, or hyphens"
                    .to_string(),
            ));
        }
        // Use first 2 chars as subdirectory for filesystem scalability
        let subdir = if hash.len() >= 2 { &hash[..2] } else { "00" };
        Ok(self.config.cache_dir.join(subdir).join(format!("{}.cwasm", hash)))
    }

    fn evict_if_needed(&self) {
        while self.memory_cache.len() >= self.config.max_memory_entries {
            // Evict the entry with lowest hit count
            let victim = self
                .memory_cache
                .iter()
                .min_by_key(|e| e.value().hit_count.load(Ordering::Relaxed))
                .map(|e| e.key().clone());

            if let Some(key) = victim {
                self.memory_cache.remove(&key);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            } else {
                break;
            }
        }
    }
}

/// Snapshot of registry statistics.
#[derive(Debug, Clone)]
pub struct RegistryStatsSnapshot {
    /// Number of entries in memory cache.
    pub memory_entries: usize,
    /// Number of memory cache hits.
    pub memory_hits: u64,
    /// Number of disk cache hits.
    pub disk_hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Total number of stores.
    pub stores: u64,
    /// Number of evictions.
    pub evictions: u64,
}

impl RegistryStatsSnapshot {
    /// Calculate cache hit rate.
    pub fn hit_rate(&self) -> f64 {
        let total = self.memory_hits + self.disk_hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        (self.memory_hits + self.disk_hits) as f64 / total as f64
    }
}

impl std::fmt::Display for RegistryStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Registry: {} entries, {:.1}% hit rate (mem:{}, disk:{}, miss:{})",
            self.memory_entries,
            self.hit_rate() * 100.0,
            self.memory_hits,
            self.disk_hits,
            self.misses
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid WASM module
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ];

    fn test_engine() -> Engine {
        Engine::default()
    }

    #[test]
    fn test_in_memory_registry() {
        let registry = ModuleRegistry::in_memory(10);
        let stats = registry.stats();
        assert_eq!(stats.memory_entries, 0);
        assert_eq!(stats.stores, 0);
    }

    #[test]
    fn test_store_and_load_memory() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        let entry = registry.store(&engine, MINIMAL_WASM).unwrap();
        assert!(entry.original_size > 0);

        let module = registry.load(&engine, &entry.hash.0).unwrap();
        assert!(module.is_some());

        let stats = registry.stats();
        assert_eq!(stats.stores, 1);
        assert_eq!(stats.memory_hits, 1);
    }

    #[test]
    fn test_store_and_load_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let config = RegistryConfig::new(tmp.path());
        let registry = ModuleRegistry::new(config).unwrap();
        let engine = test_engine();

        let entry = registry.store(&engine, MINIMAL_WASM).unwrap();
        assert!(entry.compiled_size > 0);

        // Clear memory cache to force disk load
        registry.memory_cache.clear();

        let module = registry.load(&engine, &entry.hash.0).unwrap();
        assert!(module.is_some());

        let stats = registry.stats();
        assert_eq!(stats.disk_hits, 1);
    }

    #[test]
    fn test_contains() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        let entry = registry.store(&engine, MINIMAL_WASM).unwrap();
        assert!(registry.contains(&entry.hash.0));
        assert!(!registry.contains("nonexistent"));
    }

    #[test]
    fn test_remove() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        let entry = registry.store(&engine, MINIMAL_WASM).unwrap();
        assert!(registry.contains(&entry.hash.0));

        let removed = registry.remove(&entry.hash.0);
        assert!(removed);
        assert!(!registry.contains(&entry.hash.0));
    }

    #[test]
    fn test_clear() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        registry.store(&engine, MINIMAL_WASM).unwrap();
        assert_eq!(registry.stats().memory_entries, 1);

        registry.clear();
        assert_eq!(registry.stats().memory_entries, 0);
    }

    #[test]
    fn test_list() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        let entry = registry.store(&engine, MINIMAL_WASM).unwrap();
        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert!(list.contains(&entry.hash.0));
    }

    #[test]
    fn test_eviction() {
        let registry = ModuleRegistry::in_memory(1);
        let engine = test_engine();

        // Store first module
        registry.store(&engine, MINIMAL_WASM).unwrap();
        assert_eq!(registry.stats().memory_entries, 1);

        // Store another (triggers eviction of first)
        let wasm2: Vec<u8> = {
            let mut v = MINIMAL_WASM.to_vec();
            // Add an empty custom section to make it a different module
            v.extend_from_slice(&[0x00, 0x01, 0x00]); // custom section, size 1, empty name
            v
        };
        registry.store(&engine, &wasm2).unwrap();
        assert_eq!(registry.stats().memory_entries, 1);
        assert!(registry.stats().evictions >= 1);
    }

    #[test]
    fn test_load_miss() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        let result = registry.load(&engine, "nonexistent_hash").unwrap();
        assert!(result.is_none());
        assert_eq!(registry.stats().misses, 1);
    }

    #[test]
    fn test_hit_rate() {
        let stats = RegistryStatsSnapshot {
            memory_entries: 5,
            memory_hits: 80,
            disk_hits: 10,
            misses: 10,
            stores: 15,
            evictions: 0,
        };
        assert!((stats.hit_rate() - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_display() {
        let stats = RegistryStatsSnapshot {
            memory_entries: 3,
            memory_hits: 10,
            disk_hits: 5,
            misses: 5,
            stores: 8,
            evictions: 2,
        };
        let s = format!("{}", stats);
        assert!(s.contains("3 entries"));
        assert!(s.contains("75.0%"));
    }

    #[test]
    fn test_disk_registry_config() {
        let config = RegistryConfig::new("/tmp/test-registry")
            .max_memory_entries(50)
            .max_disk_bytes(512 * 1024 * 1024)
            .entry_ttl(Duration::from_secs(3600));

        assert_eq!(config.max_memory_entries, 50);
        assert_eq!(config.max_disk_bytes, 512 * 1024 * 1024);
        assert_eq!(config.entry_ttl, Some(Duration::from_secs(3600)));
    }

    #[test]
    fn test_store_compiled_directly() {
        let registry = ModuleRegistry::in_memory(10);
        let engine = test_engine();

        let module = Module::new(&engine, MINIMAL_WASM).unwrap();
        let hash = ModuleHash::from_bytes(MINIMAL_WASM);
        let hash_str = hash.0.clone();

        registry.store_compiled(hash, module).unwrap();
        assert!(registry.contains(&hash_str));
    }
}
