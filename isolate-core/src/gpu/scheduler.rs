//! Multi-sandbox GPU scheduler with VRAM quotas and compute budgets.
//!
//! Manages GPU resources across multiple sandboxes, providing fair scheduling,
//! VRAM quota enforcement, and batched inference for AI workloads.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// VRAM quota for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VramQuota {
    /// Maximum VRAM allocation in bytes.
    pub max_bytes: u64,
    /// Currently allocated VRAM.
    pub allocated_bytes: u64,
    /// Peak allocation observed.
    pub peak_bytes: u64,
}

impl VramQuota {
    /// Create a new VRAM quota.
    pub fn new(max_bytes: u64) -> Self {
        Self { max_bytes, allocated_bytes: 0, peak_bytes: 0 }
    }

    /// Try to allocate VRAM.
    pub fn allocate(&mut self, bytes: u64) -> Result<(), GpuSchedulerError> {
        if self.allocated_bytes + bytes > self.max_bytes {
            return Err(GpuSchedulerError::VramExceeded {
                requested: bytes,
                available: self.max_bytes - self.allocated_bytes,
                limit: self.max_bytes,
            });
        }
        self.allocated_bytes += bytes;
        self.peak_bytes = self.peak_bytes.max(self.allocated_bytes);
        Ok(())
    }

    /// Free VRAM.
    pub fn free(&mut self, bytes: u64) {
        self.allocated_bytes = self.allocated_bytes.saturating_sub(bytes);
    }

    /// Utilization ratio (0.0 to 1.0).
    pub fn utilization(&self) -> f64 {
        if self.max_bytes == 0 {
            return 0.0;
        }
        self.allocated_bytes as f64 / self.max_bytes as f64
    }

    /// Available VRAM.
    pub fn available(&self) -> u64 {
        self.max_bytes.saturating_sub(self.allocated_bytes)
    }
}

/// Compute budget for GPU operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeBudget {
    /// Maximum shader invocations per time window.
    pub max_invocations: u64,
    /// Time window for rate limiting.
    pub window: Duration,
    /// Current invocations in this window.
    pub current_invocations: u64,
    /// Window start time.
    #[serde(skip)]
    pub window_start: Option<Instant>,
    /// Maximum execution time per dispatch.
    pub max_dispatch_time: Duration,
}

impl ComputeBudget {
    /// Create a new compute budget.
    pub fn new(max_invocations: u64, window: Duration) -> Self {
        Self {
            max_invocations,
            window,
            current_invocations: 0,
            window_start: None,
            max_dispatch_time: Duration::from_secs(5),
        }
    }

    /// Try to consume invocations from the budget.
    pub fn try_consume(&mut self, invocations: u64) -> Result<(), GpuSchedulerError> {
        let now = Instant::now();

        // Reset window if expired
        if let Some(start) = self.window_start {
            if now.duration_since(start) >= self.window {
                self.current_invocations = 0;
                self.window_start = Some(now);
            }
        } else {
            self.window_start = Some(now);
        }

        if self.current_invocations + invocations > self.max_invocations {
            return Err(GpuSchedulerError::BudgetExhausted {
                requested: invocations,
                remaining: self.max_invocations - self.current_invocations,
            });
        }

        self.current_invocations += invocations;
        Ok(())
    }

    /// Remaining invocations in current window.
    pub fn remaining(&self) -> u64 {
        self.max_invocations.saturating_sub(self.current_invocations)
    }
}

/// Priority level for GPU tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GpuPriority {
    Background = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

/// A GPU compute task submitted by a sandbox.
#[derive(Debug, Clone)]
pub struct GpuTask {
    /// Task identifier.
    pub id: String,
    /// Sandbox that submitted the task.
    pub sandbox_id: String,
    /// Priority.
    pub priority: GpuPriority,
    /// Estimated VRAM needed.
    pub vram_required: u64,
    /// Number of invocations.
    pub invocations: u64,
    /// Task state.
    pub state: GpuTaskState,
    /// Submitted at.
    pub submitted_at: Instant,
    /// Completed at.
    pub completed_at: Option<Instant>,
}

impl GpuTask {
    /// Create a new GPU task.
    pub fn new(sandbox_id: &str, vram_required: u64, invocations: u64) -> Self {
        Self {
            id: format!("gpu-{}", uuid::Uuid::new_v4()),
            sandbox_id: sandbox_id.to_string(),
            priority: GpuPriority::Normal,
            vram_required,
            invocations,
            state: GpuTaskState::Queued,
            submitted_at: Instant::now(),
            completed_at: None,
        }
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: GpuPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Queue wait time.
    pub fn wait_time(&self) -> Duration {
        self.submitted_at.elapsed()
    }

    /// Execution time (if completed).
    pub fn execution_time(&self) -> Option<Duration> {
        self.completed_at.map(|c| c.duration_since(self.submitted_at))
    }
}

/// State of a GPU task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuTaskState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// GPU scheduler error.
#[derive(Debug, Clone)]
pub enum GpuSchedulerError {
    VramExceeded { requested: u64, available: u64, limit: u64 },
    BudgetExhausted { requested: u64, remaining: u64 },
    QueueFull { capacity: usize },
    SandboxNotRegistered(String),
    TaskNotFound(String),
}

impl std::fmt::Display for GpuSchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VramExceeded { requested, available, limit } => write!(
                f,
                "VRAM exceeded: requested {}B, available {}B, limit {}B",
                requested, available, limit
            ),
            Self::BudgetExhausted { requested, remaining } => {
                write!(f, "Compute budget exhausted: need {}, have {}", requested, remaining)
            }
            Self::QueueFull { capacity } => write!(f, "Queue full: capacity {}", capacity),
            Self::SandboxNotRegistered(id) => write!(f, "Sandbox not registered: {}", id),
            Self::TaskNotFound(id) => write!(f, "Task not found: {}", id),
        }
    }
}

impl std::error::Error for GpuSchedulerError {}

/// Scheduler for GPU resources across multiple sandboxes.
pub struct GpuScheduler {
    /// Per-sandbox VRAM quotas.
    quotas: HashMap<String, VramQuota>,
    /// Per-sandbox compute budgets.
    budgets: HashMap<String, ComputeBudget>,
    /// Task queue.
    queue: VecDeque<GpuTask>,
    /// Active tasks.
    active: HashMap<String, GpuTask>,
    /// Completed tasks (recent).
    completed: VecDeque<GpuTask>,
    /// Configuration.
    config: SchedulerConfig,
    /// Statistics.
    stats: SchedulerStats,
}

/// Configuration for the GPU scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Total VRAM available.
    pub total_vram: u64,
    /// Default per-sandbox VRAM quota.
    pub default_vram_quota: u64,
    /// Default compute budget (invocations per window).
    pub default_compute_budget: u64,
    /// Compute budget window.
    pub budget_window: Duration,
    /// Maximum queue size.
    pub max_queue_size: usize,
    /// Maximum concurrent tasks.
    pub max_concurrent: usize,
    /// Maximum completed tasks to retain.
    pub max_completed_history: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            total_vram: 8 * 1024 * 1024 * 1024,    // 8 GB
            default_vram_quota: 512 * 1024 * 1024, // 512 MB
            default_compute_budget: 100_000,
            budget_window: Duration::from_secs(60),
            max_queue_size: 1000,
            max_concurrent: 8,
            max_completed_history: 100,
        }
    }
}

/// Scheduler statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulerStats {
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub total_vram_allocated: u64,
    pub total_invocations: u64,
    pub queue_depth: usize,
    pub active_tasks: usize,
}

impl GpuScheduler {
    /// Create a new GPU scheduler.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            quotas: HashMap::new(),
            budgets: HashMap::new(),
            queue: VecDeque::new(),
            active: HashMap::new(),
            completed: VecDeque::new(),
            config,
            stats: SchedulerStats::default(),
        }
    }

    /// Register a sandbox for GPU access.
    pub fn register_sandbox(&mut self, sandbox_id: &str) {
        self.quotas
            .entry(sandbox_id.to_string())
            .or_insert_with(|| VramQuota::new(self.config.default_vram_quota));
        self.budgets.entry(sandbox_id.to_string()).or_insert_with(|| {
            ComputeBudget::new(self.config.default_compute_budget, self.config.budget_window)
        });
    }

    /// Register with custom quota.
    pub fn register_sandbox_with_quota(&mut self, sandbox_id: &str, vram_quota: u64) {
        self.quotas.insert(sandbox_id.to_string(), VramQuota::new(vram_quota));
        self.budgets.entry(sandbox_id.to_string()).or_insert_with(|| {
            ComputeBudget::new(self.config.default_compute_budget, self.config.budget_window)
        });
    }

    /// Unregister a sandbox.
    pub fn unregister_sandbox(&mut self, sandbox_id: &str) {
        self.quotas.remove(sandbox_id);
        self.budgets.remove(sandbox_id);
    }

    /// Submit a GPU task.
    pub fn submit(&mut self, task: GpuTask) -> Result<String, GpuSchedulerError> {
        // Validate sandbox is registered
        if !self.quotas.contains_key(&task.sandbox_id) {
            return Err(GpuSchedulerError::SandboxNotRegistered(task.sandbox_id.clone()));
        }

        // Check queue capacity
        if self.queue.len() >= self.config.max_queue_size {
            return Err(GpuSchedulerError::QueueFull { capacity: self.config.max_queue_size });
        }

        let task_id = task.id.clone();
        self.queue.push_back(task);
        self.stats.tasks_submitted += 1;
        self.stats.queue_depth = self.queue.len();

        Ok(task_id)
    }

    /// Schedule pending tasks (called periodically).
    pub fn schedule(&mut self) -> Vec<String> {
        let mut started = Vec::new();

        while self.active.len() < self.config.max_concurrent {
            // Find highest priority task that fits
            let task_idx = self.find_schedulable_task();
            if let Some(idx) = task_idx {
                let mut task = self.queue.remove(idx).unwrap();

                // Allocate resources
                if let Some(quota) = self.quotas.get_mut(&task.sandbox_id) {
                    if quota.allocate(task.vram_required).is_err() {
                        // Put back in queue
                        self.queue.push_front(task);
                        break;
                    }
                }

                if let Some(budget) = self.budgets.get_mut(&task.sandbox_id) {
                    if budget.try_consume(task.invocations).is_err() {
                        // Free VRAM and put back
                        if let Some(quota) = self.quotas.get_mut(&task.sandbox_id) {
                            quota.free(task.vram_required);
                        }
                        self.queue.push_front(task);
                        break;
                    }
                }

                task.state = GpuTaskState::Running;
                started.push(task.id.clone());
                self.active.insert(task.id.clone(), task);
            } else {
                break;
            }
        }

        self.stats.queue_depth = self.queue.len();
        self.stats.active_tasks = self.active.len();
        started
    }

    fn find_schedulable_task(&self) -> Option<usize> {
        // Priority-first scheduling
        let mut best_idx = None;
        let mut best_priority = None;

        for (idx, task) in self.queue.iter().enumerate() {
            let has_vram = self
                .quotas
                .get(&task.sandbox_id)
                .map(|q| q.available() >= task.vram_required)
                .unwrap_or(false);

            if has_vram {
                if best_priority.is_none() || task.priority > best_priority.unwrap() {
                    best_idx = Some(idx);
                    best_priority = Some(task.priority);
                }
            }
        }

        best_idx
    }

    /// Mark a task as completed.
    pub fn complete_task(&mut self, task_id: &str) -> Result<(), GpuSchedulerError> {
        let mut task = self
            .active
            .remove(task_id)
            .ok_or_else(|| GpuSchedulerError::TaskNotFound(task_id.to_string()))?;

        // Free VRAM
        if let Some(quota) = self.quotas.get_mut(&task.sandbox_id) {
            quota.free(task.vram_required);
        }

        task.state = GpuTaskState::Completed;
        task.completed_at = Some(Instant::now());
        self.stats.tasks_completed += 1;
        self.stats.total_invocations += task.invocations;
        self.stats.total_vram_allocated += task.vram_required;

        if self.completed.len() >= self.config.max_completed_history {
            self.completed.pop_front();
        }
        self.completed.push_back(task);

        self.stats.active_tasks = self.active.len();
        Ok(())
    }

    /// Fail a task.
    pub fn fail_task(&mut self, task_id: &str, _reason: &str) -> Result<(), GpuSchedulerError> {
        let mut task = self
            .active
            .remove(task_id)
            .ok_or_else(|| GpuSchedulerError::TaskNotFound(task_id.to_string()))?;

        if let Some(quota) = self.quotas.get_mut(&task.sandbox_id) {
            quota.free(task.vram_required);
        }

        task.state = GpuTaskState::Failed;
        task.completed_at = Some(Instant::now());
        self.stats.tasks_failed += 1;

        self.completed.push_back(task);
        self.stats.active_tasks = self.active.len();
        Ok(())
    }

    /// Get scheduler statistics.
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    /// Get VRAM quota for a sandbox.
    pub fn quota(&self, sandbox_id: &str) -> Option<&VramQuota> {
        self.quotas.get(sandbox_id)
    }

    /// Get queue depth.
    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }

    /// Get active task count.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }
}

impl Default for GpuScheduler {
    fn default() -> Self {
        Self::new(SchedulerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vram_quota() {
        let mut quota = VramQuota::new(1024);

        assert!(quota.allocate(512).is_ok());
        assert_eq!(quota.allocated_bytes, 512);
        assert_eq!(quota.available(), 512);

        assert!(quota.allocate(600).is_err());
        assert!(quota.allocate(512).is_ok());

        quota.free(1024);
        assert_eq!(quota.allocated_bytes, 0);
        assert_eq!(quota.peak_bytes, 1024);
    }

    #[test]
    fn test_compute_budget() {
        let mut budget = ComputeBudget::new(100, Duration::from_secs(60));

        assert!(budget.try_consume(50).is_ok());
        assert_eq!(budget.remaining(), 50);

        assert!(budget.try_consume(60).is_err());
        assert!(budget.try_consume(50).is_ok());
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn test_gpu_scheduler_basic() {
        let mut scheduler =
            GpuScheduler::new(SchedulerConfig { max_concurrent: 2, ..Default::default() });

        scheduler.register_sandbox("sb-1");

        let task = GpuTask::new("sb-1", 1024, 100);
        let task_id = scheduler.submit(task).unwrap();

        let started = scheduler.schedule();
        assert_eq!(started.len(), 1);

        scheduler.complete_task(&task_id).unwrap();
        assert_eq!(scheduler.stats().tasks_completed, 1);
    }

    #[test]
    fn test_gpu_scheduler_priority() {
        let mut scheduler =
            GpuScheduler::new(SchedulerConfig { max_concurrent: 1, ..Default::default() });

        scheduler.register_sandbox("sb-1");

        // Submit low priority first
        let low = GpuTask::new("sb-1", 1024, 10).with_priority(GpuPriority::Background);
        scheduler.submit(low).unwrap();

        // Then high priority
        let high = GpuTask::new("sb-1", 1024, 10).with_priority(GpuPriority::Realtime);
        let high_id = scheduler.submit(high).unwrap();

        let started = scheduler.schedule();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0], high_id);
    }

    #[test]
    fn test_gpu_scheduler_unregistered() {
        let mut scheduler = GpuScheduler::default();

        let task = GpuTask::new("unknown", 1024, 10);
        assert!(matches!(scheduler.submit(task), Err(GpuSchedulerError::SandboxNotRegistered(_))));
    }

    #[test]
    fn test_gpu_scheduler_vram_limit() {
        let mut scheduler =
            GpuScheduler::new(SchedulerConfig { max_concurrent: 10, ..Default::default() });

        scheduler.register_sandbox_with_quota("sb-1", 1024);

        let t1 = GpuTask::new("sb-1", 512, 10);
        let t2 = GpuTask::new("sb-1", 512, 10);
        let t3 = GpuTask::new("sb-1", 512, 10);

        scheduler.submit(t1).unwrap();
        scheduler.submit(t2).unwrap();
        scheduler.submit(t3).unwrap();

        // Only first two should fit in VRAM
        let started = scheduler.schedule();
        assert_eq!(started.len(), 2);
    }

    #[test]
    fn test_gpu_task_lifecycle() {
        let task = GpuTask::new("sb-1", 1024, 100);
        assert_eq!(task.state, GpuTaskState::Queued);
        assert!(task.execution_time().is_none());
    }
}
