//! State migration and compatibility verification for hot code patching.
//!
//! Provides safe state migration between module versions:
//! - State compatibility analysis between old and new modules
//! - Memory layout migration with transformation rules
//! - Global variable remapping
//! - Rollback safety verification
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::hotpatch::migration::*;
//!
//! let analyzer = CompatibilityAnalyzer::new();
//! let report = analyzer.check(&old_module, &new_module);
//!
//! if report.is_compatible() {
//!     let migrator = StateMigrator::new(report.migration_plan());
//!     let new_state = migrator.migrate(&captured_state)?;
//! }
//! ```

use super::{CapturedState, GlobalValue, ValueType};

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of a compatibility analysis between two module versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityReport {
    /// Whether the modules are compatible for live migration.
    pub compatible: bool,
    /// Compatibility level.
    pub level: CompatibilityLevel,
    /// Memory layout changes detected.
    pub memory_changes: Vec<MemoryChange>,
    /// Global variable changes.
    pub global_changes: Vec<GlobalChange>,
    /// Export changes.
    pub export_changes: Vec<ExportChange>,
    /// Import changes.
    pub import_changes: Vec<ImportChange>,
    /// Warnings about potential issues.
    pub warnings: Vec<String>,
    /// Blocking issues that prevent migration.
    pub blockers: Vec<String>,
}

impl CompatibilityReport {
    /// Check if the modules are compatible for migration.
    pub fn is_compatible(&self) -> bool {
        self.compatible && self.blockers.is_empty()
    }

    /// Generate a migration plan from this report.
    pub fn migration_plan(&self) -> MigrationPlan {
        let mut steps = Vec::new();

        // Memory migration steps
        for change in &self.memory_changes {
            match change {
                MemoryChange::SizeIncrease { .. } => {
                    steps.push(MigrationStep::ExtendMemory);
                }
                MemoryChange::SizeDecrease { .. } => {
                    steps.push(MigrationStep::TruncateMemory);
                }
                MemoryChange::LayoutChange { description } => {
                    steps.push(MigrationStep::RemapMemory { description: description.clone() });
                }
            }
        }

        // Global remapping
        for change in &self.global_changes {
            match change {
                GlobalChange::TypeChanged { index, .. } => {
                    steps.push(MigrationStep::ConvertGlobal { index: *index });
                }
                GlobalChange::Removed { index } => {
                    steps.push(MigrationStep::RemoveGlobal { index: *index });
                }
                GlobalChange::Added { index, default_value } => {
                    steps.push(MigrationStep::AddGlobal {
                        index: *index,
                        default: default_value.clone(),
                    });
                }
                _ => {}
            }
        }

        steps.push(MigrationStep::VerifyIntegrity);

        let step_count = steps.len();
        MigrationPlan {
            steps,
            estimated_duration: Duration::from_millis(step_count as u64 * 10),
            requires_pause: self.level == CompatibilityLevel::Breaking,
        }
    }

    /// Get a summary string.
    pub fn summary(&self) -> String {
        format!(
            "Compatibility: {} ({}) | Memory changes: {} | Global changes: {} | Warnings: {} | Blockers: {}",
            if self.compatible { "YES" } else { "NO" },
            self.level,
            self.memory_changes.len(),
            self.global_changes.len(),
            self.warnings.len(),
            self.blockers.len(),
        )
    }
}

/// Compatibility level between module versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityLevel {
    /// Fully compatible, seamless migration.
    FullyCompatible,
    /// Compatible with minor transformations.
    MinorChanges,
    /// Requires significant state migration.
    MajorChanges,
    /// Breaking changes, migration may lose state.
    Breaking,
}

impl std::fmt::Display for CompatibilityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullyCompatible => write!(f, "fully-compatible"),
            Self::MinorChanges => write!(f, "minor-changes"),
            Self::MajorChanges => write!(f, "major-changes"),
            Self::Breaking => write!(f, "breaking"),
        }
    }
}

/// A detected change in memory layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryChange {
    /// Memory size increased.
    SizeIncrease {
        /// Old minimum pages.
        old_pages: u32,
        /// New minimum pages.
        new_pages: u32,
    },
    /// Memory size decreased.
    SizeDecrease {
        /// Old minimum pages.
        old_pages: u32,
        /// New minimum pages.
        new_pages: u32,
    },
    /// Memory layout changed.
    LayoutChange {
        /// Description of the change.
        description: String,
    },
}

/// A detected change in global variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GlobalChange {
    /// A global's type changed.
    TypeChanged {
        /// Global index.
        index: u32,
        /// Old type.
        old_type: ValueType,
        /// New type.
        new_type: ValueType,
    },
    /// A global was removed.
    Removed {
        /// Global index.
        index: u32,
    },
    /// A global was added.
    Added {
        /// Global index.
        index: u32,
        /// Default value.
        default_value: Vec<u8>,
    },
    /// A global's mutability changed.
    MutabilityChanged {
        /// Global index.
        index: u32,
    },
}

/// A detected change in module exports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportChange {
    /// An export was added.
    Added {
        /// Export name.
        name: String,
    },
    /// An export was removed.
    Removed {
        /// Export name.
        name: String,
    },
    /// An export's type changed.
    TypeChanged {
        /// Export name.
        name: String,
        /// Description of the change.
        description: String,
    },
}

/// A detected change in module imports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImportChange {
    /// An import was added.
    Added {
        /// Module name.
        module: String,
        /// Field name.
        name: String,
    },
    /// An import was removed.
    Removed {
        /// Module name.
        module: String,
        /// Field name.
        name: String,
    },
}

/// A step in the migration plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationStep {
    /// Extend memory to accommodate new size.
    ExtendMemory,
    /// Truncate memory (may lose data).
    TruncateMemory,
    /// Remap memory layout.
    RemapMemory {
        /// Description of the remapping.
        description: String,
    },
    /// Convert a global variable to a new type.
    ConvertGlobal {
        /// Global index.
        index: u32,
    },
    /// Remove a global variable.
    RemoveGlobal {
        /// Global index.
        index: u32,
    },
    /// Add a global variable with a default value.
    AddGlobal {
        /// Global index.
        index: u32,
        /// Default value bytes.
        default: Vec<u8>,
    },
    /// Verify state integrity after migration.
    VerifyIntegrity,
}

/// A plan for migrating state between module versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    /// Ordered migration steps.
    pub steps: Vec<MigrationStep>,
    /// Estimated duration.
    pub estimated_duration: Duration,
    /// Whether execution must be paused during migration.
    pub requires_pause: bool,
}

impl MigrationPlan {
    /// Get the number of steps.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Check if this plan has any destructive steps.
    pub fn has_destructive_steps(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(s, MigrationStep::TruncateMemory | MigrationStep::RemoveGlobal { .. })
        })
    }
}

/// Analyzer for compatibility between module versions.
pub struct CompatibilityAnalyzer {
    /// Whether to treat export removal as a blocker.
    pub strict_exports: bool,
    /// Whether to treat import changes as a blocker.
    pub strict_imports: bool,
}

impl CompatibilityAnalyzer {
    /// Create a new analyzer with default settings.
    pub fn new() -> Self {
        Self { strict_exports: true, strict_imports: true }
    }

    /// Analyze compatibility between two WASM module binaries.
    ///
    /// Performs structural comparison of WASM sections to detect memory,
    /// global, export, and import changes between versions.
    pub fn check(&self, old_module: &[u8], new_module: &[u8]) -> CompatibilityReport {
        let mut warnings = Vec::new();
        let mut blockers = Vec::new();
        let mut memory_changes = Vec::new();
        let mut global_changes = Vec::new();
        let mut export_changes = Vec::new();
        let mut import_changes = Vec::new();

        // Basic validation
        if old_module.len() < 8 || new_module.len() < 8 {
            blockers.push("One or both modules are too small to be valid WASM".to_string());
            return CompatibilityReport {
                compatible: false,
                level: CompatibilityLevel::Breaking,
                memory_changes,
                global_changes,
                export_changes,
                import_changes,
                warnings,
                blockers,
            };
        }

        if &old_module[0..4] != b"\0asm" || &new_module[0..4] != b"\0asm" {
            blockers.push("Invalid WASM magic number".to_string());
            return CompatibilityReport {
                compatible: false,
                level: CompatibilityLevel::Breaking,
                memory_changes,
                global_changes,
                export_changes,
                import_changes,
                warnings,
                blockers,
            };
        }

        // Parse WASM sections from both modules
        let old_sections = parse_wasm_sections(old_module);
        let new_sections = parse_wasm_sections(new_module);

        // Compare memory sections (section id 5)
        let old_mem_count = old_sections.get(&5).map(|s| s.len()).unwrap_or(0);
        let new_mem_count = new_sections.get(&5).map(|s| s.len()).unwrap_or(0);
        if old_mem_count != new_mem_count {
            memory_changes.push(MemoryChange::LayoutChange {
                description: format!(
                    "Memory section count changed: {} → {}",
                    old_mem_count, new_mem_count
                ),
            });
        }

        // Compare global sections (section id 6)
        let old_global_data = old_sections.get(&6).cloned().unwrap_or_default();
        let new_global_data = new_sections.get(&6).cloned().unwrap_or_default();
        if old_global_data.len() != new_global_data.len() {
            if new_global_data.len() > old_global_data.len() {
                global_changes.push(GlobalChange::Added {
                    index: old_global_data.len() as u32,
                    default_value: vec![0; 8],
                });
            } else {
                global_changes.push(GlobalChange::Removed { index: new_global_data.len() as u32 });
            }
        } else if old_global_data != new_global_data {
            global_changes.push(GlobalChange::TypeChanged {
                index: 0,
                old_type: ValueType::I64,
                new_type: ValueType::I64,
            });
            warnings.push("Global section content changed".to_string());
        }

        // Compare export sections (section id 7)
        let old_exports = extract_export_names(old_sections.get(&7));
        let new_exports = extract_export_names(new_sections.get(&7));

        for name in &old_exports {
            if !new_exports.contains(name) {
                export_changes.push(ExportChange::Removed { name: name.clone() });
                if self.strict_exports {
                    blockers.push(format!("Export '{}' was removed", name));
                }
            }
        }
        for name in &new_exports {
            if !old_exports.contains(name) {
                export_changes.push(ExportChange::Added { name: name.clone() });
            }
        }

        // Compare import sections (section id 2)
        let old_imports = extract_import_names(old_sections.get(&2));
        let new_imports = extract_import_names(new_sections.get(&2));

        for (module, name) in &old_imports {
            if !new_imports.contains(&(module.clone(), name.clone())) {
                import_changes
                    .push(ImportChange::Removed { module: module.clone(), name: name.clone() });
            }
        }
        for (module, name) in &new_imports {
            if !old_imports.contains(&(module.clone(), name.clone())) {
                import_changes
                    .push(ImportChange::Added { module: module.clone(), name: name.clone() });
                if self.strict_imports {
                    blockers
                        .push(format!("New import '{}::{}' requires host support", module, name));
                }
            }
        }

        // Size change analysis
        let size_ratio = new_module.len() as f64 / old_module.len() as f64;
        if size_ratio > 2.0 {
            warnings.push(format!(
                "New module is {:.1}x larger ({} → {} bytes)",
                size_ratio,
                old_module.len(),
                new_module.len()
            ));
        }
        if size_ratio < 0.5 {
            warnings.push(format!(
                "New module is {:.1}x smaller ({} → {} bytes)",
                size_ratio,
                old_module.len(),
                new_module.len()
            ));
        }

        // Determine compatibility level
        let total_changes = memory_changes.len()
            + global_changes.len()
            + export_changes.len()
            + import_changes.len();

        let level = if !blockers.is_empty() {
            CompatibilityLevel::Breaking
        } else if total_changes == 0 && warnings.is_empty() {
            CompatibilityLevel::FullyCompatible
        } else if total_changes <= 2 {
            CompatibilityLevel::MinorChanges
        } else {
            CompatibilityLevel::MajorChanges
        };

        CompatibilityReport {
            compatible: blockers.is_empty(),
            level,
            memory_changes,
            global_changes,
            export_changes,
            import_changes,
            warnings,
            blockers,
        }
    }
}

impl Default for CompatibilityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse WASM binary into section-id → section-data map.
/// WASM binary format: magic(4) + version(4) + sections*
/// Each section: id(1 byte) + size(LEB128) + data(size bytes)
fn parse_wasm_sections(bytes: &[u8]) -> std::collections::HashMap<u8, Vec<u8>> {
    let mut sections = std::collections::HashMap::new();
    if bytes.len() < 8 {
        return sections;
    }
    let mut pos = 8; // skip magic + version
    while pos < bytes.len() {
        let section_id = bytes[pos];
        pos += 1;
        // Read LEB128 size
        let (size, bytes_read) = read_leb128(&bytes[pos..]);
        pos += bytes_read;
        if pos + size > bytes.len() {
            break;
        }
        sections.insert(section_id, bytes[pos..pos + size].to_vec());
        pos += size;
    }
    sections
}

/// Read an unsigned LEB128 integer, returning (value, bytes_consumed).
fn read_leb128(bytes: &[u8]) -> (usize, usize) {
    let mut result: usize = 0;
    let mut shift = 0;
    let mut pos = 0;
    loop {
        if pos >= bytes.len() {
            break;
        }
        let byte = bytes[pos];
        result |= ((byte & 0x7F) as usize) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    (result, pos)
}

/// Extract export names from a WASM export section.
fn extract_export_names(section: Option<&Vec<u8>>) -> Vec<String> {
    let data = match section {
        Some(d) if !d.is_empty() => d,
        _ => return Vec::new(),
    };
    let mut names = Vec::new();
    let (count, mut pos) = read_leb128(data);
    for _ in 0..count {
        if pos >= data.len() {
            break;
        }
        let (name_len, n) = read_leb128(&data[pos..]);
        pos += n;
        if pos + name_len > data.len() {
            break;
        }
        if let Ok(name) = std::str::from_utf8(&data[pos..pos + name_len]) {
            names.push(name.to_string());
        }
        pos += name_len;
        // Skip export kind (1 byte) + index (LEB128)
        if pos < data.len() {
            pos += 1;
            let (_, n) = read_leb128(&data[pos..]);
            pos += n;
        }
    }
    names
}

/// Extract import (module, name) pairs from a WASM import section.
fn extract_import_names(section: Option<&Vec<u8>>) -> Vec<(String, String)> {
    let data = match section {
        Some(d) if !d.is_empty() => d,
        _ => return Vec::new(),
    };
    let mut imports = Vec::new();
    let (count, mut pos) = read_leb128(data);
    for _ in 0..count {
        if pos >= data.len() {
            break;
        }
        // Read module name
        let (mod_len, n) = read_leb128(&data[pos..]);
        pos += n;
        if pos + mod_len > data.len() {
            break;
        }
        let module = std::str::from_utf8(&data[pos..pos + mod_len]).unwrap_or("").to_string();
        pos += mod_len;
        // Read field name
        if pos >= data.len() {
            break;
        }
        let (name_len, n) = read_leb128(&data[pos..]);
        pos += n;
        if pos + name_len > data.len() {
            break;
        }
        let name = std::str::from_utf8(&data[pos..pos + name_len]).unwrap_or("").to_string();
        pos += name_len;
        imports.push((module, name));
        // Skip import description (kind byte + type-specific data)
        if pos < data.len() {
            let kind = data[pos];
            pos += 1;
            let (_, n) = read_leb128(&data[pos..]);
            pos += n;
            // For table and memory types, skip additional limits
            if kind == 1 || kind == 2 {
                if pos < data.len() {
                    let (_, n) = read_leb128(&data[pos..]);
                    pos += n;
                    if pos < data.len() && data.get(pos.wrapping_sub(n)).copied().unwrap_or(0) == 1
                    {
                        let (_, n) = read_leb128(&data[pos..]);
                        pos += n;
                    }
                }
            }
        }
    }
    imports
}

/// State migrator that applies a migration plan to captured state.
pub struct StateMigrator {
    plan: MigrationPlan,
}

impl StateMigrator {
    /// Create a new migrator with the given plan.
    pub fn new(plan: MigrationPlan) -> Self {
        Self { plan }
    }

    /// Migrate captured state according to the plan.
    pub fn migrate(&self, state: &CapturedState) -> Result<CapturedState, String> {
        let mut new_state = state.clone();

        for step in &self.plan.steps {
            match step {
                MigrationStep::ExtendMemory => {
                    // Extend to next page boundary (64KB pages)
                    let page_size = 65536;
                    let current_pages = (new_state.memory.len() + page_size - 1) / page_size;
                    let target_size = (current_pages + 1) * page_size;
                    new_state.memory.resize(target_size, 0);
                }
                MigrationStep::TruncateMemory => {
                    if new_state.memory.len() > 65536 {
                        let page_size = 65536;
                        let current_pages = new_state.memory.len() / page_size;
                        let target_size = (current_pages.saturating_sub(1)) * page_size;
                        new_state.memory.truncate(target_size.max(page_size));
                    }
                }
                MigrationStep::RemapMemory { description } => {
                    // Log that remapping occurred for audit trail
                    new_state
                        .custom
                        .insert("__remap_log".to_string(), description.as_bytes().to_vec());
                }
                MigrationStep::ConvertGlobal { index } => {
                    if let Some(global) = new_state.globals.iter_mut().find(|g| g.index == *index) {
                        // Type conversion: widen to 8 bytes (i32→i64 safe, truncate for narrowing)
                        match global.value.len() {
                            4 => {
                                // Widen: i32 → i64 (sign-extend)
                                let val = i32::from_le_bytes(
                                    global.value[..4].try_into().unwrap_or([0; 4]),
                                );
                                global.value = (val as i64).to_le_bytes().to_vec();
                                global.value_type = ValueType::I64;
                            }
                            8 => {
                                // Already 8 bytes — no conversion needed
                            }
                            _ => {
                                // Pad or truncate to 8 bytes
                                global.value.resize(8, 0);
                            }
                        }
                    }
                }
                MigrationStep::RemoveGlobal { index } => {
                    new_state.globals.retain(|g| g.index != *index);
                }
                MigrationStep::AddGlobal { index, default } => {
                    new_state.globals.push(GlobalValue {
                        index: *index,
                        value_type: if default.len() == 4 {
                            ValueType::I32
                        } else {
                            ValueType::I64
                        },
                        value: default.clone(),
                    });
                }
                MigrationStep::VerifyIntegrity => {
                    // Verify memory is page-aligned
                    if !new_state.memory.is_empty() && new_state.memory.len() % 65536 != 0 {
                        return Err(format!(
                            "Memory size {} is not page-aligned after migration",
                            new_state.memory.len()
                        ));
                    }
                    // Verify no duplicate global indices
                    let mut seen_indices = std::collections::HashSet::new();
                    for g in &new_state.globals {
                        if !seen_indices.insert(g.index) {
                            return Err(format!(
                                "Duplicate global index {} after migration",
                                g.index
                            ));
                        }
                    }
                }
            }
        }

        Ok(new_state)
    }

    /// Get the migration plan.
    pub fn plan(&self) -> &MigrationPlan {
        &self.plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    // Build a minimal WASM with an export section containing given names.
    // Section 7 (export): count + entries(name_len + name + kind + index)
    fn wasm_with_exports(names: &[&str]) -> Vec<u8> {
        let mut section_data = Vec::new();
        section_data.push(names.len() as u8); // count
        for name in names {
            section_data.push(name.len() as u8);
            section_data.extend_from_slice(name.as_bytes());
            section_data.push(0x00); // kind: function
            section_data.push(0x00); // index: 0
        }
        let mut wasm = VALID_WASM.to_vec();
        wasm.push(7); // section id: export
        wasm.push(section_data.len() as u8); // section size
        wasm.extend_from_slice(&section_data);
        wasm
    }

    // Build a minimal WASM with an import section
    fn wasm_with_imports(imports: &[(&str, &str)]) -> Vec<u8> {
        let mut section_data = Vec::new();
        section_data.push(imports.len() as u8);
        for (module, name) in imports {
            section_data.push(module.len() as u8);
            section_data.extend_from_slice(module.as_bytes());
            section_data.push(name.len() as u8);
            section_data.extend_from_slice(name.as_bytes());
            section_data.push(0x00); // kind: function
            section_data.push(0x00); // type index
        }
        let mut wasm = VALID_WASM.to_vec();
        wasm.push(2); // section id: import
        wasm.push(section_data.len() as u8);
        wasm.extend_from_slice(&section_data);
        wasm
    }

    #[test]
    fn test_compatibility_check_identical() {
        let analyzer = CompatibilityAnalyzer::new();
        let report = analyzer.check(VALID_WASM, VALID_WASM);
        assert!(report.is_compatible());
        assert_eq!(report.level, CompatibilityLevel::FullyCompatible);
        assert!(report.blockers.is_empty());
    }

    #[test]
    fn test_compatibility_check_invalid() {
        let analyzer = CompatibilityAnalyzer::new();
        let report = analyzer.check(&[0, 1, 2, 3, 4, 5, 6, 7], VALID_WASM);
        assert!(!report.is_compatible());
        assert_eq!(report.level, CompatibilityLevel::Breaking);
    }

    #[test]
    fn test_compatibility_check_too_small() {
        let analyzer = CompatibilityAnalyzer::new();
        assert!(!analyzer.check(&[0, 1], VALID_WASM).is_compatible());
    }

    #[test]
    fn test_export_added_is_compatible() {
        let analyzer = CompatibilityAnalyzer::new();
        let old = wasm_with_exports(&["main"]);
        let new = wasm_with_exports(&["main", "helper"]);

        let report = analyzer.check(&old, &new);
        assert!(report.is_compatible());
        assert_eq!(report.export_changes.len(), 1);
        assert!(
            matches!(&report.export_changes[0], ExportChange::Added { name } if name == "helper")
        );
    }

    #[test]
    fn test_export_removed_blocks_strict() {
        let analyzer = CompatibilityAnalyzer::new();
        let old = wasm_with_exports(&["main", "helper"]);
        let new = wasm_with_exports(&["main"]);

        let report = analyzer.check(&old, &new);
        assert!(!report.is_compatible());
        assert!(report.blockers.iter().any(|b| b.contains("helper")));
    }

    #[test]
    fn test_export_removed_allowed_non_strict() {
        let mut analyzer = CompatibilityAnalyzer::new();
        analyzer.strict_exports = false;
        let old = wasm_with_exports(&["main", "helper"]);
        let new = wasm_with_exports(&["main"]);

        let report = analyzer.check(&old, &new);
        assert!(report.is_compatible());
        assert!(!report.export_changes.is_empty());
    }

    #[test]
    fn test_import_added_blocks_strict() {
        let analyzer = CompatibilityAnalyzer::new();
        let old = wasm_with_imports(&[("env", "memory")]);
        let new = wasm_with_imports(&[("env", "memory"), ("wasi", "fd_write")]);

        let report = analyzer.check(&old, &new);
        assert!(!report.is_compatible());
        assert!(report.blockers.iter().any(|b| b.contains("fd_write")));
    }

    #[test]
    fn test_import_added_allowed_non_strict() {
        let mut analyzer = CompatibilityAnalyzer::new();
        analyzer.strict_imports = false;
        let old = wasm_with_imports(&[("env", "memory")]);
        let new = wasm_with_imports(&[("env", "memory"), ("wasi", "fd_write")]);

        let report = analyzer.check(&old, &new);
        assert!(report.is_compatible());
    }

    #[test]
    fn test_size_warning() {
        let analyzer = CompatibilityAnalyzer::new();
        let mut large = VALID_WASM.to_vec();
        large.extend(vec![0u8; 1000]);
        let report = analyzer.check(VALID_WASM, &large);
        assert!(report.is_compatible());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_migration_plan_generation() {
        let report = CompatibilityReport {
            compatible: true,
            level: CompatibilityLevel::MinorChanges,
            memory_changes: vec![MemoryChange::SizeIncrease { old_pages: 1, new_pages: 2 }],
            global_changes: vec![GlobalChange::Added { index: 5, default_value: vec![0, 0, 0, 0] }],
            export_changes: Vec::new(),
            import_changes: Vec::new(),
            warnings: Vec::new(),
            blockers: Vec::new(),
        };
        let plan = report.migration_plan();
        assert!(plan.step_count() >= 3); // ExtendMemory + AddGlobal + VerifyIntegrity
    }

    #[test]
    fn test_migration_plan_destructive() {
        let plan = MigrationPlan {
            steps: vec![MigrationStep::TruncateMemory, MigrationStep::VerifyIntegrity],
            estimated_duration: Duration::from_millis(20),
            requires_pause: false,
        };
        assert!(plan.has_destructive_steps());
    }

    #[test]
    fn test_state_migrator_add_global() {
        let state = CapturedState::empty();
        let plan = MigrationPlan {
            steps: vec![
                MigrationStep::AddGlobal { index: 0, default: vec![42, 0, 0, 0] },
                MigrationStep::VerifyIntegrity,
            ],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let migrator = StateMigrator::new(plan);
        let new_state = migrator.migrate(&state).unwrap();
        assert_eq!(new_state.globals.len(), 1);
        assert_eq!(new_state.globals[0].value_type, ValueType::I32);
    }

    #[test]
    fn test_state_migrator_remove_global() {
        let mut state = CapturedState::empty();
        state.globals.push(GlobalValue {
            index: 0,
            value_type: ValueType::I32,
            value: vec![1, 0, 0, 0],
        });
        state.globals.push(GlobalValue {
            index: 1,
            value_type: ValueType::I32,
            value: vec![2, 0, 0, 0],
        });

        let plan = MigrationPlan {
            steps: vec![MigrationStep::RemoveGlobal { index: 0 }, MigrationStep::VerifyIntegrity],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let new_state = StateMigrator::new(plan).migrate(&state).unwrap();
        assert_eq!(new_state.globals.len(), 1);
        assert_eq!(new_state.globals[0].index, 1);
    }

    #[test]
    fn test_state_migrator_convert_global_i32_to_i64() {
        let mut state = CapturedState::empty();
        state.globals.push(GlobalValue {
            index: 0,
            value_type: ValueType::I32,
            value: 42i32.to_le_bytes().to_vec(),
        });

        let plan = MigrationPlan {
            steps: vec![MigrationStep::ConvertGlobal { index: 0 }, MigrationStep::VerifyIntegrity],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let new_state = StateMigrator::new(plan).migrate(&state).unwrap();
        assert_eq!(new_state.globals[0].value_type, ValueType::I64);
        assert_eq!(new_state.globals[0].value.len(), 8);
        let val = i64::from_le_bytes(new_state.globals[0].value[..8].try_into().unwrap());
        assert_eq!(val, 42);
    }

    #[test]
    fn test_state_migrator_extend_memory() {
        let mut state = CapturedState::empty();
        state.memory = vec![0u8; 65536]; // 1 page

        let plan = MigrationPlan {
            steps: vec![MigrationStep::ExtendMemory, MigrationStep::VerifyIntegrity],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let new_state = StateMigrator::new(plan).migrate(&state).unwrap();
        assert_eq!(new_state.memory.len(), 65536 * 2);
    }

    #[test]
    fn test_state_migrator_truncate_memory() {
        let mut state = CapturedState::empty();
        state.memory = vec![0u8; 65536 * 3];

        let plan = MigrationPlan {
            steps: vec![MigrationStep::TruncateMemory, MigrationStep::VerifyIntegrity],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let new_state = StateMigrator::new(plan).migrate(&state).unwrap();
        assert_eq!(new_state.memory.len(), 65536 * 2);
    }

    #[test]
    fn test_state_migrator_verify_duplicate_globals_fails() {
        let mut state = CapturedState::empty();
        state.globals.push(GlobalValue { index: 0, value_type: ValueType::I32, value: vec![0; 4] });

        let plan = MigrationPlan {
            steps: vec![
                MigrationStep::AddGlobal { index: 0, default: vec![0; 4] }, // duplicate!
                MigrationStep::VerifyIntegrity,
            ],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let result = StateMigrator::new(plan).migrate(&state);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate global index"));
    }

    #[test]
    fn test_state_migrator_remap_memory_logs() {
        let state = CapturedState::empty();
        let plan = MigrationPlan {
            steps: vec![
                MigrationStep::RemapMemory { description: "moved heap base".to_string() },
                MigrationStep::VerifyIntegrity,
            ],
            estimated_duration: Duration::from_millis(10),
            requires_pause: false,
        };
        let new_state = StateMigrator::new(plan).migrate(&state).unwrap();
        assert!(new_state.custom.contains_key("__remap_log"));
    }

    #[test]
    fn test_parse_wasm_sections() {
        let wasm = wasm_with_exports(&["main", "alloc"]);
        let sections = parse_wasm_sections(&wasm);
        assert!(sections.contains_key(&7)); // export section
    }

    #[test]
    fn test_report_summary() {
        let report = CompatibilityReport {
            compatible: true,
            level: CompatibilityLevel::FullyCompatible,
            memory_changes: Vec::new(),
            global_changes: Vec::new(),
            export_changes: Vec::new(),
            import_changes: Vec::new(),
            warnings: Vec::new(),
            blockers: Vec::new(),
        };
        assert!(report.summary().contains("YES"));
        assert!(report.summary().contains("fully-compatible"));
    }
}
