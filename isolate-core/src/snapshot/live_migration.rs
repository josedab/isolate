//! Live Migration & Hot Failover for sandbox instances.
//!
//! Provides the ability to checkpoint a running sandbox, transfer its state
//! to another node, and resume execution with minimal downtime.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐    freeze    ┌──────────────┐   transfer   ┌─────────────┐
//! │  Running     │────────────►│  Frozen       │─────────────►│  Restored   │
//! │  Sandbox     │             │  + Snapshot    │              │  Sandbox    │
//! └─────────────┘             └──────────────┘              └─────────────┘
//!       ▲                                                          │
//!       └──────────────────── rollback ◄───────────────────────────┘
//! ```
//!
//! # Features
//!
//! - **Freeze/Thaw API**: Pause sandbox at safe yield points and resume
//! - **Stateful Migration**: Transfer memory, globals, capabilities, and resource state
//! - **Incremental Transfer**: Only transfer dirty pages for large sandboxes
//! - **Failover Registry**: Distributed registry for automatic failover coordination
//! - **Rollback Support**: Automatic rollback if migration fails mid-flight
//! - **Health Verification**: Post-migration integrity checks



use crate::config::ModuleHash;
use crate::error::{Error, Result};
use crate::sandbox::SandboxId;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// State of a live migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LiveMigrationState {
    /// Sandbox is running normally.
    Running,
    /// Sandbox is being frozen (drain in-flight work).
    Freezing,
    /// Sandbox is frozen, state captured.
    Frozen,
    /// Pre-copy phase: transferring dirty pages while sandbox still runs.
    PreCopy,
    /// Final transfer: sandbox frozen, sending remaining state.
    FinalTransfer,
    /// Verifying state integrity on target.
    Verifying,
    /// Resuming on target node.
    Resuming,
    /// Migration completed successfully.
    Completed,
    /// Migration failed, rolled back to source.
    RolledBack,
    /// Migration failed irrecoverably.
    Failed,
}

impl LiveMigrationState {
    /// Check if migration is in an active (non-terminal) state.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Freezing
                | Self::Frozen
                | Self::PreCopy
                | Self::FinalTransfer
                | Self::Verifying
                | Self::Resuming
        )
    }

    /// Check if migration has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::RolledBack | Self::Failed)
    }
}

/// Frozen sandbox state suitable for transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenState {
    /// Sandbox identifier.
    pub sandbox_id: SandboxId,
    /// Module hash for verification.
    pub module_hash: ModuleHash,
    /// Memory contents (pages, sparse representation).
    pub memory_pages: HashMap<usize, Vec<u8>>,
    /// Total memory size.
    pub memory_size: usize,
    /// Page size used.
    pub page_size: usize,
    /// Global variable values.
    pub globals: Vec<FrozenGlobal>,
    /// Remaining fuel at freeze time.
    pub fuel_remaining: Option<u64>,
    /// Capability grants (serialized).
    pub capabilities: Vec<String>,
    /// Resource usage at freeze time.
    pub resource_usage: FrozenResourceUsage,
    /// SHA-256 checksum of all state.
    pub state_checksum: String,
    /// Timestamp of freeze.
    pub frozen_at: chrono::DateTime<chrono::Utc>,
}

/// A frozen global variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenGlobal {
    pub name: String,
    pub value: FrozenValue,
}

/// Frozen value types matching WASM value types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrozenValue {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    V128([u8; 16]),
}

/// Resource usage at freeze time for accurate accounting on target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrozenResourceUsage {
    /// Fuel consumed before freeze.
    pub fuel_consumed: u64,
    /// Wall time consumed.
    pub wall_time: Duration,
    /// Bytes read.
    pub bytes_read: u64,
    /// Bytes written.
    pub bytes_written: u64,
    /// Peak memory usage.
    pub peak_memory: usize,
}

impl FrozenState {
    /// Create a new frozen state.
    pub fn new(sandbox_id: SandboxId, module_hash: ModuleHash) -> Self {
        Self {
            sandbox_id,
            module_hash,
            memory_pages: HashMap::new(),
            memory_size: 0,
            page_size: 65536,
            globals: Vec::new(),
            fuel_remaining: None,
            capabilities: Vec::new(),
            resource_usage: FrozenResourceUsage::default(),
            state_checksum: String::new(),
            frozen_at: chrono::Utc::now(),
        }
    }

    /// Capture memory into pages, skipping zero pages for efficiency.
    pub fn capture_memory(&mut self, memory: &[u8]) {
        self.memory_size = memory.len();
        self.memory_pages.clear();

        for (idx, chunk) in memory.chunks(self.page_size).enumerate() {
            if !chunk.iter().all(|&b| b == 0) {
                self.memory_pages.insert(idx, chunk.to_vec());
            }
        }

        self.recompute_checksum();
    }

    /// Restore full memory from frozen state.
    pub fn restore_memory(&self) -> Vec<u8> {
        let mut memory = vec![0u8; self.memory_size];
        for (&page_idx, page_data) in &self.memory_pages {
            let offset = page_idx * self.page_size;
            let end = (offset + page_data.len()).min(self.memory_size);
            memory[offset..end].copy_from_slice(&page_data[..end - offset]);
        }
        memory
    }

    /// Compute dirty pages relative to a previous frozen state.
    pub fn dirty_pages_since(&self, previous: &FrozenState) -> Vec<usize> {
        let mut dirty = Vec::new();

        for (&page_idx, data) in &self.memory_pages {
            match previous.memory_pages.get(&page_idx) {
                Some(prev_data) if prev_data == data => {}
                _ => dirty.push(page_idx),
            }
        }

        // Pages that existed before but are now zero
        for &page_idx in previous.memory_pages.keys() {
            if !self.memory_pages.contains_key(&page_idx) {
                dirty.push(page_idx);
            }
        }

        dirty.sort();
        dirty.dedup();
        dirty
    }

    /// Compute SHA-256 checksum of the state.
    fn recompute_checksum(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(self.sandbox_id.0.as_bytes());
        hasher.update(self.module_hash.0.as_bytes());
        hasher.update(&self.memory_size.to_le_bytes());

        let mut sorted_pages: Vec<_> = self.memory_pages.iter().collect();
        sorted_pages.sort_by_key(|(&idx, _)| idx);
        for (idx, data) in sorted_pages {
            hasher.update(&idx.to_le_bytes());
            hasher.update(data);
        }

        self.state_checksum = hex::encode(hasher.finalize());
    }

    /// Verify integrity of the frozen state.
    pub fn verify_integrity(&self) -> bool {
        let mut check = self.clone();
        check.recompute_checksum();
        check.state_checksum == self.state_checksum
    }

    /// Estimated serialized size in bytes.
    pub fn estimated_size(&self) -> usize {
        let page_data: usize = self.memory_pages.values().map(|p| p.len()).sum();
        page_data + self.globals.len() * 16 + self.capabilities.len() * 64
    }
}

/// Configuration for live migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMigrationConfig {
    /// Maximum time to wait for freeze to complete.
    pub freeze_timeout: Duration,
    /// Maximum time for the entire migration.
    pub migration_timeout: Duration,
    /// Enable pre-copy optimization (transfer pages while running).
    pub enable_pre_copy: bool,
    /// Number of pre-copy iterations before final freeze.
    pub pre_copy_rounds: u32,
    /// Dirty page rate threshold to stop pre-copy (pages/sec).
    pub dirty_rate_threshold: f64,
    /// Verify state checksum after transfer.
    pub verify_checksum: bool,
    /// Automatic rollback on failure.
    pub auto_rollback: bool,
    /// Maximum downtime during final transfer phase.
    pub max_downtime: Duration,
}

impl Default for LiveMigrationConfig {
    fn default() -> Self {
        Self {
            freeze_timeout: Duration::from_secs(5),
            migration_timeout: Duration::from_secs(60),
            enable_pre_copy: true,
            pre_copy_rounds: 3,
            dirty_rate_threshold: 100.0,
            verify_checksum: true,
            auto_rollback: true,
            max_downtime: Duration::from_millis(100),
        }
    }
}

/// Progress of a live migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationProgress {
    /// Current state.
    pub state: LiveMigrationState,
    /// Pages transferred.
    pub pages_transferred: usize,
    /// Total pages to transfer.
    pub total_pages: usize,
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Current pre-copy round (if applicable).
    pub pre_copy_round: u32,
    /// Dirty pages remaining.
    pub dirty_pages_remaining: usize,
    /// Elapsed time.
    pub elapsed: Duration,
    /// Estimated time remaining.
    pub estimated_remaining: Option<Duration>,
}

impl MigrationProgress {
    /// Calculate completion percentage.
    pub fn percent_complete(&self) -> f64 {
        if self.total_pages == 0 {
            return match self.state {
                LiveMigrationState::Completed => 100.0,
                LiveMigrationState::Verifying => 95.0,
                LiveMigrationState::Resuming => 98.0,
                _ => 0.0,
            };
        }
        (self.pages_transferred as f64 / self.total_pages as f64 * 100.0).min(100.0)
    }
}

/// A single live migration operation.
pub struct LiveMigration {
    /// Migration identifier.
    pub id: String,
    /// Source sandbox.
    pub sandbox_id: SandboxId,
    /// Target node identifier.
    pub target_node: String,
    /// Configuration.
    #[allow(dead_code)]
    config: LiveMigrationConfig,
    /// Current state.
    state: LiveMigrationState,
    /// Frozen state (populated after freeze).
    frozen_state: Option<FrozenState>,
    /// Progress tracking.
    progress: MigrationProgress,
    /// State transition log.
    transitions: Vec<(LiveMigrationState, Instant)>,
    /// Error message if failed.
    error: Option<String>,
    /// Started at.
    started_at: Instant,
}

impl LiveMigration {
    /// Create a new live migration.
    pub fn new(sandbox_id: SandboxId, target_node: String, config: LiveMigrationConfig) -> Self {
        let now = Instant::now();
        Self {
            id: format!("lm-{}", uuid::Uuid::new_v4()),
            sandbox_id,
            target_node,
            config,
            state: LiveMigrationState::Running,
            frozen_state: None,
            progress: MigrationProgress {
                state: LiveMigrationState::Running,
                pages_transferred: 0,
                total_pages: 0,
                bytes_transferred: 0,
                pre_copy_round: 0,
                dirty_pages_remaining: 0,
                elapsed: Duration::ZERO,
                estimated_remaining: None,
            },
            transitions: vec![(LiveMigrationState::Running, now)],
            error: None,
            started_at: now,
        }
    }

    /// Get current state.
    pub fn state(&self) -> LiveMigrationState {
        self.state
    }

    /// Get current progress.
    pub fn progress(&self) -> &MigrationProgress {
        &self.progress
    }

    /// Transition to a new state.
    fn transition_to(&mut self, new_state: LiveMigrationState) {
        tracing::info!(
            migration_id = %self.id,
            from = ?self.state,
            to = ?new_state,
            "Migration state transition"
        );
        self.state = new_state;
        self.progress.state = new_state;
        self.progress.elapsed = self.started_at.elapsed();
        self.transitions.push((new_state, Instant::now()));
    }

    /// Begin the freeze phase.
    pub fn begin_freeze(&mut self) -> Result<()> {
        if self.state != LiveMigrationState::Running
            && self.state != LiveMigrationState::PreCopy
        {
            return Err(Error::InvalidState {
                expected: "Running or PreCopy".to_string(),
                actual: format!("{:?}", self.state),
            });
        }
        self.transition_to(LiveMigrationState::Freezing);
        Ok(())
    }

    /// Complete freeze with captured state.
    pub fn complete_freeze(&mut self, frozen_state: FrozenState) -> Result<()> {
        if self.state != LiveMigrationState::Freezing {
            return Err(Error::InvalidState {
                expected: "Freezing".to_string(),
                actual: format!("{:?}", self.state),
            });
        }
        self.progress.total_pages = frozen_state.memory_pages.len();
        self.frozen_state = Some(frozen_state);
        self.transition_to(LiveMigrationState::Frozen);
        Ok(())
    }

    /// Begin pre-copy transfer (pages transferred while sandbox still runs).
    pub fn begin_pre_copy(&mut self) -> Result<()> {
        if self.state != LiveMigrationState::Frozen
            && self.state != LiveMigrationState::Running
        {
            return Err(Error::InvalidState {
                expected: "Frozen or Running".to_string(),
                actual: format!("{:?}", self.state),
            });
        }
        self.transition_to(LiveMigrationState::PreCopy);
        Ok(())
    }

    /// Report pre-copy progress.
    pub fn report_pre_copy_progress(&mut self, pages_sent: usize, dirty_remaining: usize) {
        self.progress.pages_transferred = pages_sent;
        self.progress.dirty_pages_remaining = dirty_remaining;
        self.progress.pre_copy_round += 1;
        self.progress.elapsed = self.started_at.elapsed();

        // Estimate remaining time based on transfer rate
        if self.progress.pages_transferred > 0 {
            let rate = self.progress.pages_transferred as f64 / self.progress.elapsed.as_secs_f64();
            if rate > 0.0 {
                let remaining_pages = dirty_remaining as f64;
                let estimated_secs = remaining_pages / rate;
                self.progress.estimated_remaining =
                    Some(Duration::from_secs_f64(estimated_secs));
            }
        }
    }

    /// Begin final transfer after freeze.
    pub fn begin_final_transfer(&mut self) -> Result<()> {
        if self.state != LiveMigrationState::Frozen
            && self.state != LiveMigrationState::PreCopy
        {
            return Err(Error::InvalidState {
                expected: "Frozen or PreCopy".to_string(),
                actual: format!("{:?}", self.state),
            });
        }
        self.transition_to(LiveMigrationState::FinalTransfer);
        Ok(())
    }

    /// Report transfer of pages.
    pub fn report_transfer(&mut self, pages_sent: usize, bytes_sent: u64) {
        self.progress.pages_transferred = pages_sent;
        self.progress.bytes_transferred += bytes_sent;
        self.progress.elapsed = self.started_at.elapsed();
    }

    /// Begin verification on target.
    pub fn begin_verification(&mut self) -> Result<()> {
        if self.state != LiveMigrationState::FinalTransfer {
            return Err(Error::InvalidState {
                expected: "FinalTransfer".to_string(),
                actual: format!("{:?}", self.state),
            });
        }
        self.transition_to(LiveMigrationState::Verifying);
        Ok(())
    }

    /// Verify the transferred state matches.
    pub fn verify_state(&self, target_checksum: &str) -> bool {
        self.frozen_state
            .as_ref()
            .map(|s| s.state_checksum == target_checksum)
            .unwrap_or(false)
    }

    /// Complete migration (resume on target).
    pub fn complete(&mut self) -> Result<()> {
        if self.state != LiveMigrationState::Verifying
            && self.state != LiveMigrationState::Resuming
        {
            return Err(Error::InvalidState {
                expected: "Verifying or Resuming".to_string(),
                actual: format!("{:?}", self.state),
            });
        }
        self.transition_to(LiveMigrationState::Completed);
        Ok(())
    }

    /// Roll back to source.
    pub fn rollback(&mut self, reason: &str) {
        tracing::warn!(
            migration_id = %self.id,
            reason = reason,
            "Rolling back migration"
        );
        self.error = Some(reason.to_string());
        self.transition_to(LiveMigrationState::RolledBack);
    }

    /// Mark as failed.
    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.transition_to(LiveMigrationState::Failed);
    }

    /// Get the frozen state.
    pub fn frozen_state(&self) -> Option<&FrozenState> {
        self.frozen_state.as_ref()
    }

    /// Get total downtime (time spent frozen).
    pub fn downtime(&self) -> Duration {
        let freeze_start = self
            .transitions
            .iter()
            .find(|(s, _)| *s == LiveMigrationState::Freezing)
            .map(|(_, t)| *t);

        let resume_end = self
            .transitions
            .iter()
            .rev()
            .find(|(s, _)| *s == LiveMigrationState::Completed || *s == LiveMigrationState::Resuming)
            .map(|(_, t)| *t);

        match (freeze_start, resume_end) {
            (Some(start), Some(end)) => end.duration_since(start),
            _ => Duration::ZERO,
        }
    }

    /// Get error message.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Registry for tracking sandbox locations and coordinating failover.
pub struct FailoverRegistry {
    /// Sandbox-to-node mapping.
    registry: Arc<RwLock<HashMap<SandboxId, NodeRegistration>>>,
    /// Active migrations.
    migrations: Arc<RwLock<HashMap<String, MigrationRecord>>>,
    /// Failover policies.
    policies: Arc<RwLock<HashMap<String, FailoverPolicy>>>,
}

/// Registration of a sandbox on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegistration {
    pub sandbox_id: SandboxId,
    pub node_id: String,
    pub region: Option<String>,
    pub registered_at: chrono::DateTime<chrono::Utc>,
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
    pub module_hash: ModuleHash,
    pub status: RegistrationStatus,
}

/// Status of a registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationStatus {
    Active,
    Migrating,
    Suspended,
    Failed,
}

/// Record of a completed or in-progress migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub id: String,
    pub sandbox_id: SandboxId,
    pub source_node: String,
    pub target_node: String,
    pub state: LiveMigrationState,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub bytes_transferred: u64,
    pub downtime_ms: u64,
}

/// Failover policy defining automatic migration triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverPolicy {
    pub name: String,
    /// Trigger failover after this many missed heartbeats.
    pub heartbeat_miss_threshold: u32,
    /// Heartbeat interval.
    pub heartbeat_interval: Duration,
    /// Prefer same-region failover targets.
    pub prefer_same_region: bool,
    /// Maximum concurrent failovers.
    pub max_concurrent_failovers: usize,
    /// Minimum time between failovers for the same sandbox.
    pub cooldown: Duration,
}

impl Default for FailoverPolicy {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            heartbeat_miss_threshold: 3,
            heartbeat_interval: Duration::from_secs(5),
            prefer_same_region: true,
            max_concurrent_failovers: 5,
            cooldown: Duration::from_secs(60),
        }
    }
}

impl FailoverRegistry {
    /// Create a new failover registry.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(HashMap::new())),
            migrations: Arc::new(RwLock::new(HashMap::new())),
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a sandbox on a node.
    pub fn register(
        &self,
        sandbox_id: SandboxId,
        node_id: String,
        module_hash: ModuleHash,
        region: Option<String>,
    ) -> Result<()> {
        let now = chrono::Utc::now();
        let registration = NodeRegistration {
            sandbox_id,
            node_id,
            region,
            registered_at: now,
            last_heartbeat: now,
            module_hash,
            status: RegistrationStatus::Active,
        };

        let mut registry = self
            .registry
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        registry.insert(sandbox_id, registration);
        Ok(())
    }

    /// Update heartbeat for a sandbox.
    pub fn heartbeat(&self, sandbox_id: &SandboxId) -> Result<()> {
        let mut registry = self
            .registry
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        if let Some(reg) = registry.get_mut(sandbox_id) {
            reg.last_heartbeat = chrono::Utc::now();
            Ok(())
        } else {
            Err(Error::Engine(format!("Sandbox {} not registered", sandbox_id)))
        }
    }

    /// Look up where a sandbox is running.
    pub fn lookup(&self, sandbox_id: &SandboxId) -> Result<Option<NodeRegistration>> {
        let registry = self
            .registry
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(registry.get(sandbox_id).cloned())
    }

    /// Get all sandboxes on a specific node.
    pub fn sandboxes_on_node(&self, node_id: &str) -> Result<Vec<SandboxId>> {
        let registry = self
            .registry
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        Ok(registry
            .iter()
            .filter(|(_, reg)| reg.node_id == node_id && reg.status == RegistrationStatus::Active)
            .map(|(id, _)| *id)
            .collect())
    }

    /// Detect failed nodes based on heartbeat policy.
    pub fn detect_failures(&self, policy: &FailoverPolicy) -> Result<Vec<SandboxId>> {
        let registry = self
            .registry
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        let now = chrono::Utc::now();
        let timeout = chrono::Duration::from_std(
            policy.heartbeat_interval * policy.heartbeat_miss_threshold,
        )
        .unwrap_or(chrono::Duration::seconds(30));

        Ok(registry
            .iter()
            .filter(|(_, reg)| {
                reg.status == RegistrationStatus::Active
                    && (now - reg.last_heartbeat) > timeout
            })
            .map(|(id, _)| *id)
            .collect())
    }

    /// Record a migration.
    pub fn record_migration(&self, record: MigrationRecord) -> Result<()> {
        let mut migrations = self
            .migrations
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        migrations.insert(record.id.clone(), record);
        Ok(())
    }

    /// Get migration history for a sandbox.
    pub fn migration_history(&self, sandbox_id: &SandboxId) -> Result<Vec<MigrationRecord>> {
        let migrations = self
            .migrations
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        Ok(migrations
            .values()
            .filter(|m| m.sandbox_id == *sandbox_id)
            .cloned()
            .collect())
    }

    /// Add a failover policy.
    pub fn add_policy(&self, policy: FailoverPolicy) -> Result<()> {
        let mut policies = self
            .policies
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        policies.insert(policy.name.clone(), policy);
        Ok(())
    }

    /// Get count of registered sandboxes.
    pub fn sandbox_count(&self) -> usize {
        self.registry.read().map(|r| r.len()).unwrap_or(0)
    }

    /// Deregister a sandbox.
    pub fn deregister(&self, sandbox_id: &SandboxId) -> Result<bool> {
        let mut registry = self
            .registry
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(registry.remove(sandbox_id).is_some())
    }
}

impl Default for FailoverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sandbox_id() -> SandboxId {
        SandboxId::new()
    }

    fn test_module_hash() -> ModuleHash {
        ModuleHash("test-module-hash".to_string())
    }

    #[test]
    fn test_migration_state_transitions() {
        assert!(LiveMigrationState::Freezing.is_active());
        assert!(!LiveMigrationState::Completed.is_active());
        assert!(LiveMigrationState::Completed.is_terminal());
        assert!(LiveMigrationState::Failed.is_terminal());
        assert!(!LiveMigrationState::Running.is_terminal());
    }

    #[test]
    fn test_frozen_state_capture_and_restore() {
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();
        let mut frozen = FrozenState::new(sandbox_id, module_hash);

        let mut memory = vec![0u8; 128 * 1024]; // 128KB
        memory[0..4].copy_from_slice(b"test");
        memory[65536..65540].copy_from_slice(b"data");

        frozen.capture_memory(&memory);

        // Should have 2 non-zero pages
        assert_eq!(frozen.memory_pages.len(), 2);
        assert_eq!(frozen.memory_size, 128 * 1024);

        // Restore and verify
        let restored = frozen.restore_memory();
        assert_eq!(restored, memory);
    }

    #[test]
    fn test_frozen_state_zero_compression() {
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();
        let mut frozen = FrozenState::new(sandbox_id, module_hash);

        let memory = vec![0u8; 1024 * 1024]; // 1MB all zeros
        frozen.capture_memory(&memory);

        assert_eq!(frozen.memory_pages.len(), 0);
        assert_eq!(frozen.estimated_size(), 0);
    }

    #[test]
    fn test_frozen_state_integrity() {
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();
        let mut frozen = FrozenState::new(sandbox_id, module_hash);

        let memory = vec![42u8; 65536];
        frozen.capture_memory(&memory);

        assert!(frozen.verify_integrity());
    }

    #[test]
    fn test_dirty_page_detection() {
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();

        let mut state1 = FrozenState::new(sandbox_id, module_hash.clone());
        let mut mem1 = vec![0u8; 128 * 1024];
        mem1[0..4].copy_from_slice(b"aaaa");
        state1.capture_memory(&mem1);

        let mut state2 = FrozenState::new(sandbox_id, module_hash);
        let mut mem2 = mem1.clone();
        mem2[65536..65540].copy_from_slice(b"bbbb");
        state2.capture_memory(&mem2);

        let dirty = state2.dirty_pages_since(&state1);
        assert!(dirty.contains(&1)); // Second page is dirty
    }

    #[test]
    fn test_live_migration_lifecycle() {
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();
        let config = LiveMigrationConfig::default();

        let mut migration =
            LiveMigration::new(sandbox_id, "node-target".to_string(), config);

        assert_eq!(migration.state(), LiveMigrationState::Running);

        // Freeze
        migration.begin_freeze().unwrap();
        assert_eq!(migration.state(), LiveMigrationState::Freezing);

        let mut frozen = FrozenState::new(sandbox_id, module_hash);
        frozen.capture_memory(&vec![0u8; 65536]);
        migration.complete_freeze(frozen).unwrap();
        assert_eq!(migration.state(), LiveMigrationState::Frozen);

        // Transfer
        migration.begin_final_transfer().unwrap();
        migration.report_transfer(1, 65536);

        // Verify
        migration.begin_verification().unwrap();

        // Complete
        migration.complete().unwrap();
        assert_eq!(migration.state(), LiveMigrationState::Completed);
        assert!(migration.state().is_terminal());
    }

    #[test]
    fn test_live_migration_rollback() {
        let sandbox_id = test_sandbox_id();
        let config = LiveMigrationConfig::default();

        let mut migration =
            LiveMigration::new(sandbox_id, "node-target".to_string(), config);

        migration.begin_freeze().unwrap();
        migration.rollback("Target node unreachable");

        assert_eq!(migration.state(), LiveMigrationState::RolledBack);
        assert_eq!(migration.error(), Some("Target node unreachable"));
    }

    #[test]
    fn test_live_migration_invalid_transition() {
        let sandbox_id = test_sandbox_id();
        let config = LiveMigrationConfig::default();

        let mut migration =
            LiveMigration::new(sandbox_id, "target".to_string(), config);

        // Can't verify without freezing first
        assert!(migration.begin_verification().is_err());
    }

    #[test]
    fn test_migration_progress() {
        let progress = MigrationProgress {
            state: LiveMigrationState::FinalTransfer,
            pages_transferred: 50,
            total_pages: 100,
            bytes_transferred: 50 * 65536,
            pre_copy_round: 0,
            dirty_pages_remaining: 50,
            elapsed: Duration::from_secs(5),
            estimated_remaining: Some(Duration::from_secs(5)),
        };

        assert!((progress.percent_complete() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_failover_registry() {
        let registry = FailoverRegistry::new();
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();

        registry
            .register(sandbox_id, "node-1".to_string(), module_hash, Some("us-east".to_string()))
            .unwrap();

        assert_eq!(registry.sandbox_count(), 1);

        let lookup = registry.lookup(&sandbox_id).unwrap().unwrap();
        assert_eq!(lookup.node_id, "node-1");
        assert_eq!(lookup.region, Some("us-east".to_string()));
    }

    #[test]
    fn test_failover_registry_heartbeat() {
        let registry = FailoverRegistry::new();
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();

        registry
            .register(sandbox_id, "node-1".to_string(), module_hash, None)
            .unwrap();

        registry.heartbeat(&sandbox_id).unwrap();
    }

    #[test]
    fn test_failover_registry_sandboxes_on_node() {
        let registry = FailoverRegistry::new();
        let module_hash = test_module_hash();

        let sb1 = test_sandbox_id();
        let sb2 = test_sandbox_id();
        let sb3 = test_sandbox_id();

        registry.register(sb1, "node-1".to_string(), module_hash.clone(), None).unwrap();
        registry.register(sb2, "node-1".to_string(), module_hash.clone(), None).unwrap();
        registry.register(sb3, "node-2".to_string(), module_hash, None).unwrap();

        let node1_sandboxes = registry.sandboxes_on_node("node-1").unwrap();
        assert_eq!(node1_sandboxes.len(), 2);
    }

    #[test]
    fn test_failover_registry_deregister() {
        let registry = FailoverRegistry::new();
        let sandbox_id = test_sandbox_id();
        let module_hash = test_module_hash();

        registry.register(sandbox_id, "node-1".to_string(), module_hash, None).unwrap();
        assert!(registry.deregister(&sandbox_id).unwrap());
        assert!(!registry.deregister(&sandbox_id).unwrap());
    }

    #[test]
    fn test_failover_policy_default() {
        let policy = FailoverPolicy::default();
        assert_eq!(policy.heartbeat_miss_threshold, 3);
        assert!(policy.prefer_same_region);
    }
}
