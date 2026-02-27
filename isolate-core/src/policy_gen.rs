//! AI-powered policy generator and WASM module analyzer.
//!
//! Analyzes WASM modules to suggest minimal capability sets, detect potential
//! security concerns, and generate human-readable policy explanations.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::policy_gen::{ModuleAnalyzer, AnalysisReport};
//!
//! let wasm_bytes = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
//! let analyzer = ModuleAnalyzer::new();
//! let report = analyzer.analyze(wasm_bytes);
//!
//! for cap in &report.suggested_capabilities {
//!     println!("Suggested: {} - {}", cap.capability, cap.reason);
//! }
//! ```

#![allow(missing_docs)]
use crate::capability::Capability;
use serde::{Deserialize, Serialize};

/// Analyzes WASM modules and suggests security policies.
pub struct ModuleAnalyzer {
    /// Known import patterns and their capability implications.
    patterns: Vec<ImportPattern>,
}

/// A pattern matching WASM imports to required capabilities.
struct ImportPattern {
    /// Module name pattern (exact or prefix match).
    module: String,
    /// Function name pattern (exact, prefix, or "*" for any).
    function: String,
    /// Capability needed for this import.
    capability: Capability,
    /// Human-readable reason.
    reason: &'static str,
    /// Risk level of this import.
    risk: RiskLevel,
}

/// Risk level for a detected import pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Benign operation, no security concern.
    Low,
    /// May need attention depending on context.
    Medium,
    /// Requires careful review.
    High,
    /// Potentially dangerous operation.
    Critical,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// A suggested capability with justification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySuggestion {
    /// The capability to grant.
    pub capability: String,
    /// Why this capability is needed.
    pub reason: String,
    /// Risk level of granting this capability.
    pub risk: RiskLevel,
    /// Confidence in this suggestion (0.0 - 1.0).
    pub confidence: f64,
}

/// Security concern detected in the module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConcern {
    /// Description of the concern.
    pub description: String,
    /// Risk level.
    pub risk: RiskLevel,
    /// Recommended mitigation.
    pub mitigation: String,
}

/// Complete analysis report for a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    /// Module size in bytes.
    pub module_size: usize,
    /// Detected imports.
    pub imports: Vec<DetectedImport>,
    /// Suggested capabilities.
    pub suggested_capabilities: Vec<CapabilitySuggestion>,
    /// Security concerns.
    pub security_concerns: Vec<SecurityConcern>,
    /// Overall risk assessment.
    pub overall_risk: RiskLevel,
    /// Human-readable summary.
    pub summary: String,
}

/// A detected import from the WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedImport {
    /// Import module name.
    pub module: String,
    /// Import function name.
    pub name: String,
    /// Whether this import is WASI-standard.
    pub is_wasi: bool,
}

impl ModuleAnalyzer {
    /// Create a new module analyzer with built-in patterns.
    pub fn new() -> Self {
        Self {
            patterns: vec![
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "fd_write".to_string(),
                    capability: Capability::stdout(),
                    reason: "Module writes to file descriptors (stdout/stderr)",
                    risk: RiskLevel::Low,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "fd_read".to_string(),
                    capability: Capability::stdin(),
                    reason: "Module reads from file descriptors (stdin)",
                    risk: RiskLevel::Low,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "path_open".to_string(),
                    capability: Capability::filesystem_read("/"),
                    reason: "Module opens files on the filesystem",
                    risk: RiskLevel::High,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "clock_time_get".to_string(),
                    capability: Capability::system_clock(),
                    reason: "Module accesses system clock",
                    risk: RiskLevel::Low,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "random_get".to_string(),
                    capability: Capability::secure_random(),
                    reason: "Module uses random number generation",
                    risk: RiskLevel::Low,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "environ_get".to_string(),
                    capability: Capability::env_all(),
                    reason: "Module reads environment variables",
                    risk: RiskLevel::Medium,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "args_get".to_string(),
                    capability: Capability::args(),
                    reason: "Module reads command-line arguments",
                    risk: RiskLevel::Low,
                },
                ImportPattern {
                    module: "wasi_snapshot_preview1".to_string(),
                    function: "sock_accept".to_string(),
                    capability: Capability::tcp_listen(0),
                    reason: "Module accepts network connections",
                    risk: RiskLevel::Critical,
                },
            ],
        }
    }

    /// Analyze a WASM module and generate a policy report.
    pub fn analyze(&self, wasm_bytes: &[u8]) -> AnalysisReport {
        let imports = self.extract_imports(wasm_bytes);
        let mut suggestions = Vec::new();
        let mut concerns = Vec::new();
        let mut max_risk = RiskLevel::Low;

        // Match imports against known patterns
        for import in &imports {
            for pattern in &self.patterns {
                if import.module == pattern.module && import.name == pattern.function {
                    suggestions.push(CapabilitySuggestion {
                        capability: pattern.capability.description(),
                        reason: pattern.reason.to_string(),
                        risk: pattern.risk,
                        confidence: 0.95,
                    });

                    if pattern.risk > max_risk {
                        max_risk = pattern.risk;
                    }

                    if pattern.risk >= RiskLevel::High {
                        concerns.push(SecurityConcern {
                            description: format!(
                                "Import '{}::{}' requires elevated permissions",
                                import.module, import.name
                            ),
                            risk: pattern.risk,
                            mitigation: format!(
                                "Restrict the '{}' capability to specific paths/hosts",
                                pattern.capability.description()
                            ),
                        });
                    }
                }
            }

            // Flag unknown (non-WASI) imports
            if !import.is_wasi {
                concerns.push(SecurityConcern {
                    description: format!(
                        "Non-standard import '{}::{}' detected",
                        import.module, import.name
                    ),
                    risk: RiskLevel::Medium,
                    mitigation: "Verify this import is from a trusted host function provider"
                        .to_string(),
                });
                if max_risk < RiskLevel::Medium {
                    max_risk = RiskLevel::Medium;
                }
            }
        }

        // Deduplicate suggestions by capability
        suggestions.sort_by(|a, b| a.capability.cmp(&b.capability));
        suggestions.dedup_by(|a, b| a.capability == b.capability);

        let summary = self.generate_summary(&imports, &suggestions, &concerns, max_risk);

        AnalysisReport {
            module_size: wasm_bytes.len(),
            imports,
            suggested_capabilities: suggestions,
            security_concerns: concerns,
            overall_risk: max_risk,
            summary,
        }
    }

    /// Extract imports from a WASM binary (lightweight parser).
    fn extract_imports(&self, wasm_bytes: &[u8]) -> Vec<DetectedImport> {
        let mut imports = Vec::new();

        // Simple WASM import section parser
        // WASM binary format: magic + version + sections
        if wasm_bytes.len() < 8 {
            return imports;
        }

        let mut pos = 8; // Skip magic + version
        while pos < wasm_bytes.len() {
            if pos >= wasm_bytes.len() {
                break;
            }
            let section_id = wasm_bytes[pos];
            pos += 1;

            // Read section size (LEB128)
            let (size, bytes_read) = read_leb128(&wasm_bytes[pos..]);
            pos += bytes_read;

            if section_id == 2 {
                // Import section
                let section_end = pos + size as usize;
                // Read import count
                let (count, bytes_read) = read_leb128(&wasm_bytes[pos..]);
                pos += bytes_read;

                for _ in 0..count {
                    if pos >= section_end {
                        break;
                    }

                    // Read module name
                    let (mod_len, bytes_read) = read_leb128(&wasm_bytes[pos..]);
                    pos += bytes_read;
                    let module = String::from_utf8_lossy(&wasm_bytes[pos..pos + mod_len as usize])
                        .to_string();
                    pos += mod_len as usize;

                    // Read function name
                    let (name_len, bytes_read) = read_leb128(&wasm_bytes[pos..]);
                    pos += bytes_read;
                    let name = String::from_utf8_lossy(&wasm_bytes[pos..pos + name_len as usize])
                        .to_string();
                    pos += name_len as usize;

                    // Skip import descriptor
                    if pos < section_end {
                        let desc_type = wasm_bytes[pos];
                        pos += 1;
                        match desc_type {
                            0x00 => {
                                // Function: skip type index
                                let (_, br) = read_leb128(&wasm_bytes[pos..]);
                                pos += br;
                            }
                            0x01 => pos += 3, // Table: type + limits
                            0x02 => {
                                // Memory: skip limits
                                let flags = wasm_bytes.get(pos).copied().unwrap_or(0);
                                pos += 1;
                                let (_, br) = read_leb128(&wasm_bytes[pos..]);
                                pos += br;
                                if flags & 1 != 0 {
                                    let (_, br) = read_leb128(&wasm_bytes[pos..]);
                                    pos += br;
                                }
                            }
                            0x03 => pos += 2, // Global: type + mutability
                            _ => {}
                        }
                    }

                    let is_wasi =
                        module.starts_with("wasi_snapshot_preview1") || module.starts_with("wasi:");

                    imports.push(DetectedImport { module, name, is_wasi });
                }
                break; // Only need import section
            } else {
                // Skip other sections
                pos += size as usize;
            }
        }

        imports
    }

    fn generate_summary(
        &self,
        imports: &[DetectedImport],
        suggestions: &[CapabilitySuggestion],
        concerns: &[SecurityConcern],
        risk: RiskLevel,
    ) -> String {
        let mut parts = Vec::new();

        parts.push(format!(
            "Module has {} import(s), {} capability suggestion(s), risk level: {}.",
            imports.len(),
            suggestions.len(),
            risk
        ));

        if concerns.is_empty() {
            parts.push("No security concerns detected.".to_string());
        } else {
            parts.push(format!("{} security concern(s) found.", concerns.len()));
        }

        if suggestions.is_empty() {
            parts.push("Module requires no special capabilities.".to_string());
        }

        parts.join(" ")
    }
}

impl Default for ModuleAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Read a LEB128-encoded unsigned integer.
fn read_leb128(bytes: &[u8]) -> (u64, usize) {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut pos = 0;

    loop {
        if pos >= bytes.len() {
            break;
        }
        let byte = bytes[pos];
        result |= ((byte & 0x7F) as u64) << shift;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_minimal_module() {
        let wasm = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let analyzer = ModuleAnalyzer::new();
        let report = analyzer.analyze(wasm);

        assert_eq!(report.module_size, 8);
        assert!(report.imports.is_empty());
        assert!(report.suggested_capabilities.is_empty());
        assert_eq!(report.overall_risk, RiskLevel::Low);
    }

    #[test]
    fn test_analyzer_with_wasi_imports() {
        // Build a WASM module with fd_write import
        let wasm = build_wasm_with_import("wasi_snapshot_preview1", "fd_write", 0);
        let analyzer = ModuleAnalyzer::new();
        let report = analyzer.analyze(&wasm);

        assert!(!report.imports.is_empty());
        assert_eq!(report.imports[0].module, "wasi_snapshot_preview1");
        assert_eq!(report.imports[0].name, "fd_write");
        assert!(report.imports[0].is_wasi);
        assert!(!report.suggested_capabilities.is_empty());
    }

    #[test]
    fn test_analyzer_non_wasi_import() {
        let wasm = build_wasm_with_import("custom_host", "do_something", 0);
        let analyzer = ModuleAnalyzer::new();
        let report = analyzer.analyze(&wasm);

        assert!(!report.imports.is_empty());
        assert!(!report.imports[0].is_wasi);
        // Should flag as security concern
        assert!(!report.security_concerns.is_empty());
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_analyzer_filesystem_import() {
        let wasm = build_wasm_with_import("wasi_snapshot_preview1", "path_open", 0);
        let analyzer = ModuleAnalyzer::new();
        let report = analyzer.analyze(&wasm);

        // Should suggest filesystem capability with high risk
        let fs_suggestion =
            report.suggested_capabilities.iter().find(|s| s.capability.starts_with("fs:"));
        assert!(fs_suggestion.is_some());
        assert_eq!(fs_suggestion.unwrap().risk, RiskLevel::High);
    }

    #[test]
    fn test_leb128_encoding() {
        assert_eq!(read_leb128(&[0x00]), (0, 1));
        assert_eq!(read_leb128(&[0x01]), (1, 1));
        assert_eq!(read_leb128(&[0x7F]), (127, 1));
        assert_eq!(read_leb128(&[0x80, 0x01]), (128, 2));
        assert_eq!(read_leb128(&[0xE5, 0x8E, 0x26]), (624485, 3));
    }

    #[test]
    fn test_report_summary() {
        let wasm = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let analyzer = ModuleAnalyzer::new();
        let report = analyzer.analyze(wasm);
        assert!(report.summary.contains("risk level: low"));
    }

    /// Helper to build a minimal WASM module with a single import.
    fn build_wasm_with_import(module: &str, name: &str, type_idx: u8) -> Vec<u8> {
        let mut wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]; // magic + version

        // Type section (section 1) - one function type: () -> ()
        let type_section = vec![
            0x01, // section id
            0x04, // section size
            0x01, // one type
            0x60, // func type
            0x00, // no params
            0x00, // no results
        ];
        wasm.extend_from_slice(&type_section);

        // Import section (section 2)
        let mut import_data = Vec::new();
        import_data.push(0x01); // one import

        // Module name
        import_data.push(module.len() as u8);
        import_data.extend_from_slice(module.as_bytes());

        // Function name
        import_data.push(name.len() as u8);
        import_data.extend_from_slice(name.as_bytes());

        // Import descriptor: function type
        import_data.push(0x00); // function
        import_data.push(type_idx);

        wasm.push(0x02); // section id
        wasm.push(import_data.len() as u8); // section size
        wasm.extend_from_slice(&import_data);

        wasm
    }
}
