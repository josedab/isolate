use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

/// Unique identifier for a module version.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct VersionId(String);

impl VersionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A versioned WASM module with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleVersion {
    pub id: VersionId,
    pub content_hash: String,
    pub size_bytes: usize,
    pub created_at_epoch_ms: u64,
    #[serde(skip)]
    module_bytes: Arc<Vec<u8>>,
}

impl ModuleVersion {
    pub fn new(version: impl Into<String>, wasm_bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            id: VersionId::new(version),
            content_hash: hash,
            size_bytes: wasm_bytes.len(),
            created_at_epoch_ms: now,
            module_bytes: Arc::new(wasm_bytes.to_vec()),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.module_bytes
    }
}

/// Registry of all known module versions.
pub struct VersionRegistry {
    versions: dashmap::DashMap<VersionId, ModuleVersion>,
}

impl VersionRegistry {
    pub fn new() -> Self {
        Self {
            versions: dashmap::DashMap::new(),
        }
    }

    pub fn register(&self, version: ModuleVersion) {
        self.versions.insert(version.id.clone(), version);
    }

    pub fn get(&self, id: &VersionId) -> Option<ModuleVersion> {
        self.versions.get(id).map(|v| v.value().clone())
    }

    pub fn remove(&self, id: &VersionId) -> Option<ModuleVersion> {
        self.versions.remove(id).map(|(_, v)| v)
    }

    pub fn list(&self) -> Vec<ModuleVersion> {
        self.versions.iter().map(|e| e.value().clone()).collect()
    }

    pub fn count(&self) -> usize {
        self.versions.len()
    }
}

impl Default for VersionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Traffic routing configuration between versions.
#[derive(Debug, Clone)]
pub enum VersionRoute {
    /// Route all traffic to a single version.
    Single(VersionId),
    /// Canary deployment: route `canary_pct`% to new, rest to primary.
    Canary {
        primary: VersionId,
        canary: VersionId,
        canary_pct: u8,
    },
}

impl VersionRoute {
    pub fn single(id: VersionId) -> Self {
        Self::Single(id)
    }

    pub fn canary(primary: VersionId, canary: VersionId, pct: u8) -> Self {
        Self::Canary {
            primary,
            canary,
            canary_pct: pct.min(100),
        }
    }
}

/// Weighted version router for traffic splitting.
pub struct VersionRouter {
    route: parking_lot::RwLock<Option<VersionRoute>>,
    counter: std::sync::atomic::AtomicU64,
}

impl VersionRouter {
    pub fn new() -> Self {
        Self {
            route: parking_lot::RwLock::new(None),
            counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn set_route(&self, route: VersionRoute) {
        *self.route.write() = Some(route);
    }

    /// Resolve which version should handle the next request.
    pub fn resolve(&self) -> Option<VersionId> {
        let route = self.route.read();
        match route.as_ref()? {
            VersionRoute::Single(id) => Some(id.clone()),
            VersionRoute::Canary {
                primary,
                canary,
                canary_pct,
            } => {
                let n = self
                    .counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if (n % 100) < (*canary_pct as u64) {
                    Some(canary.clone())
                } else {
                    Some(primary.clone())
                }
            }
        }
    }

    pub fn current_route(&self) -> Option<VersionRoute> {
        self.route.read().clone()
    }
}

impl Default for VersionRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_id() {
        let v1 = VersionId::new("v1.0.0");
        let v2 = VersionId::new("v1.0.0");
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_module_version_hash() {
        let v1 = ModuleVersion::new("v1", b"hello");
        let v2 = ModuleVersion::new("v2", b"hello");
        assert_eq!(v1.content_hash, v2.content_hash); // same content = same hash
    }

    #[test]
    fn test_module_version_different_content() {
        let v1 = ModuleVersion::new("v1", b"hello");
        let v2 = ModuleVersion::new("v2", b"world");
        assert_ne!(v1.content_hash, v2.content_hash);
    }

    #[test]
    fn test_registry_crud() {
        let reg = VersionRegistry::new();
        let v = ModuleVersion::new("v1", b"bytes");
        reg.register(v.clone());

        assert_eq!(reg.count(), 1);
        assert!(reg.get(&VersionId::new("v1")).is_some());
        assert!(reg.get(&VersionId::new("v2")).is_none());

        reg.remove(&VersionId::new("v1"));
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn test_router_single() {
        let router = VersionRouter::new();
        router.set_route(VersionRoute::single(VersionId::new("v1")));

        for _ in 0..10 {
            assert_eq!(router.resolve().unwrap(), VersionId::new("v1"));
        }
    }

    #[test]
    fn test_router_canary_distribution() {
        let router = VersionRouter::new();
        let v1 = VersionId::new("v1");
        let v2 = VersionId::new("v2");
        router.set_route(VersionRoute::canary(v1.clone(), v2.clone(), 50));

        let mut v1_count = 0u32;
        let mut v2_count = 0u32;
        for _ in 0..100 {
            match router.resolve().unwrap() {
                id if id == v1 => v1_count += 1,
                id if id == v2 => v2_count += 1,
                _ => panic!("unexpected version"),
            }
        }
        assert_eq!(v1_count, 50);
        assert_eq!(v2_count, 50);
    }

    #[test]
    fn test_router_empty() {
        let router = VersionRouter::new();
        assert!(router.resolve().is_none());
    }
}
