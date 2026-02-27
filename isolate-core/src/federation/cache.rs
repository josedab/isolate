//! Local module cache with LRU eviction.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::content::ContentId;

/// Cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub max_size_bytes: usize,
    pub max_entries: usize,
    pub ttl_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 256 * 1024 * 1024, // 256MB
            max_entries: 1000,
            ttl_seconds: 86400, // 24 hours
        }
    }
}

/// A cached entry.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub cid: ContentId,
    pub name: String,
    pub data: Vec<u8>,
    pub cached_at: u64,
    pub last_accessed: u64,
    pub access_count: u64,
}

/// Cache statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub entries: usize,
    pub total_size_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// LRU-evicting module cache.
#[derive(Clone)]
pub struct ModuleCache {
    inner: Arc<CacheInner>,
}

struct CacheInner {
    config: CacheConfig,
    entries: RwLock<HashMap<ContentId, CacheEntry>>,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
}

impl ModuleCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            inner: Arc::new(CacheInner {
                config,
                entries: RwLock::new(HashMap::new()),
                hits: AtomicU64::new(0),
                misses: AtomicU64::new(0),
                evictions: AtomicU64::new(0),
            }),
        }
    }

    /// Put a module into the cache.
    pub fn put(&self, cid: &ContentId, data: &[u8], name: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Evict if needed
        self.evict_if_needed(data.len());

        self.inner.entries.write().insert(
            cid.clone(),
            CacheEntry {
                cid: cid.clone(),
                name: name.to_string(),
                data: data.to_vec(),
                cached_at: now,
                last_accessed: now,
                access_count: 0,
            },
        );
    }

    /// Get a module from the cache.
    pub fn get(&self, cid: &ContentId) -> Option<Vec<u8>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut entries = self.inner.entries.write();
        if let Some(entry) = entries.get_mut(cid) {
            entry.last_accessed = now;
            entry.access_count += 1;
            self.inner.hits.fetch_add(1, Ordering::Relaxed);
            Some(entry.data.clone())
        } else {
            drop(entries);
            self.inner.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Check if a CID is cached.
    pub fn contains(&self, cid: &ContentId) -> bool {
        self.inner.entries.read().contains_key(cid)
    }

    /// Remove from cache.
    pub fn invalidate(&self, cid: &ContentId) -> bool {
        self.inner.entries.write().remove(cid).is_some()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.inner.entries.write().clear();
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let entries = self.inner.entries.read();
        CacheStats {
            entries: entries.len(),
            total_size_bytes: entries.values().map(|e| e.data.len()).sum(),
            hits: self.inner.hits.load(Ordering::Relaxed),
            misses: self.inner.misses.load(Ordering::Relaxed),
            evictions: self.inner.evictions.load(Ordering::Relaxed),
        }
    }

    fn evict_if_needed(&self, incoming_size: usize) {
        let mut entries = self.inner.entries.write();

        // Evict by max entries
        while entries.len() >= self.inner.config.max_entries {
            if let Some(lru_key) = self.find_lru(&entries) {
                entries.remove(&lru_key);
                self.inner.evictions.fetch_add(1, Ordering::Relaxed);
            } else {
                break;
            }
        }

        // Evict by max size
        let current_size: usize = entries.values().map(|e| e.data.len()).sum();
        let mut need_to_free =
            (current_size + incoming_size).saturating_sub(self.inner.config.max_size_bytes);

        while need_to_free > 0 {
            if let Some(lru_key) = self.find_lru(&entries) {
                let freed = entries.get(&lru_key).map_or(0, |e| e.data.len());
                entries.remove(&lru_key);
                self.inner.evictions.fetch_add(1, Ordering::Relaxed);
                need_to_free = need_to_free.saturating_sub(freed);
            } else {
                break;
            }
        }
    }

    fn find_lru(&self, entries: &HashMap<ContentId, CacheEntry>) -> Option<ContentId> {
        entries.iter().min_by_key(|(_, e)| e.last_accessed).map(|(k, _)| k.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get() {
        let cache = ModuleCache::new(CacheConfig::default());
        let cid = ContentId::from_bytes(b"module");
        cache.put(&cid, b"module", "test.wasm");

        let data = cache.get(&cid).unwrap();
        assert_eq!(data, b"module");
    }

    #[test]
    fn test_cache_miss() {
        let cache = ModuleCache::new(CacheConfig::default());
        let cid = ContentId::from_bytes(b"nonexistent");
        assert!(cache.get(&cid).is_none());
    }

    #[test]
    fn test_hit_rate() {
        let cache = ModuleCache::new(CacheConfig::default());
        let cid = ContentId::from_bytes(b"data");
        cache.put(&cid, b"data", "d.wasm");

        cache.get(&cid); // hit
        cache.get(&cid); // hit
        cache.get(&ContentId::from_bytes(b"miss")); // miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 0.667).abs() < 0.01);
    }

    #[test]
    fn test_eviction_by_count() {
        let cache = ModuleCache::new(CacheConfig {
            max_entries: 2,
            max_size_bytes: usize::MAX,
            ttl_seconds: 86400,
        });

        cache.put(&ContentId::from_bytes(b"a"), b"a", "a.wasm");
        cache.put(&ContentId::from_bytes(b"b"), b"b", "b.wasm");
        cache.put(&ContentId::from_bytes(b"c"), b"c", "c.wasm"); // should evict one

        let stats = cache.stats();
        assert!(stats.entries <= 2);
        assert!(stats.evictions >= 1);
    }

    #[test]
    fn test_invalidate() {
        let cache = ModuleCache::new(CacheConfig::default());
        let cid = ContentId::from_bytes(b"data");
        cache.put(&cid, b"data", "d.wasm");
        assert!(cache.contains(&cid));
        assert!(cache.invalidate(&cid));
        assert!(!cache.contains(&cid));
    }

    #[test]
    fn test_clear() {
        let cache = ModuleCache::new(CacheConfig::default());
        cache.put(&ContentId::from_bytes(b"a"), b"a", "a.wasm");
        cache.put(&ContentId::from_bytes(b"b"), b"b", "b.wasm");
        cache.clear();
        assert_eq!(cache.stats().entries, 0);
    }

    #[test]
    fn test_stats_size() {
        let cache = ModuleCache::new(CacheConfig::default());
        cache.put(&ContentId::from_bytes(b"aaa"), b"aaa", "a.wasm");
        cache.put(&ContentId::from_bytes(b"bb"), b"bb", "b.wasm");
        assert_eq!(cache.stats().total_size_bytes, 5);
    }
}
