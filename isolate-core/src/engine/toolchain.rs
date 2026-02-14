//! Language-specific WASM compilation toolchain support.
//!
//! This module provides the data model and orchestration for compiling source code
//! to WASM using language-specific toolchains. It handles toolchain discovery,
//! configuration, and compilation request/result types.
//!
//! **Note:** This module does NOT invoke external tools — it provides the framework
//! for discovering, configuring, and driving toolchain backends.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Supported source languages that can be compiled to WASM.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Go,
    Python,
    JavaScript,
    TypeScript,
    CSharp,
    AssemblyScript,
    Other(String),
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Rust => write!(f, "Rust"),
            Language::Go => write!(f, "Go"),
            Language::Python => write!(f, "Python"),
            Language::JavaScript => write!(f, "JavaScript"),
            Language::TypeScript => write!(f, "TypeScript"),
            Language::CSharp => write!(f, "C#"),
            Language::AssemblyScript => write!(f, "AssemblyScript"),
            Language::Other(name) => write!(f, "{}", name),
        }
    }
}

/// WASM compilation target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WasmTarget {
    /// WASI Preview 1 (the most widely supported target).
    WasiPreview1,
    /// WASI Preview 2 (Component Model).
    WasiPreview2,
    /// Core WASM module without WASI.
    CoreModule,
}

/// Compiler optimization level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptLevel {
    /// No optimization (fastest compile).
    None,
    /// Optimize for execution speed.
    Speed,
    /// Optimize for binary size.
    Size,
}

/// Configuration for a language-specific compilation toolchain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolchainConfig {
    /// The source language this toolchain compiles.
    pub language: Language,
    /// Path to the compiler binary (auto-detected if `None`).
    pub compiler_path: Option<PathBuf>,
    /// The WASM target to compile for.
    pub target: WasmTarget,
    /// Optimization level.
    pub optimization: OptLevel,
    /// Extra arguments passed to the compiler.
    pub extra_args: Vec<String>,
    /// Environment variables set during compilation.
    pub env_vars: HashMap<String, String>,
}

impl ToolchainConfig {
    /// Create a new toolchain config with sensible defaults for the given language.
    pub fn new(language: Language) -> Self {
        Self {
            language,
            compiler_path: None,
            target: WasmTarget::WasiPreview1,
            optimization: OptLevel::None,
            extra_args: Vec::new(),
            env_vars: HashMap::new(),
        }
    }
}

/// Registry of available compilation toolchains.
pub struct ToolchainRegistry {
    toolchains: HashMap<Language, ToolchainConfig>,
}

impl ToolchainRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            toolchains: HashMap::new(),
        }
    }

    /// Register a toolchain configuration for a language.
    pub fn register(&mut self, language: Language, config: ToolchainConfig) {
        self.toolchains.insert(language, config);
    }

    /// Look up the toolchain configuration for a language.
    pub fn get(&self, language: &Language) -> Option<&ToolchainConfig> {
        self.toolchains.get(language)
    }

    /// List all languages with registered toolchains.
    pub fn available_languages(&self) -> Vec<Language> {
        self.toolchains.keys().cloned().collect()
    }

    /// Detect compilers installed on the system by searching PATH.
    pub fn detect_installed() -> Vec<(Language, PathBuf)> {
        ToolchainDetector::detect_all()
    }

    /// Return a sensible default configuration for a language.
    pub fn default_config(language: &Language) -> ToolchainConfig {
        let mut config = ToolchainConfig::new(language.clone());

        match language {
            Language::Rust => {
                config.target = WasmTarget::WasiPreview1;
                config.optimization = OptLevel::Size;
                config.extra_args =
                    vec!["--target".into(), "wasm32-wasip1".into(), "--release".into()];
            }
            Language::Go => {
                config.target = WasmTarget::WasiPreview1;
                config.optimization = OptLevel::Size;
                config.extra_args = vec!["build".into(), "-target=wasi".into()];
            }
            Language::Python => {
                config.target = WasmTarget::WasiPreview2;
                config.optimization = OptLevel::None;
            }
            Language::JavaScript | Language::TypeScript => {
                config.target = WasmTarget::WasiPreview1;
                config.optimization = OptLevel::Speed;
            }
            Language::AssemblyScript => {
                config.target = WasmTarget::CoreModule;
                config.optimization = OptLevel::Size;
                config.extra_args = vec!["--optimize".into()];
            }
            Language::CSharp => {
                config.target = WasmTarget::WasiPreview1;
                config.optimization = OptLevel::Speed;
            }
            Language::Other(_) => {}
        }

        config
    }
}

impl Default for ToolchainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A request to compile source code to WASM.
#[derive(Clone, Debug)]
pub struct CompileRequest {
    /// Path to the source file or project directory.
    pub source_path: PathBuf,
    /// Desired output path for the compiled WASM module.
    pub output_path: PathBuf,
    /// Source language.
    pub language: Language,
    /// Toolchain configuration to use.
    pub config: ToolchainConfig,
}

/// The result of a successful WASM compilation.
#[derive(Clone, Debug)]
pub struct CompileResult {
    /// Path to the compiled WASM module.
    pub output_path: PathBuf,
    /// Size of the compiled WASM binary in bytes.
    pub output_size: u64,
    /// Wall-clock time spent compiling.
    pub compile_time: Duration,
    /// Compiler warnings emitted during compilation.
    pub warnings: Vec<String>,
    /// The WASM target that was compiled for.
    pub target: WasmTarget,
}

/// Detects installed WASM-capable compilers by searching PATH.
pub struct ToolchainDetector;

impl ToolchainDetector {
    /// Known compiler binary names for each language.
    fn known_compilers() -> Vec<(Language, &'static str)> {
        vec![
            (Language::Rust, "cargo"),
            (Language::Go, "tinygo"),
            (Language::Python, "componentize-py"),
            (Language::JavaScript, "javy"),
            (Language::TypeScript, "javy"),
            (Language::AssemblyScript, "asc"),
            (Language::CSharp, "dotnet"),
        ]
    }

    /// Search PATH for a binary by name.
    fn find_in_path(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).find_map(|dir| {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    Some(candidate)
                } else {
                    Option::None
                }
            })
        })
    }

    /// Detect all installed compilers.
    pub fn detect_all() -> Vec<(Language, PathBuf)> {
        Self::known_compilers()
            .into_iter()
            .filter_map(|(lang, bin)| Self::find_in_path(bin).map(|path| (lang, path)))
            .collect()
    }

    /// Detect a compiler for a specific language.
    pub fn detect(language: &Language) -> Option<PathBuf> {
        Self::known_compilers()
            .into_iter()
            .filter(|(lang, _)| lang == language)
            .find_map(|(_, bin)| Self::find_in_path(bin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Language enum tests ---

    #[test]
    fn test_language_equality() {
        assert_eq!(Language::Rust, Language::Rust);
        assert_ne!(Language::Rust, Language::Go);
        assert_eq!(
            Language::Other("Zig".into()),
            Language::Other("Zig".into())
        );
        assert_ne!(
            Language::Other("Zig".into()),
            Language::Other("C".into())
        );
    }

    #[test]
    fn test_language_display() {
        assert_eq!(Language::Rust.to_string(), "Rust");
        assert_eq!(Language::Go.to_string(), "Go");
        assert_eq!(Language::CSharp.to_string(), "C#");
        assert_eq!(Language::Other("Zig".into()).to_string(), "Zig");
    }

    #[test]
    fn test_language_hash() {
        let mut map = HashMap::new();
        map.insert(Language::Rust, "rust-toolchain");
        map.insert(Language::Go, "go-toolchain");
        assert_eq!(map.get(&Language::Rust), Some(&"rust-toolchain"));
        assert_eq!(map.get(&Language::Go), Some(&"go-toolchain"));
        assert_eq!(map.get(&Language::Python), None);
    }

    #[test]
    fn test_language_clone() {
        let lang = Language::Other("Zig".into());
        let cloned = lang.clone();
        assert_eq!(lang, cloned);
    }

    #[test]
    fn test_language_serde_roundtrip() {
        let languages = vec![
            Language::Rust,
            Language::Go,
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::CSharp,
            Language::AssemblyScript,
            Language::Other("Zig".into()),
        ];
        for lang in languages {
            let json = serde_json::to_string(&lang).unwrap();
            let deserialized: Language = serde_json::from_str(&json).unwrap();
            assert_eq!(lang, deserialized);
        }
    }

    // --- WasmTarget tests ---

    #[test]
    fn test_wasm_target_values() {
        let targets = vec![
            WasmTarget::WasiPreview1,
            WasmTarget::WasiPreview2,
            WasmTarget::CoreModule,
        ];
        assert_eq!(targets.len(), 3);
        assert_ne!(WasmTarget::WasiPreview1, WasmTarget::WasiPreview2);
        assert_ne!(WasmTarget::WasiPreview1, WasmTarget::CoreModule);
        assert_ne!(WasmTarget::WasiPreview2, WasmTarget::CoreModule);
    }

    #[test]
    fn test_wasm_target_serde_roundtrip() {
        for target in [
            WasmTarget::WasiPreview1,
            WasmTarget::WasiPreview2,
            WasmTarget::CoreModule,
        ] {
            let json = serde_json::to_string(&target).unwrap();
            let deserialized: WasmTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(target, deserialized);
        }
    }

    // --- OptLevel tests ---

    #[test]
    fn test_opt_level_values() {
        assert_ne!(OptLevel::None, OptLevel::Speed);
        assert_ne!(OptLevel::Speed, OptLevel::Size);
        assert_ne!(OptLevel::None, OptLevel::Size);
    }

    #[test]
    fn test_opt_level_serde_roundtrip() {
        for level in [OptLevel::None, OptLevel::Speed, OptLevel::Size] {
            let json = serde_json::to_string(&level).unwrap();
            let deserialized: OptLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, deserialized);
        }
    }

    // --- ToolchainConfig tests ---

    #[test]
    fn test_config_new_defaults() {
        let config = ToolchainConfig::new(Language::Rust);
        assert_eq!(config.language, Language::Rust);
        assert!(config.compiler_path.is_none());
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::None);
        assert!(config.extra_args.is_empty());
        assert!(config.env_vars.is_empty());
    }

    #[test]
    fn test_config_with_all_fields() {
        let config = ToolchainConfig {
            language: Language::Go,
            compiler_path: Some(PathBuf::from("/usr/local/bin/tinygo")),
            target: WasmTarget::WasiPreview1,
            optimization: OptLevel::Size,
            extra_args: vec!["build".into(), "-target=wasi".into()],
            env_vars: HashMap::from([("GOARCH".into(), "wasm".into())]),
        };
        assert_eq!(config.language, Language::Go);
        assert_eq!(
            config.compiler_path,
            Some(PathBuf::from("/usr/local/bin/tinygo"))
        );
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::Size);
        assert_eq!(config.extra_args.len(), 2);
        assert_eq!(config.env_vars.get("GOARCH"), Some(&"wasm".into()));
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = ToolchainConfig {
            language: Language::Rust,
            compiler_path: Some(PathBuf::from("/usr/bin/cargo")),
            target: WasmTarget::WasiPreview1,
            optimization: OptLevel::Speed,
            extra_args: vec!["--release".into()],
            env_vars: HashMap::from([("RUSTFLAGS".into(), "-C opt-level=3".into())]),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ToolchainConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.language, config.language);
        assert_eq!(deserialized.compiler_path, config.compiler_path);
        assert_eq!(deserialized.target, config.target);
        assert_eq!(deserialized.optimization, config.optimization);
        assert_eq!(deserialized.extra_args, config.extra_args);
        assert_eq!(deserialized.env_vars, config.env_vars);
    }

    // --- ToolchainRegistry tests ---

    #[test]
    fn test_registry_new_is_empty() {
        let registry = ToolchainRegistry::new();
        assert!(registry.available_languages().is_empty());
        assert!(registry.get(&Language::Rust).is_none());
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ToolchainRegistry::new();
        let config = ToolchainConfig::new(Language::Rust);
        registry.register(Language::Rust, config);

        let retrieved = registry.get(&Language::Rust);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().language, Language::Rust);
    }

    #[test]
    fn test_registry_available_languages() {
        let mut registry = ToolchainRegistry::new();
        registry.register(Language::Rust, ToolchainConfig::new(Language::Rust));
        registry.register(Language::Go, ToolchainConfig::new(Language::Go));

        let mut langs = registry.available_languages();
        langs.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        assert_eq!(langs.len(), 2);
        assert!(langs.contains(&Language::Rust));
        assert!(langs.contains(&Language::Go));
    }

    #[test]
    fn test_registry_overwrite() {
        let mut registry = ToolchainRegistry::new();

        let config1 = ToolchainConfig {
            optimization: OptLevel::None,
            ..ToolchainConfig::new(Language::Rust)
        };
        registry.register(Language::Rust, config1);
        assert_eq!(
            registry.get(&Language::Rust).unwrap().optimization,
            OptLevel::None
        );

        let config2 = ToolchainConfig {
            optimization: OptLevel::Speed,
            ..ToolchainConfig::new(Language::Rust)
        };
        registry.register(Language::Rust, config2);
        assert_eq!(
            registry.get(&Language::Rust).unwrap().optimization,
            OptLevel::Speed
        );
    }

    #[test]
    fn test_registry_get_missing() {
        let registry = ToolchainRegistry::new();
        assert!(registry.get(&Language::Python).is_none());
        assert!(registry.get(&Language::Other("Zig".into())).is_none());
    }

    #[test]
    fn test_registry_default_impl() {
        let registry = ToolchainRegistry::default();
        assert!(registry.available_languages().is_empty());
    }

    // --- Default config tests ---

    #[test]
    fn test_default_config_rust() {
        let config = ToolchainRegistry::default_config(&Language::Rust);
        assert_eq!(config.language, Language::Rust);
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::Size);
        assert!(config.extra_args.contains(&"--release".to_string()));
        assert!(config.extra_args.contains(&"wasm32-wasip1".to_string()));
    }

    #[test]
    fn test_default_config_go() {
        let config = ToolchainRegistry::default_config(&Language::Go);
        assert_eq!(config.language, Language::Go);
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::Size);
    }

    #[test]
    fn test_default_config_python() {
        let config = ToolchainRegistry::default_config(&Language::Python);
        assert_eq!(config.language, Language::Python);
        assert_eq!(config.target, WasmTarget::WasiPreview2);
        assert_eq!(config.optimization, OptLevel::None);
    }

    #[test]
    fn test_default_config_javascript() {
        let config = ToolchainRegistry::default_config(&Language::JavaScript);
        assert_eq!(config.language, Language::JavaScript);
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::Speed);
    }

    #[test]
    fn test_default_config_typescript() {
        let config = ToolchainRegistry::default_config(&Language::TypeScript);
        assert_eq!(config.language, Language::TypeScript);
        assert_eq!(config.optimization, OptLevel::Speed);
    }

    #[test]
    fn test_default_config_assemblyscript() {
        let config = ToolchainRegistry::default_config(&Language::AssemblyScript);
        assert_eq!(config.language, Language::AssemblyScript);
        assert_eq!(config.target, WasmTarget::CoreModule);
        assert_eq!(config.optimization, OptLevel::Size);
    }

    #[test]
    fn test_default_config_csharp() {
        let config = ToolchainRegistry::default_config(&Language::CSharp);
        assert_eq!(config.language, Language::CSharp);
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::Speed);
    }

    #[test]
    fn test_default_config_other() {
        let config = ToolchainRegistry::default_config(&Language::Other("Zig".into()));
        assert_eq!(config.language, Language::Other("Zig".into()));
        assert_eq!(config.target, WasmTarget::WasiPreview1);
        assert_eq!(config.optimization, OptLevel::None);
    }

    // --- CompileRequest tests ---

    #[test]
    fn test_compile_request_creation() {
        let request = CompileRequest {
            source_path: PathBuf::from("/project/src/main.rs"),
            output_path: PathBuf::from("/project/target/module.wasm"),
            language: Language::Rust,
            config: ToolchainConfig::new(Language::Rust),
        };
        assert_eq!(request.source_path, PathBuf::from("/project/src/main.rs"));
        assert_eq!(
            request.output_path,
            PathBuf::from("/project/target/module.wasm")
        );
        assert_eq!(request.language, Language::Rust);
    }

    // --- CompileResult tests ---

    #[test]
    fn test_compile_result_creation() {
        let result = CompileResult {
            output_path: PathBuf::from("/output/module.wasm"),
            output_size: 1024,
            compile_time: Duration::from_millis(500),
            warnings: vec!["unused variable".into()],
            target: WasmTarget::WasiPreview1,
        };
        assert_eq!(result.output_path, PathBuf::from("/output/module.wasm"));
        assert_eq!(result.output_size, 1024);
        assert_eq!(result.compile_time, Duration::from_millis(500));
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.target, WasmTarget::WasiPreview1);
    }

    #[test]
    fn test_compile_result_no_warnings() {
        let result = CompileResult {
            output_path: PathBuf::from("/output/module.wasm"),
            output_size: 2048,
            compile_time: Duration::from_secs(3),
            warnings: Vec::new(),
            target: WasmTarget::CoreModule,
        };
        assert!(result.warnings.is_empty());
        assert_eq!(result.target, WasmTarget::CoreModule);
    }

    // --- ToolchainDetector tests ---

    #[test]
    fn test_detector_known_compilers_coverage() {
        let compilers = ToolchainDetector::known_compilers();
        let languages: Vec<Language> = compilers.iter().map(|(l, _)| l.clone()).collect();
        assert!(languages.contains(&Language::Rust));
        assert!(languages.contains(&Language::Go));
        assert!(languages.contains(&Language::Python));
        assert!(languages.contains(&Language::JavaScript));
        assert!(languages.contains(&Language::TypeScript));
        assert!(languages.contains(&Language::AssemblyScript));
        assert!(languages.contains(&Language::CSharp));
    }

    #[test]
    fn test_detector_find_in_path_nonexistent() {
        // A binary that should never exist.
        let result = ToolchainDetector::find_in_path("__isolate_nonexistent_binary_12345__");
        assert!(result.is_none());
    }

    #[test]
    fn test_detector_detect_returns_vec() {
        // detect_all should return a vec (may be empty depending on environment).
        let installed = ToolchainDetector::detect_all();
        // Each entry should have a valid path.
        for (lang, path) in &installed {
            assert!(path.is_file(), "{:?} path {:?} should exist", lang, path);
        }
    }

    #[test]
    fn test_detector_detect_specific_nonexistent() {
        let result = ToolchainDetector::detect(&Language::Other("__nonexistent__".into()));
        assert!(result.is_none());
    }

    #[test]
    fn test_detector_find_in_empty_path() {
        // Temporarily use an empty PATH to verify no false positives.
        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let result = ToolchainDetector::find_in_path("cargo");
        // Restore PATH before any assertions.
        if let Some(p) = original_path {
            std::env::set_var("PATH", p);
        } else {
            std::env::remove_var("PATH");
        }
        assert!(result.is_none());
    }
}
