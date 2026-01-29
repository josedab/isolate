//! Hot patcher implementation.

use super::{
    differ::{PatchBundle, WasmDiffer},
    version::{ModuleVersion, VersionManager},
    CapturedState, PatchInfo, PatchRequest, PatchState, PatchStats,
};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Configuration for hot patching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchConfig {
    /// Enable state preservation.
    pub preserve_state: bool,
    /// Enable automatic rollback on failure.
    pub auto_rollback: bool,
    /// Maximum state capture size.
    pub max_state_size: usize,
    /// Timeout for patch application.
    pub apply_timeout_ms: u64,
    /// Verify patch before apply.
    pub verify_before_apply: bool,
    /// Keep history of patches.
    pub keep_history: usize,
}

impl Default for PatchConfig {
    fn default() -> Self {
        Self {
            preserve_state: true,
            auto_rollback: true,
            max_state_size: 64 * 1024 * 1024, // 64MB
            apply_timeout_ms: 5000,
            verify_before_apply: true,
            keep_history: 10,
        }
    }
}

/// Result of a patch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchResult {
    /// Patch ID.
    pub patch_id: String,
    /// Whether patch was successful.
    pub success: bool,
    /// New version after patch.
    pub new_version: Option<ModuleVersion>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Time taken to apply.
    pub duration_ms: u64,
    /// State preserved.
    pub state_preserved: bool,
    /// Rollback performed.
    pub rolled_back: bool,
}

impl PatchResult {
    /// Create a successful result.
    pub fn success(
        patch_id: String,
        new_version: ModuleVersion,
        duration_ms: u64,
        state_preserved: bool,
    ) -> Self {
        Self {
            patch_id,
            success: true,
            new_version: Some(new_version),
            error: None,
            duration_ms,
            state_preserved,
            rolled_back: false,
        }
    }

    /// Create a failure result.
    pub fn failure(patch_id: String, error: String, rolled_back: bool) -> Self {
        Self {
            patch_id,
            success: false,
            new_version: None,
            error: Some(error),
            duration_ms: 0,
            state_preserved: false,
            rolled_back,
        }
    }
}

/// The hot patcher.
pub struct HotPatcher {
    /// Configuration.
    config: PatchConfig,
    /// WASM differ.
    differ: WasmDiffer,
    /// Version manager.
    version_manager: Arc<RwLock<VersionManager>>,
    /// Active patches.
    active_patches: Arc<RwLock<HashMap<String, PatchOperation>>>,
    /// Patch history.
    history: Arc<RwLock<Vec<PatchInfo>>>,
    /// Statistics.
    stats: Arc<RwLock<PatchStats>>,
}

/// An active patch operation.
struct PatchOperation {
    /// Request.
    request: PatchRequest,
    /// Current state.
    state: PatchState,
    /// Patch bundle.
    bundle: Option<PatchBundle>,
    /// Captured state.
    captured_state: Option<CapturedState>,
    /// Original module for rollback.
    original_module: Option<Vec<u8>>,
    /// Started time.
    started_at: Instant,
}

impl HotPatcher {
    /// Create a new hot patcher.
    pub fn new(config: PatchConfig) -> Self {
        Self {
            config,
            differ: WasmDiffer::new(),
            version_manager: Arc::new(RwLock::new(VersionManager::new())),
            active_patches: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(PatchStats::default())),
        }
    }

    /// Create with default configuration.
    pub fn default_patcher() -> Self {
        Self::new(PatchConfig::default())
    }

    /// Apply a hot patch.
    pub fn apply(&self, request: PatchRequest, current_module: &[u8]) -> Result<PatchResult> {
        let patch_id = generate_id();
        let start = Instant::now();

        // Create patch operation
        let operation = PatchOperation {
            request: request.clone(),
            state: PatchState::Analyzing,
            bundle: None,
            captured_state: None,
            original_module: Some(current_module.to_vec()),
            started_at: start,
        };

        // Store operation
        {
            let mut patches = self
                .active_patches
                .write()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            patches.insert(patch_id.clone(), operation);
        }

        // Analyze and create patch bundle
        let bundle = self.differ.diff(current_module, &request.new_module);

        // Verify expected version if provided
        if let Some(expected) = &request.expected_version {
            let current = self.get_current_version(&request.sandbox_id)?;
            if let Some(current) = current {
                if current != *expected {
                    let error =
                        format!("Version mismatch: expected {:?}, got {:?}", expected, current);
                    return Ok(self.fail_patch(&patch_id, error)?);
                }
            }
        }

        // Update operation state
        self.update_operation(&patch_id, |op| {
            op.state = PatchState::CapturingState;
            op.bundle = Some(bundle);
        })?;

        // Capture state if required
        let captured_state =
            if request.preserve_state { self.capture_state(&request.sandbox_id)? } else { None };

        self.update_operation(&patch_id, |op| {
            op.state = PatchState::Applying;
            op.captured_state = captured_state;
        })?;

        // Verify patch before apply
        if self.config.verify_before_apply {
            let op = self.get_operation(&patch_id)?;
            if let Some(bundle) = &op.bundle {
                if !bundle.verify(current_module, &request.new_module) {
                    let error = "Patch verification failed".to_string();
                    return Ok(self.fail_patch(&patch_id, error)?);
                }
            }
        }

        // Apply patch (simulate)
        // In production, this would actually update the sandbox
        self.update_operation(&patch_id, |op| {
            op.state = PatchState::Verifying;
        })?;

        // Create new version
        let new_version = {
            let mut vm = self
                .version_manager
                .write()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

            let version = ModuleVersion::new(&request.new_module);
            vm.register_version(request.sandbox_id.clone(), version.clone());
            version
        };

        // Complete
        self.update_operation(&patch_id, |op| {
            op.state = PatchState::Complete;
        })?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let state_preserved = request.preserve_state;

        // Record history
        self.record_patch(
            &patch_id,
            current_module,
            &request.new_module,
            duration_ms,
            state_preserved,
        )?;

        // Update stats
        self.update_stats(true, duration_ms)?;

        // Clean up
        {
            let mut patches = self
                .active_patches
                .write()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            patches.remove(&patch_id);
        }

        Ok(PatchResult::success(patch_id, new_version, duration_ms, state_preserved))
    }

    /// Rollback a patch.
    pub fn rollback(&self, sandbox_id: &str) -> Result<PatchResult> {
        let vm =
            self.version_manager.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        if let Some(prev) = vm.previous_version(sandbox_id) {
            // In production, would restore the previous module
            self.update_stats(false, 0)?;

            Ok(PatchResult {
                patch_id: generate_id(),
                success: true,
                new_version: Some(prev.clone()),
                error: None,
                duration_ms: 0,
                state_preserved: false,
                rolled_back: true,
            })
        } else {
            Err(Error::Engine("No previous version to rollback to".to_string()))
        }
    }

    /// Get current version of a sandbox module.
    pub fn get_current_version(&self, sandbox_id: &str) -> Result<Option<ModuleVersion>> {
        let vm =
            self.version_manager.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(vm.current_version(sandbox_id).cloned())
    }

    /// Get patch statistics.
    pub fn stats(&self) -> Result<PatchStats> {
        let stats = self.stats.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(stats.clone())
    }

    /// Get patch history.
    pub fn history(&self, limit: usize) -> Result<Vec<PatchInfo>> {
        let history =
            self.history.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(history.iter().rev().take(limit).cloned().collect())
    }

    // Private helpers

    fn capture_state(&self, _sandbox_id: &str) -> Result<Option<CapturedState>> {
        // In production, would capture actual sandbox state
        Ok(Some(CapturedState::empty()))
    }

    fn update_operation<F>(&self, patch_id: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut PatchOperation),
    {
        let mut patches =
            self.active_patches.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        if let Some(op) = patches.get_mut(patch_id) {
            f(op);
        }
        Ok(())
    }

    fn get_operation(&self, patch_id: &str) -> Result<PatchOperation> {
        let patches =
            self.active_patches.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        patches
            .get(patch_id)
            .cloned()
            .ok_or_else(|| Error::Engine("Patch operation not found".to_string()))
    }

    fn fail_patch(&self, patch_id: &str, error: String) -> Result<PatchResult> {
        let rolled_back = if self.config.auto_rollback {
            self.update_operation(patch_id, |op| {
                op.state = PatchState::RollingBack;
            })?;
            // In production, would restore original module
            true
        } else {
            false
        };

        self.update_operation(patch_id, |op| {
            op.state = PatchState::Failed;
        })?;

        self.update_stats(false, 0)?;

        Ok(PatchResult::failure(patch_id.to_string(), error, rolled_back))
    }

    fn record_patch(
        &self,
        patch_id: &str,
        source: &[u8],
        target: &[u8],
        duration_ms: u64,
        state_preserved: bool,
    ) -> Result<()> {
        let info = PatchInfo {
            id: patch_id.to_string(),
            from_version: ModuleVersion::new(source),
            to_version: ModuleVersion::new(target),
            patch_size: target.len() - source.len().min(target.len()),
            full_size: target.len(),
            compression_ratio: 0.0, // Would calculate from actual diff
            apply_duration_ms: duration_ms,
            state_preserved,
            applied_at: std::time::SystemTime::now(),
        };

        let mut history =
            self.history.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        history.push(info);

        // Trim history
        while history.len() > self.config.keep_history {
            history.remove(0);
        }

        Ok(())
    }

    fn update_stats(&self, success: bool, duration_ms: u64) -> Result<()> {
        let mut stats =
            self.stats.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        stats.total_patches += 1;
        if success {
            stats.successful_patches += 1;
            let n = stats.successful_patches as f64;
            stats.avg_apply_time_ms =
                (stats.avg_apply_time_ms * (n - 1.0) + duration_ms as f64) / n;
        } else {
            stats.failed_patches += 1;
        }

        Ok(())
    }
}

impl Clone for PatchOperation {
    fn clone(&self) -> Self {
        Self {
            request: self.request.clone(),
            state: self.state,
            bundle: self.bundle.clone(),
            captured_state: self.captured_state.clone(),
            original_module: self.original_module.clone(),
            started_at: self.started_at,
        }
    }
}

/// Generate a unique ID.
fn generate_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("patch-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_config_default() {
        let config = PatchConfig::default();
        assert!(config.preserve_state);
        assert!(config.auto_rollback);
        assert!(config.verify_before_apply);
    }

    #[test]
    fn test_patch_result_success() {
        let version = ModuleVersion::new(b"test");
        let result = PatchResult::success("patch-1".to_string(), version, 100, true);

        assert!(result.success);
        assert!(result.new_version.is_some());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_patch_result_failure() {
        let result = PatchResult::failure("patch-1".to_string(), "error".to_string(), true);

        assert!(!result.success);
        assert!(result.new_version.is_none());
        assert!(result.error.is_some());
        assert!(result.rolled_back);
    }

    #[test]
    fn test_hot_patcher_apply() {
        let patcher = HotPatcher::default_patcher();
        let current = b"Hello World";
        let new = b"Hello Rust";

        let request = PatchRequest::new("sandbox-1", new.to_vec());
        let result = patcher.apply(request, current).unwrap();

        assert!(result.success);
        assert!(result.new_version.is_some());
    }

    #[test]
    fn test_hot_patcher_stats() {
        let patcher = HotPatcher::default_patcher();

        let stats = patcher.stats().unwrap();
        assert_eq!(stats.total_patches, 0);
    }
}
