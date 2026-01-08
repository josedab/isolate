//! Module version management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Version of a WASM module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleVersion {
    /// Version hash.
    pub hash: String,
    /// Version number.
    pub number: u64,
    /// Creation timestamp.
    pub created_at: SystemTime,
    /// Module size.
    pub size: usize,
    /// Semantic version (if available).
    pub semver: Option<String>,
}

impl ModuleVersion {
    /// Create a new version from module bytes.
    pub fn new(bytes: &[u8]) -> Self {
        Self {
            hash: compute_hash(bytes),
            number: 0,
            created_at: SystemTime::now(),
            size: bytes.len(),
            semver: None,
        }
    }

    /// Create with a version number.
    pub fn with_number(bytes: &[u8], number: u64) -> Self {
        let mut v = Self::new(bytes);
        v.number = number;
        v
    }

    /// Set semantic version.
    pub fn with_semver(mut self, semver: impl Into<String>) -> Self {
        self.semver = Some(semver.into());
        self
    }

    /// Get short hash.
    pub fn short_hash(&self) -> &str {
        &self.hash[..8.min(self.hash.len())]
    }

    /// Check if this version is newer than another.
    pub fn is_newer_than(&self, other: &ModuleVersion) -> bool {
        self.number > other.number || self.created_at > other.created_at
    }
}

/// History of versions for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionHistory {
    /// All versions in order.
    versions: Vec<ModuleVersion>,
    /// Current version index.
    current_index: usize,
    /// Maximum history length.
    max_history: usize,
}

impl VersionHistory {
    /// Create a new version history.
    pub fn new(max_history: usize) -> Self {
        Self {
            versions: Vec::new(),
            current_index: 0,
            max_history,
        }
    }

    /// Add a new version.
    pub fn push(&mut self, version: ModuleVersion) {
        // If we're not at the end, truncate forward history
        if self.current_index < self.versions.len() {
            self.versions.truncate(self.current_index + 1);
        }

        self.versions.push(version);
        self.current_index = self.versions.len() - 1;

        // Trim old versions
        while self.versions.len() > self.max_history {
            self.versions.remove(0);
            if self.current_index > 0 {
                self.current_index -= 1;
            }
        }
    }

    /// Get current version.
    pub fn current(&self) -> Option<&ModuleVersion> {
        self.versions.get(self.current_index)
    }

    /// Get previous version.
    pub fn previous(&self) -> Option<&ModuleVersion> {
        if self.current_index > 0 {
            self.versions.get(self.current_index - 1)
        } else {
            None
        }
    }

    /// Go back to previous version.
    pub fn go_back(&mut self) -> Option<&ModuleVersion> {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.versions.get(self.current_index)
        } else {
            None
        }
    }

    /// Go forward to next version.
    pub fn go_forward(&mut self) -> Option<&ModuleVersion> {
        if self.current_index < self.versions.len() - 1 {
            self.current_index += 1;
            self.versions.get(self.current_index)
        } else {
            None
        }
    }

    /// Get version at index.
    pub fn get(&self, index: usize) -> Option<&ModuleVersion> {
        self.versions.get(index)
    }

    /// Get all versions.
    pub fn all(&self) -> &[ModuleVersion] {
        &self.versions
    }

    /// Get version count.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// Can go back.
    pub fn can_go_back(&self) -> bool {
        self.current_index > 0
    }

    /// Can go forward.
    pub fn can_go_forward(&self) -> bool {
        self.current_index < self.versions.len().saturating_sub(1)
    }
}

/// Manages versions across multiple sandboxes.
pub struct VersionManager {
    /// Version history per sandbox.
    histories: HashMap<String, VersionHistory>,
    /// Default max history.
    default_max_history: usize,
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionManager {
    /// Create a new version manager.
    pub fn new() -> Self {
        Self {
            histories: HashMap::new(),
            default_max_history: 10,
        }
    }

    /// Set default max history.
    pub fn with_max_history(mut self, max: usize) -> Self {
        self.default_max_history = max;
        self
    }

    /// Register a new version.
    pub fn register_version(&mut self, sandbox_id: String, mut version: ModuleVersion) {
        let history = self
            .histories
            .entry(sandbox_id)
            .or_insert_with(|| VersionHistory::new(self.default_max_history));

        // Set version number
        version.number = history.len() as u64 + 1;
        history.push(version);
    }

    /// Get current version of a sandbox.
    pub fn current_version(&self, sandbox_id: &str) -> Option<&ModuleVersion> {
        self.histories.get(sandbox_id)?.current()
    }

    /// Get previous version of a sandbox.
    pub fn previous_version(&self, sandbox_id: &str) -> Option<&ModuleVersion> {
        self.histories.get(sandbox_id)?.previous()
    }

    /// Get version history of a sandbox.
    pub fn history(&self, sandbox_id: &str) -> Option<&VersionHistory> {
        self.histories.get(sandbox_id)
    }

    /// Get mutable version history.
    pub fn history_mut(&mut self, sandbox_id: &str) -> Option<&mut VersionHistory> {
        self.histories.get_mut(sandbox_id)
    }

    /// Get all sandbox IDs.
    pub fn sandbox_ids(&self) -> impl Iterator<Item = &String> {
        self.histories.keys()
    }

    /// Get total version count.
    pub fn total_versions(&self) -> usize {
        self.histories.values().map(|h| h.len()).sum()
    }

    /// Clear history for a sandbox.
    pub fn clear(&mut self, sandbox_id: &str) {
        self.histories.remove(sandbox_id);
    }
}

/// Compute hash of bytes.
fn compute_hash(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_version() {
        let v1 = ModuleVersion::new(b"hello");
        let v2 = ModuleVersion::new(b"world");

        assert_ne!(v1.hash, v2.hash);
        assert_eq!(v1.size, 5);
    }

    #[test]
    fn test_module_version_semver() {
        let v = ModuleVersion::new(b"test").with_semver("1.0.0");

        assert_eq!(v.semver, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_version_history() {
        let mut history = VersionHistory::new(5);

        history.push(ModuleVersion::new(b"v1"));
        history.push(ModuleVersion::new(b"v2"));
        history.push(ModuleVersion::new(b"v3"));

        assert_eq!(history.len(), 3);
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        history.go_back();
        assert!(history.can_go_forward());
    }

    #[test]
    fn test_version_history_max() {
        let mut history = VersionHistory::new(3);

        for i in 0..5 {
            history.push(ModuleVersion::new(format!("v{}", i).as_bytes()));
        }

        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_version_manager() {
        let mut vm = VersionManager::new();

        vm.register_version("sb-1".to_string(), ModuleVersion::new(b"v1"));
        vm.register_version("sb-1".to_string(), ModuleVersion::new(b"v2"));
        vm.register_version("sb-2".to_string(), ModuleVersion::new(b"v1"));

        assert_eq!(vm.total_versions(), 3);
        assert!(vm.current_version("sb-1").is_some());
        assert!(vm.previous_version("sb-1").is_some());
    }

    #[test]
    fn test_version_ordering() {
        let v1 = ModuleVersion::with_number(b"test", 1);
        let v2 = ModuleVersion::with_number(b"test", 2);

        assert!(v2.is_newer_than(&v1));
        assert!(!v1.is_newer_than(&v2));
    }
}
