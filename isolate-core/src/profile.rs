//! Language-specific optimization profiles for sandbox configuration.
//!
//! Pre-tuned resource configurations for common WASM-compiled languages.
//! Each profile provides sensible defaults for memory, CPU, I/O, and
//! capabilities based on the typical needs of that language's runtime.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::profile::LanguageProfile;
//! use isolate_core::SandboxConfig;
//!
//! # fn example() -> isolate_core::Result<()> {
//! # let wasm_bytes = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
//! let config = SandboxConfig::builder()
//!     .module(wasm_bytes)?
//!     .apply_profile(LanguageProfile::Rust)
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use crate::capability::Capability;
use crate::resource::{CpuLimits, IoLimits, MemoryLimits, ResourceLimits, TimeLimits};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A language-specific optimization profile with pre-tuned resource defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LanguageProfile {
    /// Rust: minimal heap, low stack, efficient fuel usage.
    Rust,
    /// Python: large heap for interpreter, temp dir, generous stack.
    Python,
    /// JavaScript: medium heap, high I/O limits for event loops.
    JavaScript,
    /// Go: large stack for goroutines, medium heap.
    Go,
    /// C/C++: customizable stack, file I/O oriented.
    C,
}

impl LanguageProfile {
    /// Get the recommended resource limits for this language profile.
    pub fn resource_limits(&self) -> ResourceLimits {
        match self {
            Self::Rust => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 16 * 1024 * 1024,  // 16MB
                    stack_max: 512 * 1024,       // 512KB
                    total_max: 32 * 1024 * 1024, // 32MB
                },
                cpu: CpuLimits {
                    fuel: Some(50_000_000), // 50M - Rust is efficient
                    cpu_time: Some(Duration::from_secs(10)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(10 * 1024 * 1024), // 10MB
                    write_bytes: Some(1024 * 1024),     // 1MB
                    iops: Some(500),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(30)),
                    cpu_time: Some(Duration::from_secs(10)),
                },
            },
            Self::Python => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 128 * 1024 * 1024,  // 128MB - interpreter needs room
                    stack_max: 2 * 1024 * 1024,   // 2MB
                    total_max: 256 * 1024 * 1024, // 256MB
                },
                cpu: CpuLimits {
                    fuel: Some(500_000_000), // 500M - interpreted, needs more
                    cpu_time: Some(Duration::from_secs(60)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(50 * 1024 * 1024),  // 50MB
                    write_bytes: Some(10 * 1024 * 1024), // 10MB
                    iops: Some(2000),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(120)),
                    cpu_time: Some(Duration::from_secs(60)),
                },
            },
            Self::JavaScript => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 64 * 1024 * 1024,   // 64MB
                    stack_max: 1024 * 1024,       // 1MB
                    total_max: 128 * 1024 * 1024, // 128MB
                },
                cpu: CpuLimits {
                    fuel: Some(200_000_000), // 200M
                    cpu_time: Some(Duration::from_secs(30)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(50 * 1024 * 1024), // 50MB - high I/O for event loops
                    write_bytes: Some(10 * 1024 * 1024), // 10MB
                    iops: Some(5000),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(60)),
                    cpu_time: Some(Duration::from_secs(30)),
                },
            },
            Self::Go => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 64 * 1024 * 1024,   // 64MB
                    stack_max: 4 * 1024 * 1024,   // 4MB - goroutines need stack
                    total_max: 128 * 1024 * 1024, // 128MB
                },
                cpu: CpuLimits {
                    fuel: Some(100_000_000), // 100M
                    cpu_time: Some(Duration::from_secs(30)),
                    preemption_interval: Duration::from_millis(5),
                },
                io: IoLimits {
                    read_bytes: Some(50 * 1024 * 1024),  // 50MB
                    write_bytes: Some(10 * 1024 * 1024), // 10MB
                    iops: Some(2000),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(60)),
                    cpu_time: Some(Duration::from_secs(30)),
                },
            },
            Self::C => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 32 * 1024 * 1024,  // 32MB
                    stack_max: 2 * 1024 * 1024,  // 2MB
                    total_max: 64 * 1024 * 1024, // 64MB
                },
                cpu: CpuLimits {
                    fuel: Some(50_000_000), // 50M - C is efficient
                    cpu_time: Some(Duration::from_secs(10)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(50 * 1024 * 1024),  // 50MB - file I/O oriented
                    write_bytes: Some(10 * 1024 * 1024), // 10MB
                    iops: Some(1000),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(30)),
                    cpu_time: Some(Duration::from_secs(10)),
                },
            },
        }
    }

    /// Get recommended default capabilities for this language profile.
    pub fn default_capabilities(&self) -> Vec<Capability> {
        match self {
            Self::Rust => vec![Capability::stdout(), Capability::stderr()],
            Self::Python => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::temp_dir(),
                Capability::monotonic_clock(),
                Capability::secure_random(),
            ],
            Self::JavaScript => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::monotonic_clock(),
                Capability::timers(),
                Capability::secure_random(),
            ],
            Self::Go => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::monotonic_clock(),
                Capability::secure_random(),
            ],
            Self::C => vec![Capability::stdout(), Capability::stderr()],
        }
    }

    /// Get a human-readable description of this profile.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Rust => "Rust: minimal heap (16MB), low stack (512KB), efficient fuel (50M)",
            Self::Python => {
                "Python: large heap (128MB), temp dir, generous stack (2MB), high fuel (500M)"
            }
            Self::JavaScript => "JavaScript: medium heap (64MB), high I/O limits, timers enabled",
            Self::Go => "Go: large stack (4MB), medium heap (64MB), fast preemption",
            Self::C => "C/C++: moderate heap (32MB), custom stack (2MB), file I/O oriented",
        }
    }

    /// Attempt to detect the language profile from WASM module metadata.
    ///
    /// Inspects custom sections and import patterns to guess the source language.
    /// Returns `None` if the language cannot be determined.
    pub fn detect_from_module(wasm_bytes: &[u8]) -> Option<Self> {
        // Check for known custom section markers
        let bytes_str = String::from_utf8_lossy(wasm_bytes);

        if bytes_str.contains("rustc") || bytes_str.contains(".rust") {
            return Some(Self::Rust);
        }
        if bytes_str.contains("cpython") || bytes_str.contains("Python") {
            return Some(Self::Python);
        }
        if bytes_str.contains("wasm-bindgen") || bytes_str.contains("javy") {
            return Some(Self::JavaScript);
        }
        if bytes_str.contains("tinygo") || bytes_str.contains("go.buildid") {
            return Some(Self::Go);
        }
        if bytes_str.contains("clang") || bytes_str.contains("emscripten") {
            return Some(Self::C);
        }

        None
    }
}

impl std::fmt::Display for LanguageProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::Go => "go",
            Self::C => "c",
        };
        write!(f, "{}", name)
    }
}

impl std::str::FromStr for LanguageProfile {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rust" | "rs" => Ok(Self::Rust),
            "python" | "py" => Ok(Self::Python),
            "javascript" | "js" | "typescript" | "ts" => Ok(Self::JavaScript),
            "go" | "golang" => Ok(Self::Go),
            "c" | "cpp" | "c++" | "cc" => Ok(Self::C),
            _ => Err(format!(
                "Unknown language profile: '{}'. Available: rust, python, javascript, go, c",
                s
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_resource_limits() {
        let rust = LanguageProfile::Rust.resource_limits();
        assert_eq!(rust.memory.heap_max, 16 * 1024 * 1024);
        assert_eq!(rust.memory.stack_max, 512 * 1024);

        let python = LanguageProfile::Python.resource_limits();
        assert_eq!(python.memory.heap_max, 128 * 1024 * 1024);
        assert!(python.cpu.fuel.unwrap() > rust.cpu.fuel.unwrap());
    }

    #[test]
    fn test_profile_capabilities() {
        let rust_caps = LanguageProfile::Rust.default_capabilities();
        assert_eq!(rust_caps.len(), 2); // stdout + stderr

        let python_caps = LanguageProfile::Python.default_capabilities();
        assert!(python_caps.len() > rust_caps.len()); // More capabilities needed
        assert!(python_caps.contains(&Capability::temp_dir()));
    }

    #[test]
    fn test_profile_from_str() {
        assert_eq!("rust".parse::<LanguageProfile>().unwrap(), LanguageProfile::Rust);
        assert_eq!("py".parse::<LanguageProfile>().unwrap(), LanguageProfile::Python);
        assert_eq!("js".parse::<LanguageProfile>().unwrap(), LanguageProfile::JavaScript);
        assert_eq!("golang".parse::<LanguageProfile>().unwrap(), LanguageProfile::Go);
        assert_eq!("cpp".parse::<LanguageProfile>().unwrap(), LanguageProfile::C);
        assert!("unknown".parse::<LanguageProfile>().is_err());
    }

    #[test]
    fn test_profile_display() {
        assert_eq!(LanguageProfile::Rust.to_string(), "rust");
        assert_eq!(LanguageProfile::Python.to_string(), "python");
    }

    #[test]
    fn test_profile_detect_rust() {
        let mut wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        wasm.extend_from_slice(b"rustc version 1.75");
        assert_eq!(LanguageProfile::detect_from_module(&wasm), Some(LanguageProfile::Rust));
    }

    #[test]
    fn test_profile_detect_go() {
        let mut wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        wasm.extend_from_slice(b"tinygo version 0.30");
        assert_eq!(LanguageProfile::detect_from_module(&wasm), Some(LanguageProfile::Go));
    }

    #[test]
    fn test_profile_detect_unknown() {
        let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        assert_eq!(LanguageProfile::detect_from_module(&wasm), None);
    }

    #[test]
    fn test_all_profiles_have_stdout() {
        for profile in [
            LanguageProfile::Rust,
            LanguageProfile::Python,
            LanguageProfile::JavaScript,
            LanguageProfile::Go,
            LanguageProfile::C,
        ] {
            let caps = profile.default_capabilities();
            assert!(caps.contains(&Capability::stdout()), "Profile {} missing stdout", profile);
        }
    }
}
