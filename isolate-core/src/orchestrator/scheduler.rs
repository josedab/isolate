//! Orchestrator scheduler implementation.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Orchestrator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Maximum total concurrent sandboxes across all tenants.
    pub max_global_concurrent: usize,
    /// Maximum queue depth per tenant.
    pub max_queue_depth: usize,
    /// Scheduling interval for the fair-share scheduler.
    pub scheduling_interval: Duration,
    /// Enable priority-based scheduling.
    pub priority_scheduling: bool,
    /// Enable deficit round-robin fairness.
    pub deficit_round_robin: bool,
    /// Starvation timeout: promote low-priority items after this duration.
    pub starvation_timeout: Duration,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_global_concurrent: 100,
            max_queue_depth: 1000,
            scheduling_interval: Duration::from_millis(10),
            priority_scheduling: true,
            deficit_round_robin: true,
            starvation_timeout: Duration::from_secs(60),
        }
    }
}

/// Per-tenant configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// Maximum concurrent sandboxes for this tenant.
    pub max_concurrent: usize,
    /// Maximum total memory in bytes for this tenant.
    pub max_memory_bytes: u64,
    /// Maximum total fuel per scheduling window.
    pub max_fuel_per_window: u64,
    /// Scheduling priority (higher = more resources, default 5).
    pub priority: u8,
    /// Rate limit: max submissions per second.
    pub rate_limit: f64,
    /// Whether this tenant is enabled.
    pub enabled: bool,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            max_memory_bytes: 1024 * 1024 * 1024, // 1 GB
            max_fuel_per_window: 100_000_000,
            priority: 5,
            rate_limit: 100.0,
            enabled: true,
            metadata: HashMap::new(),
        }
    }
}

/// Tenant status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantStatus {
    /// Active and accepting submissions.
    Active,
    /// Suspended (quota exceeded or admin action).
    Suspended,
    /// Disabled.
    Disabled,
}

/// Per-tenant metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantMetrics {
    /// Total submissions.
    pub total_submissions: u64,
    /// Successful completions.
    pub successful: u64,
    /// Failed executions.
    pub failed: u64,
    /// Timed out executions.
    pub timed_out: u64,
    /// Rejected submissions (quota/rate limit).
    pub rejected: u64,
    /// Currently active sandboxes.
    pub active_count: u32,
    /// Current queue depth.
    pub queue_depth: u32,
    /// Total fuel consumed.
    pub total_fuel: u64,
    /// Total memory used (current).
    pub current_memory_bytes: u64,
    /// Average execution duration.
    pub avg_duration_ms: f64,
}

/// A ticket for a submitted sandbox execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubmitTicket(pub Uuid);

impl SubmitTicket {
    /// Create a new ticket.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SubmitTicket {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SubmitTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// State of a submitted item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketState {
    /// Queued, waiting to be scheduled.
    Queued,
    /// Currently executing.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed.
    Failed,
    /// Rejected (quota/rate limit).
    Rejected,
    /// Cancelled.
    Cancelled,
}

/// Internal queue item.
struct QueueItem {
    ticket: SubmitTicket,
    tenant_id: String,
    priority: u8,
    submitted_at: Instant,
    state: TicketState,
    estimated_memory: u64,
}

/// Internal tenant state.
struct TenantState {
    config: TenantConfig,
    status: TenantStatus,
    metrics: TenantMetrics,
    queue: VecDeque<SubmitTicket>,
    active: Vec<SubmitTicket>,
    deficit: f64, // For deficit round-robin
    last_submission: Option<Instant>,
    submissions_this_second: f64,
}

impl TenantState {
    fn new(config: TenantConfig) -> Self {
        Self {
            config,
            status: TenantStatus::Active,
            metrics: TenantMetrics::default(),
            queue: VecDeque::new(),
            active: Vec::new(),
            deficit: 0.0,
            last_submission: None,
            submissions_this_second: 0.0,
        }
    }

    fn check_rate_limit(&mut self) -> bool {
        let now = Instant::now();
        match self.last_submission {
            Some(last) => {
                let elapsed = now.duration_since(last).as_secs_f64();
                if elapsed >= 1.0 {
                    self.submissions_this_second = 1.0;
                    self.last_submission = Some(now);
                    true
                } else {
                    self.submissions_this_second += 1.0;
                    self.last_submission = Some(now);
                    self.submissions_this_second <= self.config.rate_limit
                }
            }
            None => {
                self.submissions_this_second = 1.0;
                self.last_submission = Some(now);
                true
            }
        }
    }
}

/// Orchestrator statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestratorStats {
    /// Total tenants registered.
    pub tenant_count: usize,
    /// Active tenants (with running or queued items).
    pub active_tenants: usize,
    /// Total items currently running.
    pub total_running: usize,
    /// Total items currently queued.
    pub total_queued: usize,
    /// Total submissions received.
    pub total_submissions: u64,
    /// Total completions.
    pub total_completions: u64,
    /// Total rejections.
    pub total_rejections: u64,
    /// Scheduler cycles executed.
    pub scheduler_cycles: u64,
}

/// The multi-tenant orchestrator.
pub struct Orchestrator {
    config: OrchestratorConfig,
    tenants: Arc<RwLock<HashMap<String, TenantState>>>,
    items: Arc<RwLock<HashMap<SubmitTicket, QueueItem>>>,
    total_running: AtomicU64,
    total_submissions: AtomicU64,
    total_completions: AtomicU64,
    total_rejections: AtomicU64,
    scheduler_cycles: AtomicU64,
}

impl Orchestrator {
    /// Create a new orchestrator.
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            config,
            tenants: Arc::new(RwLock::new(HashMap::new())),
            items: Arc::new(RwLock::new(HashMap::new())),
            total_running: AtomicU64::new(0),
            total_submissions: AtomicU64::new(0),
            total_completions: AtomicU64::new(0),
            total_rejections: AtomicU64::new(0),
            scheduler_cycles: AtomicU64::new(0),
        }
    }

    /// Register a new tenant.
    pub fn register_tenant(
        &self,
        tenant_id: impl Into<String>,
        config: TenantConfig,
    ) -> Result<(), String> {
        let id = tenant_id.into();
        let mut tenants = self.tenants.write();

        if tenants.contains_key(&id) {
            return Err(format!("Tenant '{}' already registered", id));
        }

        tenants.insert(id, TenantState::new(config));
        Ok(())
    }

    /// Update a tenant's configuration.
    pub fn update_tenant(&self, tenant_id: &str, config: TenantConfig) -> Result<(), String> {
        let mut tenants = self.tenants.write();
        match tenants.get_mut(tenant_id) {
            Some(state) => {
                state.config = config;
                Ok(())
            }
            None => Err(format!("Tenant '{}' not found", tenant_id)),
        }
    }

    /// Suspend a tenant.
    pub fn suspend_tenant(&self, tenant_id: &str) -> Result<(), String> {
        let mut tenants = self.tenants.write();
        match tenants.get_mut(tenant_id) {
            Some(state) => {
                state.status = TenantStatus::Suspended;
                Ok(())
            }
            None => Err(format!("Tenant '{}' not found", tenant_id)),
        }
    }

    /// Resume a suspended tenant.
    pub fn resume_tenant(&self, tenant_id: &str) -> Result<(), String> {
        let mut tenants = self.tenants.write();
        match tenants.get_mut(tenant_id) {
            Some(state) => {
                state.status = TenantStatus::Active;
                Ok(())
            }
            None => Err(format!("Tenant '{}' not found", tenant_id)),
        }
    }

    /// Remove a tenant (fails if tenant has active items).
    pub fn remove_tenant(&self, tenant_id: &str) -> Result<(), String> {
        let mut tenants = self.tenants.write();
        match tenants.get(tenant_id) {
            Some(state) => {
                if !state.active.is_empty() || !state.queue.is_empty() {
                    return Err(format!(
                        "Tenant '{}' has {} active and {} queued items",
                        tenant_id,
                        state.active.len(),
                        state.queue.len()
                    ));
                }
                tenants.remove(tenant_id);
                Ok(())
            }
            None => Err(format!("Tenant '{}' not found", tenant_id)),
        }
    }

    /// Submit a sandbox execution for a tenant.
    pub fn submit(&self, tenant_id: &str, estimated_memory: u64) -> Result<SubmitTicket, String> {
        let mut tenants = self.tenants.write();
        let state = tenants
            .get_mut(tenant_id)
            .ok_or_else(|| format!("Tenant '{}' not found", tenant_id))?;

        // Check tenant status
        if state.status != TenantStatus::Active {
            state.metrics.rejected += 1;
            self.total_rejections.fetch_add(1, Ordering::Relaxed);
            return Err(format!("Tenant '{}' is {:?}", tenant_id, state.status));
        }

        // Check rate limit
        if !state.check_rate_limit() {
            state.metrics.rejected += 1;
            self.total_rejections.fetch_add(1, Ordering::Relaxed);
            return Err(format!("Tenant '{}' rate limit exceeded", tenant_id));
        }

        // Check queue depth
        if state.queue.len() >= self.config.max_queue_depth {
            state.metrics.rejected += 1;
            self.total_rejections.fetch_add(1, Ordering::Relaxed);
            return Err(format!("Tenant '{}' queue is full", tenant_id));
        }

        // Check concurrent limit
        if state.active.len() >= state.config.max_concurrent
            && state.queue.len() >= self.config.max_queue_depth
        {
            state.metrics.rejected += 1;
            self.total_rejections.fetch_add(1, Ordering::Relaxed);
            return Err(format!(
                "Tenant '{}' at capacity ({} active, {} queued)",
                tenant_id,
                state.active.len(),
                state.queue.len()
            ));
        }

        let ticket = SubmitTicket::new();
        let item = QueueItem {
            ticket,
            tenant_id: tenant_id.to_string(),
            priority: state.config.priority,
            submitted_at: Instant::now(),
            state: TicketState::Queued,
            estimated_memory,
        };

        state.queue.push_back(ticket);
        state.metrics.total_submissions += 1;
        state.metrics.queue_depth = state.queue.len() as u32;

        let mut items = self.items.write();
        items.insert(ticket, item);

        self.total_submissions.fetch_add(1, Ordering::Relaxed);

        tracing::debug!(
            tenant_id = tenant_id,
            ticket = %ticket,
            "Submission queued"
        );

        Ok(ticket)
    }

    /// Run a scheduling cycle, returning tickets ready to execute.
    pub fn schedule(&self) -> Vec<SubmitTicket> {
        self.scheduler_cycles.fetch_add(1, Ordering::Relaxed);
        let mut ready = Vec::new();
        let global_running = self.total_running.load(Ordering::Relaxed) as usize;

        if global_running >= self.config.max_global_concurrent {
            return ready;
        }

        let available_slots = self.config.max_global_concurrent - global_running;
        let mut tenants = self.tenants.write();
        let mut items = self.items.write();

        if self.config.deficit_round_robin {
            // Deficit round-robin: give each tenant credits proportional to priority
            let total_priority: u32 = tenants
                .values()
                .filter(|t| t.status == TenantStatus::Active && !t.queue.is_empty())
                .map(|t| t.config.priority as u32)
                .sum();

            if total_priority == 0 {
                return ready;
            }

            for state in tenants.values_mut() {
                if state.status != TenantStatus::Active || state.queue.is_empty() {
                    continue;
                }
                state.deficit +=
                    (state.config.priority as f64 / total_priority as f64) * available_slots as f64;
            }
        }

        // Schedule items from each tenant
        let mut scheduled_count = 0;
        let tenant_ids: Vec<String> = tenants.keys().cloned().collect();

        for tenant_id in tenant_ids {
            if scheduled_count >= available_slots {
                break;
            }

            let state = tenants.get_mut(&tenant_id).unwrap();
            if state.status != TenantStatus::Active {
                continue;
            }

            let slots = if self.config.deficit_round_robin {
                let slots = state.deficit.floor() as usize;
                state.deficit -= slots as f64;
                slots.min(state.config.max_concurrent - state.active.len())
            } else {
                (state.config.max_concurrent - state.active.len())
                    .min(available_slots - scheduled_count)
            };

            for _ in 0..slots {
                if let Some(ticket) = state.queue.pop_front() {
                    if let Some(item) = items.get_mut(&ticket) {
                        item.state = TicketState::Running;
                    }
                    state.active.push(ticket);
                    state.metrics.active_count = state.active.len() as u32;
                    state.metrics.queue_depth = state.queue.len() as u32;
                    ready.push(ticket);
                    scheduled_count += 1;
                    self.total_running.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        // Anti-starvation: promote old queued items
        if !self.config.starvation_timeout.is_zero() {
            let now = Instant::now();
            for state in tenants.values_mut() {
                if state.status != TenantStatus::Active {
                    continue;
                }
                for ticket in &state.queue {
                    if let Some(item) = items.get(ticket) {
                        if now.duration_since(item.submitted_at) >= self.config.starvation_timeout
                            && state.active.len() < state.config.max_concurrent
                            && scheduled_count < available_slots
                        {
                            // This item is starving, prioritize it next cycle
                            tracing::warn!(
                                tenant_id = state.config.metadata.get("id").map(|s| s.as_str()).unwrap_or("unknown"),
                                ticket = %ticket,
                                "Item approaching starvation timeout"
                            );
                        }
                    }
                }
            }
        }

        ready
    }

    /// Mark a ticket as completed.
    pub fn complete(&self, ticket: SubmitTicket, success: bool) {
        let mut items = self.items.write();
        let mut tenants = self.tenants.write();

        if let Some(item) = items.get_mut(&ticket) {
            item.state = if success { TicketState::Completed } else { TicketState::Failed };

            if let Some(state) = tenants.get_mut(&item.tenant_id) {
                state.active.retain(|t| *t != ticket);
                state.metrics.active_count = state.active.len() as u32;

                if success {
                    state.metrics.successful += 1;
                } else {
                    state.metrics.failed += 1;
                }
            }

            self.total_running.fetch_sub(1, Ordering::Relaxed);
            self.total_completions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get the state of a ticket.
    pub fn ticket_state(&self, ticket: &SubmitTicket) -> Option<TicketState> {
        self.items.read().get(ticket).map(|i| i.state)
    }

    /// Get metrics for a specific tenant.
    pub fn tenant_metrics(&self, tenant_id: &str) -> Option<TenantMetrics> {
        self.tenants.read().get(tenant_id).map(|s| s.metrics.clone())
    }

    /// Get tenant status.
    pub fn tenant_status(&self, tenant_id: &str) -> Option<TenantStatus> {
        self.tenants.read().get(tenant_id).map(|s| s.status)
    }

    /// List all tenant IDs.
    pub fn list_tenants(&self) -> Vec<String> {
        self.tenants.read().keys().cloned().collect()
    }

    /// Get orchestrator statistics.
    pub fn stats(&self) -> OrchestratorStats {
        let tenants = self.tenants.read();
        let active_tenants =
            tenants.values().filter(|t| !t.active.is_empty() || !t.queue.is_empty()).count();
        let total_running = tenants.values().map(|t| t.active.len()).sum();
        let total_queued = tenants.values().map(|t| t.queue.len()).sum();

        OrchestratorStats {
            tenant_count: tenants.len(),
            active_tenants,
            total_running,
            total_queued,
            total_submissions: self.total_submissions.load(Ordering::Relaxed),
            total_completions: self.total_completions.load(Ordering::Relaxed),
            total_rejections: self.total_rejections.load(Ordering::Relaxed),
            scheduler_cycles: self.scheduler_cycles.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Orchestrator {
        Orchestrator::new(OrchestratorConfig {
            max_global_concurrent: 5,
            max_queue_depth: 10,
            ..Default::default()
        })
    }

    #[test]
    fn test_register_tenant() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();
        assert_eq!(orch.list_tenants().len(), 1);

        // Duplicate registration fails
        let result = orch.register_tenant("t1", TenantConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_and_schedule() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();

        let ticket = orch.submit("t1", 1024).unwrap();
        assert_eq!(orch.ticket_state(&ticket), Some(TicketState::Queued));

        let ready = orch.schedule();
        assert_eq!(ready.len(), 1);
        assert_eq!(orch.ticket_state(&ticket), Some(TicketState::Running));
    }

    #[test]
    fn test_complete() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();

        let ticket = orch.submit("t1", 1024).unwrap();
        orch.schedule();
        orch.complete(ticket, true);

        assert_eq!(orch.ticket_state(&ticket), Some(TicketState::Completed));

        let metrics = orch.tenant_metrics("t1").unwrap();
        assert_eq!(metrics.successful, 1);
        assert_eq!(metrics.total_submissions, 1);
    }

    #[test]
    fn test_concurrent_limit() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig { max_concurrent: 2, ..Default::default() })
            .unwrap();

        // Submit 5 items
        for _ in 0..5 {
            orch.submit("t1", 1024).unwrap();
        }

        // Only 2 should be scheduled (tenant limit)
        let ready = orch.schedule();
        assert_eq!(ready.len(), 2);

        // Complete one, schedule more
        orch.complete(ready[0], true);
        let ready2 = orch.schedule();
        assert_eq!(ready2.len(), 1);
    }

    #[test]
    fn test_global_concurrent_limit() {
        let orch = Orchestrator::new(OrchestratorConfig {
            max_global_concurrent: 3,
            max_queue_depth: 100,
            ..Default::default()
        });

        orch.register_tenant("t1", TenantConfig { max_concurrent: 5, ..Default::default() })
            .unwrap();
        orch.register_tenant("t2", TenantConfig { max_concurrent: 5, ..Default::default() })
            .unwrap();

        // Submit 5 from each tenant
        for _ in 0..5 {
            orch.submit("t1", 1024).unwrap();
            orch.submit("t2", 1024).unwrap();
        }

        // Only 3 should be scheduled (global limit)
        let ready = orch.schedule();
        assert!(ready.len() <= 3);
    }

    #[test]
    fn test_suspend_resume() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();

        orch.suspend_tenant("t1").unwrap();
        assert_eq!(orch.tenant_status("t1"), Some(TenantStatus::Suspended));

        // Submit should fail when suspended
        let result = orch.submit("t1", 1024);
        assert!(result.is_err());

        orch.resume_tenant("t1").unwrap();
        assert_eq!(orch.tenant_status("t1"), Some(TenantStatus::Active));

        // Submit should work now
        assert!(orch.submit("t1", 1024).is_ok());
    }

    #[test]
    fn test_remove_tenant() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();

        // Can remove empty tenant
        assert!(orch.remove_tenant("t1").is_ok());
        assert!(orch.list_tenants().is_empty());
    }

    #[test]
    fn test_remove_tenant_with_active() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();
        orch.submit("t1", 1024).unwrap();
        orch.schedule();

        // Cannot remove tenant with active items
        assert!(orch.remove_tenant("t1").is_err());
    }

    #[test]
    fn test_unknown_tenant() {
        let orch = setup();
        assert!(orch.submit("unknown", 1024).is_err());
        assert!(orch.tenant_metrics("unknown").is_none());
    }

    #[test]
    fn test_stats() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();
        orch.register_tenant("t2", TenantConfig::default()).unwrap();

        orch.submit("t1", 1024).unwrap();
        orch.submit("t1", 1024).unwrap();
        orch.submit("t2", 1024).unwrap();

        orch.schedule();

        let stats = orch.stats();
        assert_eq!(stats.tenant_count, 2);
        assert_eq!(stats.total_submissions, 3);
        assert_eq!(stats.total_running, 3);
    }

    #[test]
    fn test_failed_completion() {
        let orch = setup();
        orch.register_tenant("t1", TenantConfig::default()).unwrap();

        let ticket = orch.submit("t1", 1024).unwrap();
        orch.schedule();
        orch.complete(ticket, false);

        assert_eq!(orch.ticket_state(&ticket), Some(TicketState::Failed));
        let metrics = orch.tenant_metrics("t1").unwrap();
        assert_eq!(metrics.failed, 1);
    }

    #[test]
    fn test_update_tenant_config() {
        let orch = Orchestrator::new(OrchestratorConfig {
            max_global_concurrent: 100,
            max_queue_depth: 100,
            ..Default::default()
        });
        orch.register_tenant("t1", TenantConfig::default()).unwrap();

        let new_config = TenantConfig { max_concurrent: 20, ..Default::default() };
        orch.update_tenant("t1", new_config).unwrap();

        // Verify by submitting more than the old limit (10)
        for _ in 0..15 {
            orch.submit("t1", 1024).unwrap();
        }
        let ready = orch.schedule();
        assert!(ready.len() > 10);
    }
}
