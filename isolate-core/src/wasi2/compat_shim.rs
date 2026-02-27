//! Compatibility shim for running WASI Preview 1 modules in Preview 2 contexts.
//!
//! Provides interface mapping between Preview 1 and Preview 2 APIs,
//! allowing Preview 1 modules to work transparently with the Preview 2 runtime.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mapping from a Preview 1 import to its Preview 2 equivalent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceMapping {
    pub preview1_module: String,
    pub preview1_name: String,
    pub preview2_interface: String,
    pub preview2_function: String,
    pub notes: String,
}

/// Compatibility shim that translates Preview 1 calls to Preview 2.
pub struct CompatibilityShim {
    mappings: HashMap<(String, String), InterfaceMapping>,
    unmapped_imports: parking_lot::Mutex<Vec<(String, String)>>,
}

impl CompatibilityShim {
    /// Create a new shim with standard WASI Preview 1 → Preview 2 mappings.
    pub fn new() -> Self {
        let mut shim = Self {
            mappings: HashMap::new(),
            unmapped_imports: parking_lot::Mutex::new(Vec::new()),
        };
        shim.register_standard_mappings();
        shim
    }

    fn register_standard_mappings(&mut self) {
        let standard = vec![
            // Filesystem
            (
                "wasi_snapshot_preview1",
                "fd_read",
                "wasi:filesystem/types",
                "read",
                "Preview 1 fd_read → Preview 2 read via resource handle",
            ),
            (
                "wasi_snapshot_preview1",
                "fd_write",
                "wasi:filesystem/types",
                "write",
                "Preview 1 fd_write → Preview 2 write via resource handle",
            ),
            (
                "wasi_snapshot_preview1",
                "fd_close",
                "wasi:filesystem/types",
                "drop-descriptor",
                "Preview 1 fd_close → Preview 2 resource drop",
            ),
            (
                "wasi_snapshot_preview1",
                "fd_seek",
                "wasi:filesystem/types",
                "seek",
                "Preview 1 fd_seek → Preview 2 seek with stream position",
            ),
            (
                "wasi_snapshot_preview1",
                "path_open",
                "wasi:filesystem/types",
                "open-at",
                "Preview 1 path_open → Preview 2 open-at via descriptor",
            ),
            // Clocks
            (
                "wasi_snapshot_preview1",
                "clock_time_get",
                "wasi:clocks/wall-clock",
                "now",
                "Preview 1 clock_time_get → Preview 2 wall-clock.now or monotonic-clock.now",
            ),
            // Random
            (
                "wasi_snapshot_preview1",
                "random_get",
                "wasi:random/random",
                "get-random-bytes",
                "Preview 1 random_get → Preview 2 get-random-bytes",
            ),
            // Environment
            (
                "wasi_snapshot_preview1",
                "environ_get",
                "wasi:cli/environment",
                "get-environment",
                "Preview 1 environ_get → Preview 2 get-environment",
            ),
            (
                "wasi_snapshot_preview1",
                "environ_sizes_get",
                "wasi:cli/environment",
                "get-environment",
                "Merged into get-environment in Preview 2",
            ),
            // Process
            (
                "wasi_snapshot_preview1",
                "proc_exit",
                "wasi:cli/exit",
                "exit",
                "Preview 1 proc_exit → Preview 2 exit",
            ),
            (
                "wasi_snapshot_preview1",
                "args_get",
                "wasi:cli/environment",
                "get-arguments",
                "Preview 1 args_get → Preview 2 get-arguments",
            ),
            // I/O
            (
                "wasi_snapshot_preview1",
                "fd_prestat_get",
                "wasi:filesystem/preopens",
                "get-directories",
                "Preopens are handled differently in Preview 2",
            ),
        ];

        for (p1_mod, p1_name, p2_iface, p2_func, notes) in standard {
            self.add_mapping(p1_mod, p1_name, p2_iface, p2_func, notes);
        }
    }

    /// Add a custom import mapping.
    pub fn add_mapping(
        &mut self,
        p1_module: &str,
        p1_name: &str,
        p2_interface: &str,
        p2_function: &str,
        notes: &str,
    ) {
        let key = (p1_module.to_string(), p1_name.to_string());
        self.mappings.insert(
            key,
            InterfaceMapping {
                preview1_module: p1_module.to_string(),
                preview1_name: p1_name.to_string(),
                preview2_interface: p2_interface.to_string(),
                preview2_function: p2_function.to_string(),
                notes: notes.to_string(),
            },
        );
    }

    /// Look up the Preview 2 equivalent of a Preview 1 import.
    pub fn translate(&self, module: &str, name: &str) -> Option<&InterfaceMapping> {
        let key = (module.to_string(), name.to_string());
        let result = self.mappings.get(&key);
        if result.is_none() {
            self.unmapped_imports.lock().push((module.to_string(), name.to_string()));
        }
        result
    }

    /// Check if a Preview 1 import has a known Preview 2 mapping.
    pub fn has_mapping(&self, module: &str, name: &str) -> bool {
        self.mappings.contains_key(&(module.to_string(), name.to_string()))
    }

    /// Get all registered mappings.
    pub fn all_mappings(&self) -> Vec<&InterfaceMapping> {
        self.mappings.values().collect()
    }

    /// Get imports that were queried but had no mapping.
    pub fn unmapped_imports(&self) -> Vec<(String, String)> {
        self.unmapped_imports.lock().clone()
    }

    /// Number of registered mappings.
    pub fn mapping_count(&self) -> usize {
        self.mappings.len()
    }
}

impl Default for CompatibilityShim {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_mappings() {
        let shim = CompatibilityShim::new();
        assert!(shim.mapping_count() > 0);
    }

    #[test]
    fn test_translate_fd_read() {
        let shim = CompatibilityShim::new();
        let mapping = shim.translate("wasi_snapshot_preview1", "fd_read").unwrap();
        assert_eq!(mapping.preview2_interface, "wasi:filesystem/types");
        assert_eq!(mapping.preview2_function, "read");
    }

    #[test]
    fn test_translate_clock() {
        let shim = CompatibilityShim::new();
        let mapping = shim.translate("wasi_snapshot_preview1", "clock_time_get").unwrap();
        assert_eq!(mapping.preview2_interface, "wasi:clocks/wall-clock");
    }

    #[test]
    fn test_translate_unknown() {
        let shim = CompatibilityShim::new();
        assert!(shim.translate("wasi_snapshot_preview1", "nonexistent").is_none());
        assert_eq!(shim.unmapped_imports().len(), 1);
    }

    #[test]
    fn test_has_mapping() {
        let shim = CompatibilityShim::new();
        assert!(shim.has_mapping("wasi_snapshot_preview1", "fd_write"));
        assert!(!shim.has_mapping("wasi_snapshot_preview1", "custom_func"));
    }

    #[test]
    fn test_custom_mapping() {
        let mut shim = CompatibilityShim::new();
        let initial = shim.mapping_count();
        shim.add_mapping(
            "custom_module",
            "my_func",
            "custom:interface/types",
            "my-func",
            "custom mapping",
        );
        assert_eq!(shim.mapping_count(), initial + 1);
        assert!(shim.has_mapping("custom_module", "my_func"));
    }

    #[test]
    fn test_translate_proc_exit() {
        let shim = CompatibilityShim::new();
        let mapping = shim.translate("wasi_snapshot_preview1", "proc_exit").unwrap();
        assert_eq!(mapping.preview2_interface, "wasi:cli/exit");
        assert_eq!(mapping.preview2_function, "exit");
    }

    #[test]
    fn test_all_standard_functions_covered() {
        let shim = CompatibilityShim::new();
        let critical = [
            "fd_read",
            "fd_write",
            "fd_close",
            "clock_time_get",
            "random_get",
            "environ_get",
            "proc_exit",
        ];
        for func in critical {
            assert!(shim.has_mapping("wasi_snapshot_preview1", func), "missing mapping for {func}");
        }
    }
}
