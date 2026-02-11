//! Real-time metrics dashboard API.
//!
//! Provides HTTP-compatible data structures for exposing sandbox metrics,
//! status information, and configuration through a dashboard interface.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::dashboard::{DashboardState, SandboxSummary};
//!
//! let dashboard = DashboardState::new(100);
//! let overview = dashboard.overview();
//! assert_eq!(overview.active_sandboxes, 0);
//! ```

use crate::resource::ResourceUsage;
use crate::sandbox::{SandboxId, SandboxState};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Central state for the metrics dashboard.
pub struct DashboardState {
    /// Tracked sandbox summaries.
    sandboxes: DashMap<SandboxId, SandboxSummary>,
    /// Recent events log.
    events: RwLock<VecDeque<DashboardEvent>>,
    /// Maximum events to retain.
    max_events: usize,
    /// Global counters.
    counters: DashboardCounters,
    /// Start time.
    started_at: Instant,
}

/// Counters for global dashboard metrics.
#[derive(Debug, Default)]
struct DashboardCounters {
    total_created: AtomicU64,
    total_completed: AtomicU64,
    total_failed: AtomicU64,
}

impl DashboardState {
    /// Create a new dashboard state.
    pub fn new(max_events: usize) -> Self {
        Self {
            sandboxes: DashMap::new(),
            events: RwLock::new(VecDeque::new()),
            max_events,
            counters: DashboardCounters::default(),
            started_at: Instant::now(),
        }
    }

    /// Register a new sandbox.
    pub fn register_sandbox(&self, id: SandboxId, module_hash: String) {
        let summary = SandboxSummary {
            id,
            state: SandboxState::Ready,
            module_hash,
            created_at: SystemTime::now(),
            run_count: 0,
            last_run_duration: None,
            resource_usage: None,
        };
        self.sandboxes.insert(id, summary);
        self.counters.total_created.fetch_add(1, Ordering::Relaxed);
        self.push_event(DashboardEvent::SandboxCreated { sandbox_id: id });
    }

    /// Update sandbox state.
    pub fn update_state(&self, id: &SandboxId, state: SandboxState) {
        if let Some(mut entry) = self.sandboxes.get_mut(id) {
            entry.state = state;
        }

        if state == SandboxState::Terminated {
            self.counters.total_completed.fetch_add(1, Ordering::Relaxed);
            self.push_event(DashboardEvent::SandboxTerminated { sandbox_id: *id });
        }
    }

    /// Record a run completion.
    pub fn record_run(&self, id: &SandboxId, duration: Duration, usage: ResourceUsage, success: bool) {
        if let Some(mut entry) = self.sandboxes.get_mut(id) {
            entry.run_count += 1;
            entry.last_run_duration = Some(duration);
            entry.resource_usage = Some(usage);
        }
        if !success {
            self.counters.total_failed.fetch_add(1, Ordering::Relaxed);
        }
        self.push_event(DashboardEvent::RunCompleted {
            sandbox_id: *id,
            duration,
            success,
        });
    }

    /// Remove a sandbox from tracking.
    pub fn remove_sandbox(&self, id: &SandboxId) {
        self.sandboxes.remove(id);
    }

    /// Get dashboard overview.
    pub fn overview(&self) -> DashboardOverview {
        let sandboxes: Vec<SandboxSummary> =
            self.sandboxes.iter().map(|e| e.value().clone()).collect();

        let active = sandboxes.iter().filter(|s| s.state == SandboxState::Running).count();
        let ready = sandboxes.iter().filter(|s| s.state == SandboxState::Ready).count();

        DashboardOverview {
            active_sandboxes: active,
            ready_sandboxes: ready,
            total_sandboxes: sandboxes.len(),
            total_created: self.counters.total_created.load(Ordering::Relaxed),
            total_completed: self.counters.total_completed.load(Ordering::Relaxed),
            total_failed: self.counters.total_failed.load(Ordering::Relaxed),
            uptime: self.started_at.elapsed(),
        }
    }

    /// List all tracked sandboxes.
    pub fn list_sandboxes(&self) -> Vec<SandboxSummary> {
        self.sandboxes.iter().map(|e| e.value().clone()).collect()
    }

    /// Get a specific sandbox summary.
    pub fn get_sandbox(&self, id: &SandboxId) -> Option<SandboxSummary> {
        self.sandboxes.get(id).map(|e| e.value().clone())
    }

    /// Get recent events.
    pub fn recent_events(&self, limit: usize) -> Vec<DashboardEvent> {
        let events = self.events.read();
        events.iter().rev().take(limit).cloned().collect()
    }

    fn push_event(&self, event: DashboardEvent) {
        let mut events = self.events.write();
        if events.len() >= self.max_events {
            events.pop_front();
        }
        events.push_back(event);
    }
}

/// Summary of a tracked sandbox for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSummary {
    /// Sandbox ID.
    pub id: SandboxId,
    /// Current state.
    pub state: SandboxState,
    /// Module hash.
    pub module_hash: String,
    /// When the sandbox was created.
    pub created_at: SystemTime,
    /// Number of completed runs.
    pub run_count: u64,
    /// Duration of the last run.
    pub last_run_duration: Option<Duration>,
    /// Last known resource usage.
    pub resource_usage: Option<ResourceUsage>,
}

/// High-level dashboard overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardOverview {
    /// Number of currently running sandboxes.
    pub active_sandboxes: usize,
    /// Number of ready (idle) sandboxes.
    pub ready_sandboxes: usize,
    /// Total tracked sandboxes.
    pub total_sandboxes: usize,
    /// Total sandboxes ever created.
    pub total_created: u64,
    /// Total sandboxes completed.
    pub total_completed: u64,
    /// Total failed runs.
    pub total_failed: u64,
    /// Server uptime.
    pub uptime: Duration,
}

/// Events tracked by the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DashboardEvent {
    /// A sandbox was created.
    SandboxCreated { sandbox_id: SandboxId },
    /// A sandbox was terminated.
    SandboxTerminated { sandbox_id: SandboxId },
    /// A run completed.
    RunCompleted {
        sandbox_id: SandboxId,
        duration: Duration,
        success: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_state_new() {
        let dashboard = DashboardState::new(100);
        let overview = dashboard.overview();
        assert_eq!(overview.active_sandboxes, 0);
        assert_eq!(overview.total_sandboxes, 0);
        assert_eq!(overview.total_created, 0);
    }

    #[test]
    fn test_register_sandbox() {
        let dashboard = DashboardState::new(100);
        let id = SandboxId::new();

        dashboard.register_sandbox(id, "hash123".to_string());

        let overview = dashboard.overview();
        assert_eq!(overview.total_sandboxes, 1);
        assert_eq!(overview.total_created, 1);
        assert_eq!(overview.ready_sandboxes, 1);

        let sandbox = dashboard.get_sandbox(&id).unwrap();
        assert_eq!(sandbox.module_hash, "hash123");
        assert_eq!(sandbox.state, SandboxState::Ready);
    }

    #[test]
    fn test_update_state() {
        let dashboard = DashboardState::new(100);
        let id = SandboxId::new();

        dashboard.register_sandbox(id, "hash".to_string());
        dashboard.update_state(&id, SandboxState::Running);

        let overview = dashboard.overview();
        assert_eq!(overview.active_sandboxes, 1);
        assert_eq!(overview.ready_sandboxes, 0);
    }

    #[test]
    fn test_record_run() {
        let dashboard = DashboardState::new(100);
        let id = SandboxId::new();

        dashboard.register_sandbox(id, "hash".to_string());
        dashboard.record_run(&id, Duration::from_millis(50), ResourceUsage::default(), true);

        let sandbox = dashboard.get_sandbox(&id).unwrap();
        assert_eq!(sandbox.run_count, 1);
        assert_eq!(sandbox.last_run_duration, Some(Duration::from_millis(50)));
    }

    #[test]
    fn test_failed_run_tracking() {
        let dashboard = DashboardState::new(100);
        let id = SandboxId::new();

        dashboard.register_sandbox(id, "hash".to_string());
        dashboard.record_run(&id, Duration::from_millis(10), ResourceUsage::default(), false);

        let overview = dashboard.overview();
        assert_eq!(overview.total_failed, 1);
    }

    #[test]
    fn test_recent_events() {
        let dashboard = DashboardState::new(100);
        let id = SandboxId::new();

        dashboard.register_sandbox(id, "hash".to_string());
        dashboard.update_state(&id, SandboxState::Terminated);

        let events = dashboard.recent_events(10);
        assert_eq!(events.len(), 2); // Created + Terminated
    }

    #[test]
    fn test_event_limit() {
        let dashboard = DashboardState::new(3);

        for _ in 0..5 {
            let id = SandboxId::new();
            dashboard.register_sandbox(id, "hash".to_string());
        }

        let events = dashboard.recent_events(10);
        assert_eq!(events.len(), 3); // Limited to max_events
    }

    #[test]
    fn test_list_sandboxes() {
        let dashboard = DashboardState::new(100);

        let id1 = SandboxId::new();
        let id2 = SandboxId::new();
        dashboard.register_sandbox(id1, "hash1".to_string());
        dashboard.register_sandbox(id2, "hash2".to_string());

        let sandboxes = dashboard.list_sandboxes();
        assert_eq!(sandboxes.len(), 2);
    }

    #[test]
    fn test_remove_sandbox() {
        let dashboard = DashboardState::new(100);
        let id = SandboxId::new();

        dashboard.register_sandbox(id, "hash".to_string());
        assert!(dashboard.get_sandbox(&id).is_some());

        dashboard.remove_sandbox(&id);
        assert!(dashboard.get_sandbox(&id).is_none());
    }
}
