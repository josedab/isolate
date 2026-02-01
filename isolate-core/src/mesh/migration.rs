//! Sandbox migration between mesh nodes.

use super::NodeId;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// State of a migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MigrationState {
    /// Migration is queued.
    Pending,
    /// Pre-migration checks in progress.
    Preparing,
    /// Transferring sandbox state.
    Transferring,
    /// Verifying transfer.
    Verifying,
    /// Completing migration.
    Completing,
    /// Migration completed successfully.
    Completed,
    /// Migration failed.
    Failed,
    /// Migration was cancelled.
    Cancelled,
}

impl MigrationState {
    /// Check if migration is in progress.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            MigrationState::Pending
                | MigrationState::Preparing
                | MigrationState::Transferring
                | MigrationState::Verifying
                | MigrationState::Completing
        )
    }

    /// Check if migration is terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            MigrationState::Completed | MigrationState::Failed | MigrationState::Cancelled
        )
    }
}

/// A migration plan for moving sandboxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    /// Plan identifier.
    pub id: String,
    /// Sandboxes to migrate.
    pub migrations: Vec<MigrationTask>,
    /// Created timestamp.
    #[serde(skip)]
    pub created_at: Option<Instant>,
    /// Reason for migration.
    pub reason: MigrationReason,
    /// Priority level.
    pub priority: MigrationPriority,
}

impl MigrationPlan {
    /// Create a new migration plan.
    pub fn new(reason: MigrationReason) -> Self {
        Self {
            id: format!("plan-{}", uuid_v4()),
            migrations: Vec::new(),
            created_at: Some(Instant::now()),
            reason,
            priority: MigrationPriority::Normal,
        }
    }

    /// Add a migration task.
    pub fn add(&mut self, task: MigrationTask) {
        self.migrations.push(task);
    }

    /// Get total number of tasks.
    pub fn task_count(&self) -> usize {
        self.migrations.len()
    }

    /// Get completed tasks.
    pub fn completed_count(&self) -> usize {
        self.migrations.iter().filter(|t| t.state == MigrationState::Completed).count()
    }

    /// Check if plan is complete.
    pub fn is_complete(&self) -> bool {
        self.migrations.iter().all(|t| t.state.is_terminal())
    }
}

/// Reason for migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationReason {
    /// Node is leaving the cluster.
    NodeLeaving,
    /// Node failed.
    NodeFailed,
    /// Rebalancing workload.
    Rebalance,
    /// User-initiated.
    Manual,
    /// Resource pressure.
    ResourcePressure,
    /// Maintenance.
    Maintenance,
}

/// Priority level for migrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MigrationPriority {
    /// Low priority - can be delayed.
    Low = 0,
    /// Normal priority.
    Normal = 1,
    /// High priority - should be done soon.
    High = 2,
    /// Urgent - do immediately.
    Urgent = 3,
}

/// A single migration task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationTask {
    /// Task identifier.
    pub id: String,
    /// Sandbox ID being migrated.
    pub sandbox_id: String,
    /// Source node.
    pub source: NodeId,
    /// Destination node.
    pub destination: NodeId,
    /// Current state.
    pub state: MigrationState,
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Error message if failed.
    pub error: Option<String>,
    /// Started timestamp.
    #[serde(skip)]
    pub started_at: Option<Instant>,
    /// Completed timestamp.
    #[serde(skip)]
    pub completed_at: Option<Instant>,
}

impl MigrationTask {
    /// Create a new migration task.
    pub fn new(sandbox_id: String, source: NodeId, destination: NodeId) -> Self {
        Self {
            id: format!("mig-{}", uuid_v4()),
            sandbox_id,
            source,
            destination,
            state: MigrationState::Pending,
            bytes_transferred: 0,
            total_bytes: 0,
            error: None,
            started_at: None,
            completed_at: None,
        }
    }

    /// Transition to a new state.
    pub fn transition(&mut self, new_state: MigrationState) {
        self.state = new_state;
        if new_state == MigrationState::Transferring {
            self.started_at = Some(Instant::now());
        }
        if new_state.is_terminal() {
            self.completed_at = Some(Instant::now());
        }
    }

    /// Mark as failed with error.
    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.transition(MigrationState::Failed);
    }

    /// Get progress percentage.
    pub fn progress(&self) -> f64 {
        if self.total_bytes == 0 {
            match self.state {
                MigrationState::Pending => 0.0,
                MigrationState::Preparing => 0.1,
                MigrationState::Transferring => 0.5,
                MigrationState::Verifying => 0.9,
                MigrationState::Completing => 0.95,
                MigrationState::Completed => 1.0,
                _ => 0.0,
            }
        } else {
            self.bytes_transferred as f64 / self.total_bytes as f64
        }
    }

    /// Get duration if completed.
    pub fn duration(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }
}

/// Manages sandbox migrations.
pub struct MigrationManager {
    /// Maximum concurrent migrations.
    max_concurrent: usize,
    /// Active migrations.
    active: Arc<RwLock<HashMap<String, MigrationTask>>>,
    /// Pending migrations.
    pending: Arc<RwLock<VecDeque<MigrationTask>>>,
    /// Completed migrations.
    completed: Arc<RwLock<Vec<MigrationTask>>>,
    /// Migration plans.
    plans: Arc<RwLock<HashMap<String, MigrationPlan>>>,
}

impl MigrationManager {
    /// Create a new migration manager.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            active: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(RwLock::new(VecDeque::new())),
            completed: Arc::new(RwLock::new(Vec::new())),
            plans: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Queue a migration task.
    pub fn queue(&self, task: MigrationTask) -> Result<()> {
        let mut pending =
            self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        pending.push_back(task);
        Ok(())
    }

    /// Queue a migration plan.
    pub fn queue_plan(&self, plan: MigrationPlan) -> Result<()> {
        let plan_id = plan.id.clone();

        // Queue all tasks
        {
            let mut pending =
                self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            for task in &plan.migrations {
                pending.push_back(task.clone());
            }
        }

        // Store the plan
        {
            let mut plans =
                self.plans.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            plans.insert(plan_id, plan);
        }

        Ok(())
    }

    /// Start pending migrations up to the limit.
    pub fn start_pending(&self) -> Result<Vec<MigrationTask>> {
        let active_count =
            self.active.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?.len();

        if active_count >= self.max_concurrent {
            return Ok(Vec::new());
        }

        let slots = self.max_concurrent - active_count;
        let mut started = Vec::new();

        {
            let mut pending =
                self.pending.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            let mut active =
                self.active.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

            for _ in 0..slots {
                if let Some(mut task) = pending.pop_front() {
                    task.transition(MigrationState::Preparing);
                    let task_id = task.id.clone();
                    started.push(task.clone());
                    active.insert(task_id, task);
                } else {
                    break;
                }
            }
        }

        Ok(started)
    }

    /// Update a migration's progress.
    pub fn update_progress(
        &self,
        task_id: &str,
        bytes_transferred: u64,
        total_bytes: u64,
    ) -> Result<()> {
        let mut active =
            self.active.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        if let Some(task) = active.get_mut(task_id) {
            task.bytes_transferred = bytes_transferred;
            task.total_bytes = total_bytes;
            if task.state == MigrationState::Preparing {
                task.transition(MigrationState::Transferring);
            }
        }

        Ok(())
    }

    /// Complete a migration.
    pub fn complete(&self, task_id: &str) -> Result<()> {
        let task = {
            let mut active =
                self.active.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            active.remove(task_id)
        };

        if let Some(mut task) = task {
            task.transition(MigrationState::Completed);
            let mut completed =
                self.completed.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            completed.push(task);
        }

        Ok(())
    }

    /// Fail a migration.
    pub fn fail(&self, task_id: &str, error: String) -> Result<()> {
        let task = {
            let mut active =
                self.active.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            active.remove(task_id)
        };

        if let Some(mut task) = task {
            task.fail(error);
            let mut completed =
                self.completed.write().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            completed.push(task);
        }

        Ok(())
    }

    /// Get active migrations.
    pub fn active_migrations(&self) -> Result<Vec<MigrationTask>> {
        let active = self.active.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(active.values().cloned().collect())
    }

    /// Get pending count.
    pub fn pending_count(&self) -> Result<usize> {
        let pending =
            self.pending.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(pending.len())
    }

    /// Get statistics.
    pub fn stats(&self) -> Result<MigrationStats> {
        let active = self.active.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        let pending =
            self.pending.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        let completed =
            self.completed.read().map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        let successful = completed.iter().filter(|t| t.state == MigrationState::Completed).count();
        let failed = completed.iter().filter(|t| t.state == MigrationState::Failed).count();

        Ok(MigrationStats {
            active: active.len(),
            pending: pending.len(),
            completed: successful,
            failed,
            total_bytes_transferred: completed.iter().map(|t| t.bytes_transferred).sum(),
        })
    }
}

/// Migration statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MigrationStats {
    /// Active migrations.
    pub active: usize,
    /// Pending migrations.
    pub pending: usize,
    /// Completed migrations.
    pub completed: usize,
    /// Failed migrations.
    pub failed: usize,
    /// Total bytes transferred.
    pub total_bytes_transferred: u64,
}

/// Generate a simple UUID-like string.
fn uuid_v4() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_state() {
        assert!(MigrationState::Pending.is_active());
        assert!(!MigrationState::Completed.is_active());
        assert!(MigrationState::Completed.is_terminal());
        assert!(MigrationState::Failed.is_terminal());
    }

    #[test]
    fn test_migration_task() {
        let mut task = MigrationTask::new("sandbox-1".to_string(), NodeId::new(1), NodeId::new(2));

        assert_eq!(task.state, MigrationState::Pending);
        assert_eq!(task.progress(), 0.0);

        task.transition(MigrationState::Completed);
        assert_eq!(task.progress(), 1.0);
    }

    #[test]
    fn test_migration_plan() {
        let mut plan = MigrationPlan::new(MigrationReason::Rebalance);

        plan.add(MigrationTask::new("sandbox-1".to_string(), NodeId::new(1), NodeId::new(2)));
        plan.add(MigrationTask::new("sandbox-2".to_string(), NodeId::new(1), NodeId::new(3)));

        assert_eq!(plan.task_count(), 2);
        assert_eq!(plan.completed_count(), 0);
        assert!(!plan.is_complete());
    }

    #[test]
    fn test_migration_manager() {
        let manager = MigrationManager::new(2);

        let task = MigrationTask::new("sandbox-1".to_string(), NodeId::new(1), NodeId::new(2));

        manager.queue(task).unwrap();
        assert_eq!(manager.pending_count().unwrap(), 1);

        let started = manager.start_pending().unwrap();
        assert_eq!(started.len(), 1);
        assert_eq!(manager.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_migration_manager_max_concurrent() {
        let manager = MigrationManager::new(1);

        manager
            .queue(MigrationTask::new("sb-1".to_string(), NodeId::new(1), NodeId::new(2)))
            .unwrap();
        manager
            .queue(MigrationTask::new("sb-2".to_string(), NodeId::new(1), NodeId::new(3)))
            .unwrap();

        let started = manager.start_pending().unwrap();
        assert_eq!(started.len(), 1);

        // Can't start more until current completes
        let started2 = manager.start_pending().unwrap();
        assert_eq!(started2.len(), 0);
    }
}
