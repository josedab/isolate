//! Pre-initialized module pool for sub-100μs warm starts.
//!
//! Uses Wasmtime's `InstancePre` to pre-link modules so that instantiation
//! skips the linker phase entirely, significantly reducing warm start latency.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::engine::{WasmEngine, PreInitializedPool, PreInitConfig};
//! use std::sync::Arc;
//!
//! let engine = Arc::new(WasmEngine::new()?);
//! let pool = PreInitializedPool::new(engine.clone(), PreInitConfig::default());
//!
//! // Pre-warm a module
//! let wasm = std::fs::read("module.wasm")?;
//! let config = SandboxConfig::builder().module(&wasm)?.build()?;
//! pool.pre_warm(&config).await?;
//!
//! // Fast instantiation (skips linker)
//! let instance = pool.instantiate(&config, enforcer, meter)?;
//! ```

use crate::capability::CapabilityEnforcer;
use crate::config::{ModuleHash, SandboxConfig};
use crate::error::{Error, Result};
use crate::resource::ResourceMeter;

use super::capture::{
    new_capture_buffer, BufferedStdin, CaptureStream, EmptyStdin, NullStream,
};
use super::host::HostState;
use super::wasm::{SandboxWasiState, WasmEngine, WasmInstance};

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use wasmtime::{InstancePre, Linker, Store, StoreLimitsBuilder};
use wasmtime_wasi::preview1;
use wasmtime_wasi::WasiCtxBuilder;

/// Configuration for the pre-initialized pool.
#[derive(Debug, Clone)]
pub struct PreInitConfig {
    /// Maximum number of pre-linked entries to keep.
    pub max_entries: usize,
    /// TTL in seconds before an entry is evicted.
    pub ttl_secs: u64,
}

impl Default for PreInitConfig {
    fn default() -> Self {
        Self {
            max_entries: 64,
            ttl_secs: 600,
        }
    }
}

/// Statistics for the pre-initialized pool.
#[derive(Debug, Clone)]
pub struct PreInitStats {
    /// Total hits (fast instantiations).
    pub hits: u64,
    /// Total misses (fell back to full instantiation).
    pub misses: u64,
    /// Current number of cached entries.
    pub entries: usize,
}

struct PreLinkedEntry {
    instance_pre: InstancePre<SandboxWasiState>,
    created_at: Instant,
}

/// A pool of pre-linked WASM modules for fast instantiation.
///
/// The pool caches `InstancePre` objects keyed by module hash. When a
/// module is requested, the pool skips the linker phase and goes straight
/// to instantiation, which is dramatically faster.
pub struct PreInitializedPool {
    engine: Arc<WasmEngine>,
    entries: DashMap<ModuleHash, PreLinkedEntry>,
    config: PreInitConfig,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl PreInitializedPool {
    /// Create a new pool.
    pub fn new(engine: Arc<WasmEngine>, config: PreInitConfig) -> Self {
        Self {
            engine,
            entries: DashMap::new(),
            config,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Pre-warm a module by pre-linking it.
    pub fn pre_warm(&self, sandbox_config: &SandboxConfig) -> Result<()> {
        let hash = sandbox_config.module.hash().clone();

        if self.entries.contains_key(&hash) {
            return Ok(());
        }

        // Compile the module
        let compiled = self.engine.compile(&sandbox_config.module)?;

        // Create a linker and add WASI
        let mut linker: Linker<SandboxWasiState> = Linker::new(self.engine.engine());
        preview1::add_to_linker_sync(&mut linker, |state: &mut SandboxWasiState| {
            state.wasi_ctx()
        })
        .map_err(|e| Error::Instantiation(e.to_string()))?;

        // Pre-instantiate
        let instance_pre = linker
            .instantiate_pre(&compiled.module_ref())
            .map_err(|e| Error::Instantiation(format!("Pre-instantiation failed: {}", e)))?;

        // Evict oldest if over capacity
        if self.entries.len() >= self.config.max_entries {
            self.evict_oldest();
        }

        self.entries.insert(
            hash,
            PreLinkedEntry {
                instance_pre,
                created_at: Instant::now(),
            },
        );

        Ok(())
    }

    /// Fast-path instantiation using a pre-linked module.
    ///
    /// Returns `None` if no pre-linked entry exists for this module.
    pub fn try_instantiate(
        &self,
        sandbox_config: &SandboxConfig,
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
        input: Option<Vec<u8>>,
    ) -> Result<Option<WasmInstance>> {
        let hash = sandbox_config.module.hash();

        let entry = match self.entries.get(hash) {
            Some(e) => e,
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
        };

        // Check TTL
        if entry.created_at.elapsed().as_secs() > self.config.ttl_secs {
            drop(entry);
            self.entries.remove(hash);
            self.misses.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        self.hits.fetch_add(1, Ordering::Relaxed);

        // Build a store with the correct WASI context
        let stdout_buffer = new_capture_buffer();
        let stderr_buffer = new_capture_buffer();

        let mut wasi_builder = WasiCtxBuilder::new();

        if enforcer.check_stdin().is_ok() {
            if let Some(data) = input {
                if sandbox_config.resources.io.is_limited() {
                    wasi_builder.stdin(BufferedStdin::with_meter(data, meter.clone()));
                } else {
                    wasi_builder.stdin(BufferedStdin::new(data));
                }
            } else {
                wasi_builder.stdin(EmptyStdin);
            }
        } else {
            wasi_builder.stdin(EmptyStdin);
        }

        if enforcer.check_stdout().is_ok() {
            if sandbox_config.resources.io.is_limited() {
                wasi_builder.stdout(CaptureStream::with_meter(stdout_buffer.clone(), meter.clone()));
            } else {
                wasi_builder.stdout(CaptureStream::new(stdout_buffer.clone()));
            }
        } else {
            wasi_builder.stdout(NullStream);
        }

        if enforcer.check_stderr().is_ok() {
            if sandbox_config.resources.io.is_limited() {
                wasi_builder.stderr(CaptureStream::with_meter(stderr_buffer.clone(), meter.clone()));
            } else {
                wasi_builder.stderr(CaptureStream::new(stderr_buffer.clone()));
            }
        } else {
            wasi_builder.stderr(NullStream);
        }

        for (key, value) in &sandbox_config.env {
            if enforcer.check_env_var(key).is_ok() {
                wasi_builder.env(key, value);
            }
        }
        if enforcer.check_args().is_ok() {
            let args: Vec<&str> = sandbox_config.args.iter().map(|s| s.as_str()).collect();
            wasi_builder.args(&args);
        }

        let wasi = wasi_builder.build_p1();
        let host = HostState::new(enforcer, meter);
        let initial_fuel = sandbox_config.resources.cpu.fuel;
        let limits = StoreLimitsBuilder::new()
            .memory_size(sandbox_config.resources.memory.heap_max)
            .trap_on_grow_failure(true)
            .build();

        let state = SandboxWasiState::new(wasi, host, initial_fuel, limits);
        let mut store = Store::new(self.engine.engine(), state);
        store.limiter(|s| s.store_limits());

        if let Some(fuel) = sandbox_config.resources.cpu.fuel {
            store.set_fuel(fuel).map_err(|e| Error::Engine(e.to_string()))?;
        }
        if self.engine.config().enable_epoch_interruption {
            store.epoch_deadline_trap();
            store.set_epoch_deadline(u64::MAX);
        }

        let instance = entry
            .instance_pre
            .instantiate(&mut store)
            .map_err(|e| Error::Instantiation(format!("Pre-init instantiation failed: {}", e)))?;

        Ok(Some(WasmInstance::from_parts(
            store,
            instance,
            sandbox_config.entry_point.clone(),
            stdout_buffer,
            stderr_buffer,
        )))
    }

    /// Get pool statistics.
    pub fn stats(&self) -> PreInitStats {
        PreInitStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: self.entries.len(),
        }
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.clear();
    }

    fn evict_oldest(&self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|e| e.value().created_at)
            .map(|e| e.key().clone());
        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pre_init_config_default() {
        let config = PreInitConfig::default();
        assert_eq!(config.max_entries, 64);
        assert_eq!(config.ttl_secs, 600);
    }

    #[test]
    fn test_pool_creation() {
        let engine = Arc::new(WasmEngine::new().unwrap());
        let pool = PreInitializedPool::new(engine, PreInitConfig::default());
        let stats = pool.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.entries, 0);
    }

    #[test]
    fn test_pool_clear() {
        let engine = Arc::new(WasmEngine::new().unwrap());
        let pool = PreInitializedPool::new(engine, PreInitConfig::default());
        pool.clear();
        assert_eq!(pool.stats().entries, 0);
    }
}
