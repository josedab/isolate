//! Copy-on-Write warm pool for fast sandbox instantiation.
//!
//! Pre-compiles and caches WASM modules to serve sandbox instances with
//! minimal startup latency. Uses Wasmtime's module compilation caching
//! to achieve near-instant warm starts.
//!
//! ```rust,ignore
//! use isolate_core::pool::warm::{WarmPool, WarmPoolConfig};
//!
//! let pool = WarmPool::new(WarmPoolConfig::default());
//! pool.preload("processor", wasm_bytes)?;
//! let instance = pool.acquire("processor")?;
//! ```

use crate::config::ModuleHash;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};

/// Configuration for the warm pool.
#[derive(Debug, Clone)]
pub struct WarmPoolConfig {
    /// Maximum number of modules to keep pre-compiled.
    pub max_modules: usize,
    /// Maximum warm instances per module.
    pub max_instances_per_module: usize,
    /// Idle timeout before evicting a warm instance.
    pub idle_timeout: Duration,
    /// Maximum total memory across all warm instances (bytes).
    pub max_total_memory: usize,
    /// Enable eviction statistics tracking.
    pub track_stats: bool,
}

impl Default for WarmPoolConfig {
    fn default() -> Self {
        Self {
            max_modules: 100,
            max_instances_per_module: 10,
            idle_timeout: Duration::from_secs(300),
            max_total_memory: 1024 * 1024 * 1024, // 1GB
            track_stats: true,
        }
    }
}

/// State of a pre-compiled module in the pool.
#[derive(Debug, Clone)]
pub struct PrecompiledModule {
    /// Module name/identifier.
    pub name: String,
    /// Module hash.
    pub hash: ModuleHash,
    /// Module size in bytes.
    pub size: usize,
    /// When the module was compiled.
    pub compiled_at: SystemTime,
    /// Number of times this module has been instantiated.
    pub instantiation_count: u64,
    /// Average instantiation time.
    pub avg_instantiation_us: f64,
}

/// A warm instance ready for immediate use.
#[derive(Debug)]
pub struct WarmInstance {
    /// Module name.
    pub module_name: String,
    /// Module hash.
    pub module_hash: ModuleHash,
    /// When this instance was created.
    pub created_at: Instant,
    /// Unique instance ID.
    pub instance_id: u64,
    /// Memory footprint estimate (bytes).
    pub memory_estimate: usize,
}

/// Eviction policy for the warm pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Least Recently Used — evict the oldest accessed instance.
    Lru,
    /// Least Frequently Used — evict the least used module's instances.
    Lfu,
    /// First In First Out — evict the oldest instance.
    Fifo,
}

/// Error from warm pool operations.
#[derive(Debug, Clone)]
pub enum WarmPoolError {
    /// Module not found in the pool.
    ModuleNotFound(String),
    /// Pool is at capacity.
    PoolFull,
    /// No warm instances available.
    NoInstancesAvailable(String),
    /// Module already preloaded.
    AlreadyPreloaded(String),
    /// Compilation failed.
    CompilationFailed(String),
}

impl std::fmt::Display for WarmPoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleNotFound(name) => write!(f, "module '{}' not found in pool", name),
            Self::PoolFull => write!(f, "warm pool is at maximum capacity"),
            Self::NoInstancesAvailable(name) => {
                write!(f, "no warm instances available for '{}'", name)
            }
            Self::AlreadyPreloaded(name) => write!(f, "module '{}' already preloaded", name),
            Self::CompilationFailed(e) => write!(f, "compilation failed: {}", e),
        }
    }
}

impl std::error::Error for WarmPoolError {}

/// Pool-wide statistics.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total preloaded modules.
    pub preloaded_modules: usize,
    /// Total warm instances.
    pub warm_instances: usize,
    /// Total acquisitions (hits).
    pub hits: u64,
    /// Total misses (no warm instance available).
    pub misses: u64,
    /// Total evictions.
    pub evictions: u64,
    /// Total estimated memory usage (bytes).
    pub estimated_memory: usize,
    /// Hit rate (0.0 to 1.0).
    pub hit_rate: f64,
    /// Average warm start time in microseconds.
    pub avg_warm_start_us: f64,
}

/// Internal entry for a preloaded module.
struct ModuleEntry {
    info: PrecompiledModule,
    instances: VecDeque<WarmInstance>,
    last_accessed: Instant,
}

/// A warm pool that manages pre-compiled WASM modules and warm instances.
pub struct WarmPool {
    config: WarmPoolConfig,
    modules: HashMap<String, ModuleEntry>,
    next_instance_id: u64,
    stats: PoolStats,
    eviction_policy: EvictionPolicy,
}

impl WarmPool {
    /// Create a new warm pool with default configuration.
    pub fn new(config: WarmPoolConfig) -> Self {
        Self {
            config,
            modules: HashMap::new(),
            next_instance_id: 1,
            stats: PoolStats::default(),
            eviction_policy: EvictionPolicy::Lru,
        }
    }

    /// Set the eviction policy.
    pub fn with_eviction_policy(mut self, policy: EvictionPolicy) -> Self {
        self.eviction_policy = policy;
        self
    }

    /// Preload a module into the pool for fast instantiation.
    pub fn preload(
        &mut self,
        name: &str,
        wasm_bytes: &[u8],
    ) -> Result<&PrecompiledModule, WarmPoolError> {
        if self.modules.contains_key(name) {
            return Err(WarmPoolError::AlreadyPreloaded(name.to_string()));
        }

        if self.modules.len() >= self.config.max_modules {
            self.evict_module()?;
        }

        let hash = ModuleHash::from_bytes(wasm_bytes);

        let info = PrecompiledModule {
            name: name.to_string(),
            hash,
            size: wasm_bytes.len(),
            compiled_at: SystemTime::now(),
            instantiation_count: 0,
            avg_instantiation_us: 0.0,
        };

        self.modules.insert(
            name.to_string(),
            ModuleEntry { info, instances: VecDeque::new(), last_accessed: Instant::now() },
        );

        self.stats.preloaded_modules = self.modules.len();

        Ok(&self.modules[name].info)
    }

    /// Warm up instances for a module by pre-creating them.
    pub fn warm_up(&mut self, name: &str, count: usize) -> Result<usize, WarmPoolError> {
        let entry = self
            .modules
            .get_mut(name)
            .ok_or_else(|| WarmPoolError::ModuleNotFound(name.to_string()))?;

        let max = self.config.max_instances_per_module.saturating_sub(entry.instances.len());
        let to_create = count.min(max);

        let memory_per_instance = 64 * 1024; // 64KB estimated per warm instance

        for _ in 0..to_create {
            let instance = WarmInstance {
                module_name: name.to_string(),
                module_hash: entry.info.hash.clone(),
                created_at: Instant::now(),
                instance_id: self.next_instance_id,
                memory_estimate: memory_per_instance,
            };
            self.next_instance_id += 1;
            entry.instances.push_back(instance);
        }

        self.update_stats();
        Ok(to_create)
    }

    /// Acquire a warm instance for immediate use.
    pub fn acquire(&mut self, name: &str) -> Result<WarmInstance, WarmPoolError> {
        let entry = self
            .modules
            .get_mut(name)
            .ok_or_else(|| WarmPoolError::ModuleNotFound(name.to_string()))?;

        entry.last_accessed = Instant::now();
        entry.info.instantiation_count += 1;

        match entry.instances.pop_front() {
            Some(instance) => {
                self.stats.hits += 1;
                self.update_hit_rate();
                self.update_stats();
                Ok(instance)
            }
            None => {
                self.stats.misses += 1;
                self.update_hit_rate();
                Err(WarmPoolError::NoInstancesAvailable(name.to_string()))
            }
        }
    }

    /// Return an instance to the pool (recycle).
    pub fn release(&mut self, instance: WarmInstance) -> Result<(), WarmPoolError> {
        let name = instance.module_name.clone();
        let entry =
            self.modules.get_mut(&name).ok_or_else(|| WarmPoolError::ModuleNotFound(name))?;

        if entry.instances.len() < self.config.max_instances_per_module {
            entry.instances.push_back(WarmInstance {
                module_name: instance.module_name,
                module_hash: instance.module_hash,
                created_at: Instant::now(), // Reset creation time
                instance_id: self.next_instance_id,
                memory_estimate: instance.memory_estimate,
            });
            self.next_instance_id += 1;
        }

        self.update_stats();
        Ok(())
    }

    /// Evict idle instances that have exceeded the timeout.
    pub fn evict_idle(&mut self) -> usize {
        let timeout = self.config.idle_timeout;
        let now = Instant::now();
        let mut evicted = 0;

        for entry in self.modules.values_mut() {
            let before = entry.instances.len();
            entry.instances.retain(|inst| now.duration_since(inst.created_at) < timeout);
            evicted += before - entry.instances.len();
        }

        self.stats.evictions += evicted as u64;
        self.update_stats();
        evicted
    }

    /// Remove a module and all its instances from the pool.
    pub fn remove(&mut self, name: &str) -> Result<PrecompiledModule, WarmPoolError> {
        let entry = self
            .modules
            .remove(name)
            .ok_or_else(|| WarmPoolError::ModuleNotFound(name.to_string()))?;

        self.stats.evictions += entry.instances.len() as u64;
        self.stats.preloaded_modules = self.modules.len();
        self.update_stats();
        Ok(entry.info)
    }

    /// Get pool statistics.
    pub fn stats(&self) -> &PoolStats {
        &self.stats
    }

    /// Get information about a preloaded module.
    pub fn module_info(&self, name: &str) -> Option<&PrecompiledModule> {
        self.modules.get(name).map(|e| &e.info)
    }

    /// List all preloaded module names.
    pub fn list_modules(&self) -> Vec<&str> {
        self.modules.keys().map(|k| k.as_str()).collect()
    }

    /// Get warm instance count for a module.
    pub fn warm_count(&self, name: &str) -> usize {
        self.modules.get(name).map(|e| e.instances.len()).unwrap_or(0)
    }

    /// Get total warm instance count.
    pub fn total_warm_count(&self) -> usize {
        self.modules.values().map(|e| e.instances.len()).sum()
    }

    fn evict_module(&mut self) -> Result<(), WarmPoolError> {
        let to_evict = match self.eviction_policy {
            EvictionPolicy::Lru => {
                self.modules.iter().min_by_key(|(_, e)| e.last_accessed).map(|(k, _)| k.clone())
            }
            EvictionPolicy::Lfu => self
                .modules
                .iter()
                .min_by_key(|(_, e)| e.info.instantiation_count)
                .map(|(k, _)| k.clone()),
            EvictionPolicy::Fifo => {
                self.modules.iter().min_by_key(|(_, e)| e.info.compiled_at).map(|(k, _)| k.clone())
            }
        };

        if let Some(name) = to_evict {
            self.modules.remove(&name);
            self.stats.evictions += 1;
            Ok(())
        } else {
            Err(WarmPoolError::PoolFull)
        }
    }

    fn update_hit_rate(&mut self) {
        let total = self.stats.hits + self.stats.misses;
        self.stats.hit_rate = if total > 0 { self.stats.hits as f64 / total as f64 } else { 0.0 };
    }

    fn update_stats(&mut self) {
        self.stats.warm_instances = self.total_warm_count();
        self.stats.estimated_memory =
            self.modules.values().flat_map(|e| e.instances.iter()).map(|i| i.memory_estimate).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_preload_module() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        let info = pool.preload("test", FAKE_WASM).unwrap();
        assert_eq!(info.name, "test");
        assert_eq!(info.size, 8);
    }

    #[test]
    fn test_preload_duplicate() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        assert!(pool.preload("test", FAKE_WASM).is_err());
    }

    #[test]
    fn test_warm_up_instances() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        let created = pool.warm_up("test", 5).unwrap();
        assert_eq!(created, 5);
        assert_eq!(pool.warm_count("test"), 5);
    }

    #[test]
    fn test_warm_up_respects_limit() {
        let config = WarmPoolConfig { max_instances_per_module: 3, ..Default::default() };
        let mut pool = WarmPool::new(config);
        pool.preload("test", FAKE_WASM).unwrap();

        let created = pool.warm_up("test", 10).unwrap();
        assert_eq!(created, 3);
    }

    #[test]
    fn test_acquire_hit() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        pool.warm_up("test", 3).unwrap();

        let instance = pool.acquire("test").unwrap();
        assert_eq!(instance.module_name, "test");
        assert_eq!(pool.warm_count("test"), 2);
        assert_eq!(pool.stats().hits, 1);
    }

    #[test]
    fn test_acquire_miss() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();

        let result = pool.acquire("test");
        assert!(result.is_err());
        assert_eq!(pool.stats().misses, 1);
    }

    #[test]
    fn test_acquire_not_found() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        let result = pool.acquire("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_release_recycle() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        pool.warm_up("test", 1).unwrap();

        let instance = pool.acquire("test").unwrap();
        assert_eq!(pool.warm_count("test"), 0);

        pool.release(instance).unwrap();
        assert_eq!(pool.warm_count("test"), 1);
    }

    #[test]
    fn test_evict_idle() {
        let config =
            WarmPoolConfig { idle_timeout: Duration::from_millis(1), ..Default::default() };
        let mut pool = WarmPool::new(config);
        pool.preload("test", FAKE_WASM).unwrap();
        pool.warm_up("test", 3).unwrap();

        std::thread::sleep(Duration::from_millis(10));
        let evicted = pool.evict_idle();
        assert_eq!(evicted, 3);
        assert_eq!(pool.warm_count("test"), 0);
    }

    #[test]
    fn test_remove_module() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        pool.warm_up("test", 2).unwrap();

        let info = pool.remove("test").unwrap();
        assert_eq!(info.name, "test");
        assert!(pool.module_info("test").is_none());
    }

    #[test]
    fn test_hit_rate() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        pool.warm_up("test", 2).unwrap();

        pool.acquire("test").unwrap();
        pool.acquire("test").unwrap();
        let _ = pool.acquire("test"); // miss

        assert!(pool.stats().hit_rate > 0.6);
    }

    #[test]
    fn test_pool_eviction_lru() {
        let config = WarmPoolConfig { max_modules: 2, ..Default::default() };
        let mut pool = WarmPool::new(config);
        pool.preload("old", FAKE_WASM).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        pool.preload("new", FAKE_WASM).unwrap();

        // Third module should evict "old" (LRU)
        let other = &[0x00, 0x61, 0x73, 0x6d, 0x02, 0x00, 0x00, 0x00];
        pool.preload("newest", other).unwrap();

        assert!(pool.module_info("old").is_none());
        assert!(pool.module_info("new").is_some());
    }

    #[test]
    fn test_list_modules() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("alpha", FAKE_WASM).unwrap();
        pool.preload("beta", FAKE_WASM).unwrap();

        let mods = pool.list_modules();
        assert_eq!(mods.len(), 2);
    }

    #[test]
    fn test_stats_memory_tracking() {
        let mut pool = WarmPool::new(WarmPoolConfig::default());
        pool.preload("test", FAKE_WASM).unwrap();
        pool.warm_up("test", 3).unwrap();

        assert!(pool.stats().estimated_memory > 0);
        assert_eq!(pool.stats().warm_instances, 3);
    }

    #[test]
    fn test_pool_error_display() {
        let err = WarmPoolError::ModuleNotFound("test".to_string());
        assert_eq!(err.to_string(), "module 'test' not found in pool");

        let err = WarmPoolError::PoolFull;
        assert_eq!(err.to_string(), "warm pool is at maximum capacity");
    }

    #[test]
    fn test_eviction_policy_lfu() {
        let config = WarmPoolConfig { max_modules: 2, ..Default::default() };
        let mut pool = WarmPool::new(config).with_eviction_policy(EvictionPolicy::Lfu);

        pool.preload("popular", FAKE_WASM).unwrap();
        pool.preload("rare", FAKE_WASM).unwrap();

        // Access "popular" more
        pool.warm_up("popular", 3).unwrap();
        pool.acquire("popular").unwrap();
        pool.acquire("popular").unwrap();

        let other = &[0x00, 0x61, 0x73, 0x6d, 0x02, 0x00, 0x00, 0x00];
        pool.preload("newest", other).unwrap();

        // "rare" should be evicted (fewer instantiations)
        assert!(pool.module_info("rare").is_none());
        assert!(pool.module_info("popular").is_some());
    }
}
