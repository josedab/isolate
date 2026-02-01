//! Content-addressed module storage with OCI-compatible digest references.
//!
//! Provides a storage layer for WASM modules indexed by their SHA-256 digest,
//! with OCI-compatible push/pull operations and tag management.
//!
//! ```rust
//! use isolate_core::marketplace::content_store::{
//!     ContentStore, ModuleDigest, ModuleTag, StoredModule,
//! };
//!
//! let mut store = ContentStore::new(1024 * 1024 * 100); // 100 MB max
//!
//! let bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
//! let digest = store.push("my-module", "1.0.0", &bytes).unwrap();
//! assert!(digest.as_str().starts_with("sha256:"));
//!
//! let module = store.pull(&digest).unwrap();
//! assert_eq!(module.bytes, bytes);
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::SystemTime;

/// SHA-256 content digest (OCI-compatible format: `sha256:<hex>`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleDigest(String);

impl ModuleDigest {
    /// Compute digest from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hasher.finalize();
        Self(format!("sha256:{}", hex::encode(hash)))
    }

    /// Get the digest string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get just the hex hash portion.
    pub fn hex(&self) -> &str {
        self.0.strip_prefix("sha256:").unwrap_or(&self.0)
    }

    /// Short digest (first 12 characters of hex).
    pub fn short(&self) -> String {
        let h = self.hex();
        if h.len() > 12 {
            h[..12].to_string()
        } else {
            h.to_string()
        }
    }
}

impl std::fmt::Display for ModuleDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A tagged reference to a module (e.g., "my-module:1.0.0").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleTag {
    /// Module name.
    pub name: String,
    /// Tag (version or label).
    pub tag: String,
}

impl ModuleTag {
    pub fn new(name: impl Into<String>, tag: impl Into<String>) -> Self {
        Self { name: name.into(), tag: tag.into() }
    }

    /// Parse "name:tag" format.
    pub fn parse(reference: &str) -> Option<Self> {
        let parts: Vec<&str> = reference.splitn(2, ':').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            Some(Self { name: parts[0].to_string(), tag: parts[1].to_string() })
        } else {
            None
        }
    }
}

impl std::fmt::Display for ModuleTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.tag)
    }
}

/// A module stored in the content-addressed store.
#[derive(Debug, Clone)]
pub struct StoredModule {
    /// The WASM bytes.
    pub bytes: Vec<u8>,
    /// Content digest.
    pub digest: ModuleDigest,
    /// Size in bytes.
    pub size: usize,
    /// Upload timestamp.
    pub uploaded_at: SystemTime,
    /// Tags pointing to this content.
    pub tags: Vec<ModuleTag>,
    /// Optional signature digest.
    pub signature: Option<String>,
}

/// Store error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Module not found.
    NotFound(String),
    /// Store capacity exceeded.
    CapacityExceeded { current: usize, max: usize },
    /// Invalid module (not valid WASM).
    InvalidModule(String),
    /// Tag already exists pointing to different content.
    TagConflict { tag: String, existing_digest: String },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(d) => write!(f, "Module not found: {}", d),
            Self::CapacityExceeded { current, max } => {
                write!(f, "Store capacity exceeded: {} / {} bytes", current, max)
            }
            Self::InvalidModule(msg) => write!(f, "Invalid module: {}", msg),
            Self::TagConflict { tag, existing_digest } => {
                write!(f, "Tag '{}' already points to {}", tag, existing_digest)
            }
        }
    }
}

/// Content-addressed module store.
pub struct ContentStore {
    /// Modules indexed by digest.
    modules: HashMap<ModuleDigest, StoredModule>,
    /// Tag-to-digest index.
    tag_index: HashMap<ModuleTag, ModuleDigest>,
    /// Maximum total storage in bytes.
    max_bytes: usize,
    /// Current storage usage.
    current_bytes: usize,
    /// Total pushes.
    total_pushes: u64,
    /// Total pulls.
    total_pulls: u64,
}

impl ContentStore {
    /// Create a new content store with a maximum size.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            modules: HashMap::new(),
            tag_index: HashMap::new(),
            max_bytes,
            current_bytes: 0,
            total_pushes: 0,
            total_pulls: 0,
        }
    }

    /// Push a module into the store, returning its digest.
    pub fn push(
        &mut self,
        name: &str,
        tag: &str,
        bytes: &[u8],
    ) -> Result<ModuleDigest, StoreError> {
        // Validate WASM magic number
        if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
            return Err(StoreError::InvalidModule("Not a valid WASM binary".to_string()));
        }

        let digest = ModuleDigest::from_bytes(bytes);
        let module_tag = ModuleTag::new(name, tag);

        // Check capacity
        if !self.modules.contains_key(&digest) && self.current_bytes + bytes.len() > self.max_bytes
        {
            return Err(StoreError::CapacityExceeded {
                current: self.current_bytes,
                max: self.max_bytes,
            });
        }

        // Check for tag conflicts
        if let Some(existing_digest) = self.tag_index.get(&module_tag) {
            if existing_digest != &digest {
                return Err(StoreError::TagConflict {
                    tag: module_tag.to_string(),
                    existing_digest: existing_digest.to_string(),
                });
            }
            // Same content, tag already exists — no-op
            return Ok(digest);
        }

        // Store or update the module
        if let Some(existing) = self.modules.get_mut(&digest) {
            // Content already exists, just add tag
            existing.tags.push(module_tag.clone());
        } else {
            // New content
            let stored = StoredModule {
                bytes: bytes.to_vec(),
                digest: digest.clone(),
                size: bytes.len(),
                uploaded_at: SystemTime::now(),
                tags: vec![module_tag.clone()],
                signature: None,
            };
            self.current_bytes += bytes.len();
            self.modules.insert(digest.clone(), stored);
        }

        self.tag_index.insert(module_tag, digest.clone());
        self.total_pushes += 1;

        Ok(digest)
    }

    /// Pull a module by digest.
    pub fn pull(&mut self, digest: &ModuleDigest) -> Result<StoredModule, StoreError> {
        self.total_pulls += 1;
        self.modules.get(digest).cloned().ok_or_else(|| StoreError::NotFound(digest.to_string()))
    }

    /// Resolve a tag to a digest.
    pub fn resolve_tag(&self, name: &str, tag: &str) -> Option<&ModuleDigest> {
        let module_tag = ModuleTag::new(name, tag);
        self.tag_index.get(&module_tag)
    }

    /// Pull by tag reference.
    pub fn pull_by_tag(&mut self, name: &str, tag: &str) -> Result<StoredModule, StoreError> {
        let digest = self
            .resolve_tag(name, tag)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("{}:{}", name, tag)))?;
        self.pull(&digest)
    }

    /// Delete a module by digest.
    pub fn delete(&mut self, digest: &ModuleDigest) -> bool {
        if let Some(module) = self.modules.remove(digest) {
            self.current_bytes -= module.size;
            // Remove all tags pointing to this digest
            self.tag_index.retain(|_, d| d != digest);
            true
        } else {
            false
        }
    }

    /// List all tags for a given module name.
    pub fn list_tags(&self, name: &str) -> Vec<String> {
        self.tag_index.keys().filter(|t| t.name == name).map(|t| t.tag.clone()).collect()
    }

    /// List all module names.
    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tag_index
            .keys()
            .map(|t| t.name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        names.sort();
        names
    }

    /// Get store statistics.
    pub fn stats(&self) -> StoreStats {
        StoreStats {
            total_modules: self.modules.len(),
            total_tags: self.tag_index.len(),
            used_bytes: self.current_bytes,
            max_bytes: self.max_bytes,
            utilization: if self.max_bytes > 0 {
                self.current_bytes as f64 / self.max_bytes as f64
            } else {
                0.0
            },
            total_pushes: self.total_pushes,
            total_pulls: self.total_pulls,
        }
    }

    /// Attach a signature to a stored module.
    pub fn attach_signature(
        &mut self,
        digest: &ModuleDigest,
        signature: String,
    ) -> Result<(), StoreError> {
        let module =
            self.modules.get_mut(digest).ok_or_else(|| StoreError::NotFound(digest.to_string()))?;
        module.signature = Some(signature);
        Ok(())
    }
}

/// Store statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStats {
    pub total_modules: usize,
    pub total_tags: usize,
    pub used_bytes: usize,
    pub max_bytes: usize,
    pub utilization: f64,
    pub total_pushes: u64,
    pub total_pulls: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_digest_from_bytes() {
        let digest = ModuleDigest::from_bytes(VALID_WASM);
        assert!(digest.as_str().starts_with("sha256:"));
        assert_eq!(digest.hex().len(), 64);
    }

    #[test]
    fn test_digest_short() {
        let digest = ModuleDigest::from_bytes(VALID_WASM);
        assert_eq!(digest.short().len(), 12);
    }

    #[test]
    fn test_digest_deterministic() {
        let d1 = ModuleDigest::from_bytes(VALID_WASM);
        let d2 = ModuleDigest::from_bytes(VALID_WASM);
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_module_tag_parse() {
        let tag = ModuleTag::parse("my-module:1.0.0").unwrap();
        assert_eq!(tag.name, "my-module");
        assert_eq!(tag.tag, "1.0.0");
    }

    #[test]
    fn test_module_tag_parse_invalid() {
        assert!(ModuleTag::parse("no-tag").is_none());
        assert!(ModuleTag::parse(":tag").is_none());
        assert!(ModuleTag::parse("name:").is_none());
    }

    #[test]
    fn test_push_and_pull() {
        let mut store = ContentStore::new(1024 * 1024);
        let digest = store.push("test", "1.0.0", VALID_WASM).unwrap();

        let module = store.pull(&digest).unwrap();
        assert_eq!(module.bytes, VALID_WASM);
        assert_eq!(module.size, VALID_WASM.len());
    }

    #[test]
    fn test_push_invalid_wasm() {
        let mut store = ContentStore::new(1024 * 1024);
        let result = store.push("test", "1.0.0", b"not wasm");
        assert!(matches!(result, Err(StoreError::InvalidModule(_))));
    }

    #[test]
    fn test_push_capacity_exceeded() {
        let mut store = ContentStore::new(4); // Very small
        let result = store.push("test", "1.0.0", VALID_WASM);
        assert!(matches!(result, Err(StoreError::CapacityExceeded { .. })));
    }

    #[test]
    fn test_push_duplicate_content() {
        let mut store = ContentStore::new(1024 * 1024);
        let d1 = store.push("test", "1.0.0", VALID_WASM).unwrap();
        // Same content, different tag
        let d2 = store.push("test", "latest", VALID_WASM).unwrap();
        assert_eq!(d1, d2);

        let stats = store.stats();
        assert_eq!(stats.total_modules, 1); // Deduplicated
        assert_eq!(stats.total_tags, 2);
    }

    #[test]
    fn test_tag_conflict() {
        let mut store = ContentStore::new(1024 * 1024);
        store.push("test", "1.0.0", VALID_WASM).unwrap();

        // Different content, same tag
        let different_wasm = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0xFF];
        let result = store.push("test", "1.0.0", different_wasm);
        assert!(matches!(result, Err(StoreError::TagConflict { .. })));
    }

    #[test]
    fn test_pull_by_tag() {
        let mut store = ContentStore::new(1024 * 1024);
        store.push("test", "1.0.0", VALID_WASM).unwrap();

        let module = store.pull_by_tag("test", "1.0.0").unwrap();
        assert_eq!(module.bytes, VALID_WASM);
    }

    #[test]
    fn test_pull_not_found() {
        let mut store = ContentStore::new(1024 * 1024);
        let digest = ModuleDigest::from_bytes(b"nonexistent");
        assert!(matches!(store.pull(&digest), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn test_delete() {
        let mut store = ContentStore::new(1024 * 1024);
        let digest = store.push("test", "1.0.0", VALID_WASM).unwrap();

        assert!(store.delete(&digest));
        assert!(store.pull(&digest).is_err());
        assert_eq!(store.stats().total_modules, 0);
    }

    #[test]
    fn test_list_tags() {
        let mut store = ContentStore::new(1024 * 1024);
        store.push("test", "1.0.0", VALID_WASM).unwrap();
        store.push("test", "latest", VALID_WASM).unwrap();

        let mut tags = store.list_tags("test");
        tags.sort();
        assert_eq!(tags, vec!["1.0.0", "latest"]);
    }

    #[test]
    fn test_list_names() {
        let mut store = ContentStore::new(1024 * 1024);
        store.push("alpha", "1.0", VALID_WASM).unwrap();
        store.push("beta", "1.0", VALID_WASM).unwrap();

        let names = store.list_names();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_store_stats() {
        let mut store = ContentStore::new(1024 * 1024);
        store.push("test", "1.0.0", VALID_WASM).unwrap();
        let _ = store.pull_by_tag("test", "1.0.0");

        let stats = store.stats();
        assert_eq!(stats.total_modules, 1);
        assert_eq!(stats.total_pushes, 1);
        assert_eq!(stats.total_pulls, 1);
        assert!(stats.utilization > 0.0);
    }

    #[test]
    fn test_attach_signature() {
        let mut store = ContentStore::new(1024 * 1024);
        let digest = store.push("test", "1.0.0", VALID_WASM).unwrap();

        store.attach_signature(&digest, "sig_abc123".to_string()).unwrap();
        let module = store.pull(&digest).unwrap();
        assert_eq!(module.signature, Some("sig_abc123".to_string()));
    }

    #[test]
    fn test_store_error_display() {
        let err = StoreError::NotFound("sha256:abc".to_string());
        assert!(err.to_string().contains("not found"));

        let err = StoreError::CapacityExceeded { current: 100, max: 50 };
        assert!(err.to_string().contains("exceeded"));
    }
}
