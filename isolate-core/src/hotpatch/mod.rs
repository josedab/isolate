//! Hot Code Patching for WebAssembly Modules
//!
//! **WARNING: This module is experimental and not production-ready.**
//! Patch operations are currently simulated. The API may change significantly.
//!
//! Enables updating WASM modules in running sandboxes without restart:
//! - Binary diff-based patching
//! - State preservation during updates
//! - Rollback on failure
//! - Version management
//!
//! # How It Works
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                   Hot Patch Flow                    │
//! │                                                     │
//! │  ┌──────────┐    ┌──────────┐    ┌──────────┐      │
//! │  │ V1 WASM  │───▶│  Differ  │───▶│  Patch   │      │
//! │  └──────────┘    │          │    │ Bundle   │      │
//! │                  └──────────┘    └────┬─────┘      │
//! │  ┌──────────┐         │               │            │
//! │  │ V2 WASM  │─────────┘               ▼            │
//! │  └──────────┘                  ┌──────────┐        │
//! │                                │  Apply   │        │
//! │  ┌─────────────────────────────┤  Patch   │        │
//! │  │ Running Sandbox             └────┬─────┘        │
//! │  │  ┌─────────┐  ┌─────────┐       │              │
//! │  │  │ State   │  │ Memory  │◀──────┘              │
//! │  │  │ Capture │  │ Migrate │                       │
//! │  │  └─────────┘  └─────────┘                       │
//! │  └─────────────────────────────────────────────────┘
//! └─────────────────────────────────────────────────────┘
//! ```

mod differ;
mod patcher;
mod version;

pub use differ::{DiffChunk, PatchBundle, WasmDiffer};
pub use patcher::{HotPatcher, PatchConfig, PatchResult};
pub use version::{ModuleVersion, VersionHistory, VersionManager};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// State of a hot patch operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchState {
    /// Patch is queued.
    Pending,
    /// Analyzing differences.
    Analyzing,
    /// Capturing current state.
    CapturingState,
    /// Applying patch.
    Applying,
    /// Verifying patch.
    Verifying,
    /// Patch complete.
    Complete,
    /// Patch failed, rolling back.
    RollingBack,
    /// Patch failed.
    Failed,
}

/// A hot patch request.
#[derive(Debug, Clone)]
pub struct PatchRequest {
    /// Target sandbox ID.
    pub sandbox_id: String,
    /// New module bytes.
    pub new_module: Vec<u8>,
    /// Expected current version (for optimistic locking).
    pub expected_version: Option<ModuleVersion>,
    /// Preserve sandbox state.
    pub preserve_state: bool,
    /// Rollback on failure.
    pub rollback_on_failure: bool,
    /// Patch metadata.
    pub metadata: HashMap<String, String>,
}

impl PatchRequest {
    /// Create a new patch request.
    pub fn new(sandbox_id: impl Into<String>, new_module: Vec<u8>) -> Self {
        Self {
            sandbox_id: sandbox_id.into(),
            new_module,
            expected_version: None,
            preserve_state: true,
            rollback_on_failure: true,
            metadata: HashMap::new(),
        }
    }

    /// Set expected version.
    pub fn with_expected_version(mut self, version: ModuleVersion) -> Self {
        self.expected_version = Some(version);
        self
    }

    /// Disable state preservation.
    pub fn without_state_preservation(mut self) -> Self {
        self.preserve_state = false;
        self
    }

    /// Disable rollback.
    pub fn without_rollback(mut self) -> Self {
        self.rollback_on_failure = false;
        self
    }
}

/// Information about an applied patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    /// Patch ID.
    pub id: String,
    /// Source version.
    pub from_version: ModuleVersion,
    /// Target version.
    pub to_version: ModuleVersion,
    /// Patch size in bytes.
    pub patch_size: usize,
    /// Full module size.
    pub full_size: usize,
    /// Compression ratio (patch_size / full_size).
    pub compression_ratio: f64,
    /// Time to apply.
    pub apply_duration_ms: u64,
    /// State preserved.
    pub state_preserved: bool,
    /// Applied timestamp.
    pub applied_at: std::time::SystemTime,
}

/// Captured sandbox state for migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedState {
    /// Memory snapshot.
    pub memory: Vec<u8>,
    /// Global variables.
    pub globals: Vec<GlobalValue>,
    /// Table entries.
    pub tables: Vec<TableSnapshot>,
    /// Call stack info (if available).
    pub call_stack: Option<CallStackInfo>,
    /// Custom state data.
    pub custom: HashMap<String, Vec<u8>>,
}

impl CapturedState {
    /// Create empty state.
    pub fn empty() -> Self {
        Self {
            memory: Vec::new(),
            globals: Vec::new(),
            tables: Vec::new(),
            call_stack: None,
            custom: HashMap::new(),
        }
    }

    /// Get total size.
    pub fn size(&self) -> usize {
        self.memory.len()
            + self.globals.iter().map(|g| g.size()).sum::<usize>()
            + self.custom.values().map(|v| v.len()).sum::<usize>()
    }
}

/// A global variable value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalValue {
    /// Global index.
    pub index: u32,
    /// Value type.
    pub value_type: ValueType,
    /// Value bytes.
    pub value: Vec<u8>,
}

impl GlobalValue {
    /// Get size in bytes.
    pub fn size(&self) -> usize {
        self.value.len()
    }
}

/// WebAssembly value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}

/// Table snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSnapshot {
    /// Table index.
    pub index: u32,
    /// Table entries.
    pub entries: Vec<Option<u32>>,
}

/// Call stack information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallStackInfo {
    /// Stack frames.
    pub frames: Vec<StackFrame>,
    /// Current instruction pointer.
    pub instruction_pointer: u64,
}

/// A stack frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    /// Function index.
    pub function_index: u32,
    /// Local variables.
    pub locals: Vec<GlobalValue>,
    /// Return address.
    pub return_addr: u64,
}

/// Statistics about hot patching.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatchStats {
    /// Total patches applied.
    pub total_patches: u64,
    /// Successful patches.
    pub successful_patches: u64,
    /// Failed patches.
    pub failed_patches: u64,
    /// Rollbacks performed.
    pub rollbacks: u64,
    /// Average patch size.
    pub avg_patch_size: f64,
    /// Average apply time ms.
    pub avg_apply_time_ms: f64,
    /// State bytes transferred.
    pub state_bytes_transferred: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_request() {
        let req = PatchRequest::new("sandbox-1", vec![0, 1, 2, 3]).without_state_preservation();

        assert_eq!(req.sandbox_id, "sandbox-1");
        assert!(!req.preserve_state);
        assert!(req.rollback_on_failure);
    }

    #[test]
    fn test_captured_state() {
        let mut state = CapturedState::empty();
        state.memory = vec![0u8; 1024];
        state.globals.push(GlobalValue {
            index: 0,
            value_type: ValueType::I32,
            value: vec![1, 0, 0, 0],
        });

        assert_eq!(state.size(), 1028);
    }

    #[test]
    fn test_value_type() {
        assert_eq!(ValueType::I32, ValueType::I32);
        assert_ne!(ValueType::I32, ValueType::I64);
    }
}
