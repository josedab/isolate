//! HTTP API router for the observability dashboard.
//!
//! Provides typed route definitions and JSON response builders for exposing
//! dashboard data over HTTP. This module defines the API surface without
//! coupling to a specific HTTP framework.
//!
//! # Routes
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | /api/v1/overview | Dashboard overview metrics |
//! | GET | /api/v1/sandboxes | List all sandboxes |
//! | GET | /api/v1/sandboxes/:id | Get specific sandbox |
//! | GET | /api/v1/events | Recent events |
//! | GET | /api/v1/resources | Resource usage summary |
//! | GET | /api/v1/health | Health check |



#![allow(missing_docs)]
use crate::dashboard::{
    AlertLevel, AlertThresholds, DashboardEvent, DashboardState,
};
use crate::sandbox::SandboxId;

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// HTTP method enum for route matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// A registered API route.
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method.
    pub method: HttpMethod,
    /// Path pattern (e.g., "/api/v1/sandboxes/:id").
    pub path: String,
    /// Description of what this route does.
    pub description: String,
}

/// API response wrapper with consistent structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T: Serialize> {
    /// Whether the request succeeded.
    pub ok: bool,
    /// Response data (None on error).
    pub data: Option<T>,
    /// Error message (None on success).
    pub error: Option<String>,
    /// Response timestamp.
    pub timestamp: u64,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a success response.
    pub fn success(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs(),
        }
    }
}

impl ApiResponse<()> {
    /// Create an error response.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs(),
        }
    }
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Version string.
    pub version: String,
}

/// Events list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsResponse {
    /// Events returned.
    pub events: Vec<DashboardEvent>,
    /// Total events available.
    pub count: usize,
}

/// Alert status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertStatus {
    /// Current alert level.
    pub level: AlertLevel,
    /// Active alerts.
    pub active_alerts: Vec<String>,
    /// Thresholds in use.
    pub thresholds: AlertThresholds,
}

/// The API router — dispatches requests to the dashboard state.
pub struct DashboardRouter {
    state: Arc<DashboardState>,
    version: String,
    thresholds: AlertThresholds,
}

impl DashboardRouter {
    /// Create a new router wrapping the given dashboard state.
    pub fn new(state: Arc<DashboardState>) -> Self {
        Self {
            state,
            version: env!("CARGO_PKG_VERSION").to_string(),
            thresholds: AlertThresholds::default(),
        }
    }

    /// Set custom alert thresholds.
    pub fn with_thresholds(mut self, thresholds: AlertThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// List all registered routes.
    pub fn routes(&self) -> Vec<Route> {
        vec![
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/overview".to_string(),
                description: "Dashboard overview metrics".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/sandboxes".to_string(),
                description: "List all tracked sandboxes".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/sandboxes/:id".to_string(),
                description: "Get a specific sandbox by ID".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/events".to_string(),
                description: "Recent dashboard events".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/resources".to_string(),
                description: "Aggregate resource usage summary".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/health".to_string(),
                description: "Health check endpoint".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/alerts".to_string(),
                description: "Current alert status".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/history".to_string(),
                description: "Execution history with durations and status".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/resources/heatmap".to_string(),
                description: "Resource usage heatmap across sandboxes".to_string(),
            },
            Route {
                method: HttpMethod::Get,
                path: "/api/v1/ws/events".to_string(),
                description: "WebSocket connection info for live streaming".to_string(),
            },
        ]
    }

    /// Handle GET /api/v1/overview
    pub fn handle_overview(&self) -> String {
        let overview = self.state.overview();
        serde_json::to_string(&ApiResponse::success(overview)).unwrap_or_default()
    }

    /// Handle GET /api/v1/sandboxes
    pub fn handle_list_sandboxes(&self) -> String {
        let sandboxes = self.state.list_sandboxes();
        serde_json::to_string(&ApiResponse::success(sandboxes)).unwrap_or_default()
    }

    /// Handle GET /api/v1/sandboxes/:id
    pub fn handle_get_sandbox(&self, id: &SandboxId) -> String {
        match self.state.get_sandbox(id) {
            Some(sandbox) => {
                serde_json::to_string(&ApiResponse::success(sandbox)).unwrap_or_default()
            }
            None => {
                serde_json::to_string(&ApiResponse::<()>::error("Sandbox not found"))
                    .unwrap_or_default()
            }
        }
    }

    /// Handle GET /api/v1/events?limit=N
    pub fn handle_events(&self, limit: Option<usize>) -> String {
        let limit = limit.unwrap_or(50).min(1000);
        let events = self.state.recent_events(limit);
        let resp = EventsResponse {
            count: events.len(),
            events,
        };
        serde_json::to_string(&ApiResponse::success(resp)).unwrap_or_default()
    }

    /// Handle GET /api/v1/resources
    pub fn handle_resources(&self) -> String {
        let summary = self.state.resource_summary();
        serde_json::to_string(&ApiResponse::success(summary)).unwrap_or_default()
    }

    /// Handle GET /api/v1/health
    pub fn handle_health(&self) -> String {
        let overview = self.state.overview();
        let resp = HealthResponse {
            status: "healthy".to_string(),
            uptime_secs: overview.uptime.as_secs(),
            version: self.version.clone(),
        };
        serde_json::to_string(&ApiResponse::success(resp)).unwrap_or_default()
    }

    /// Handle GET /api/v1/alerts
    pub fn handle_alerts(&self) -> String {
        self.state.check_alerts(&self.thresholds);
        let events = self.state.recent_events(100);
        let active_alerts: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                DashboardEvent::Alert { level: _, message } => Some(message.clone()),
                _ => None,
            })
            .collect();

        let level = if active_alerts.iter().any(|_| true) {
            // Check if any critical alerts exist
            let has_critical = events.iter().any(|e| matches!(e, DashboardEvent::Alert { level: AlertLevel::Critical, .. }));
            if has_critical {
                AlertLevel::Critical
            } else if !active_alerts.is_empty() {
                AlertLevel::Warning
            } else {
                AlertLevel::Info
            }
        } else {
            AlertLevel::Info
        };

        let resp = AlertStatus {
            level,
            active_alerts,
            thresholds: self.thresholds.clone(),
        };
        serde_json::to_string(&ApiResponse::success(resp)).unwrap_or_default()
    }

    /// Simple request dispatcher — matches path to handler.
    pub fn dispatch(&self, method: HttpMethod, path: &str) -> String {
        match (method, path) {
            (HttpMethod::Get, "/api/v1/overview") => self.handle_overview(),
            (HttpMethod::Get, "/api/v1/sandboxes") => self.handle_list_sandboxes(),
            (HttpMethod::Get, "/api/v1/events") => self.handle_events(None),
            (HttpMethod::Get, "/api/v1/resources") => self.handle_resources(),
            (HttpMethod::Get, "/api/v1/resources/heatmap") => self.handle_resource_heatmap(),
            (HttpMethod::Get, "/api/v1/history") => self.handle_execution_history(None),
            (HttpMethod::Get, "/api/v1/health") => self.handle_health(),
            (HttpMethod::Get, "/api/v1/alerts") => self.handle_alerts(),
            (HttpMethod::Get, "/api/v1/ws/events") => self.handle_ws_info(),
            _ => {
                serde_json::to_string(&ApiResponse::<()>::error(format!("Not found: {}", path)))
                    .unwrap_or_default()
            }
        }
    }

    /// Handle GET /api/v1/history?limit=N — execution history.
    pub fn handle_execution_history(&self, limit: Option<usize>) -> String {
        let limit = limit.unwrap_or(100).min(1000);
        let events = self.state.recent_events(limit);
        let history: Vec<ExecutionHistoryEntry> = events
            .iter()
            .filter_map(|e| match e {
                DashboardEvent::RunCompleted { sandbox_id, duration, success } => {
                    Some(ExecutionHistoryEntry {
                        sandbox_id: sandbox_id.to_string(),
                        duration_ms: duration.as_millis() as u64,
                        success: *success,
                        timestamp: SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or(Duration::ZERO)
                            .as_secs(),
                    })
                }
                _ => None,
            })
            .collect();

        let resp = ExecutionHistoryResponse {
            entries: history,
            total: events.len(),
        };
        serde_json::to_string(&ApiResponse::success(resp)).unwrap_or_default()
    }

    /// Handle GET /api/v1/resources/heatmap — resource usage heatmap data.
    pub fn handle_resource_heatmap(&self) -> String {
        let sandboxes = self.state.list_sandboxes();
        let cells: Vec<HeatmapCell> = sandboxes
            .iter()
            .map(|s| {
                let (memory_pct, fuel_pct) = if let Some(ref usage) = s.resource_usage {
                    let mem = (usage.peak_memory as f64 / (128.0 * 1024.0 * 1024.0) * 100.0).min(100.0);
                    let fuel = (usage.fuel_consumed as f64 / 10_000_000.0 * 100.0).min(100.0);
                    (mem, fuel)
                } else {
                    (0.0, 0.0)
                };
                HeatmapCell {
                    sandbox_id: s.id.to_string(),
                    memory_pct,
                    fuel_pct,
                    run_count: s.run_count,
                    intensity: (memory_pct + fuel_pct) / 2.0,
                }
            })
            .collect();

        let resp = ResourceHeatmapResponse { cells };
        serde_json::to_string(&ApiResponse::success(resp)).unwrap_or_default()
    }

    /// Handle GET /api/v1/ws/events — returns WebSocket connection info.
    pub fn handle_ws_info(&self) -> String {
        let info = WebSocketInfo {
            endpoint: "/api/v1/ws/events".to_string(),
            protocol: "isolate-dashboard-v1".to_string(),
            supported_channels: vec![
                "sandbox.created".to_string(),
                "sandbox.completed".to_string(),
                "sandbox.failed".to_string(),
                "resource.threshold".to_string(),
                "alert.triggered".to_string(),
            ],
        };
        serde_json::to_string(&ApiResponse::success(info)).unwrap_or_default()
    }

    /// Generate a WebSocket event frame from a dashboard event.
    pub fn event_to_ws_frame(event: &DashboardEvent) -> String {
        let ws_event = WebSocketEvent {
            event_type: match event {
                DashboardEvent::SandboxCreated { .. } => "sandbox.created",
                DashboardEvent::SandboxTerminated { .. } => "sandbox.terminated",
                DashboardEvent::RunCompleted { .. } => "run.completed",
                DashboardEvent::Alert { .. } => "alert.triggered",
            }
            .to_string(),
            payload: serde_json::to_value(event).unwrap_or_default(),
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
        };
        serde_json::to_string(&ws_event).unwrap_or_default()
    }
}

/// WebSocket connection information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketInfo {
    /// WebSocket endpoint path.
    pub endpoint: String,
    /// Sub-protocol identifier.
    pub protocol: String,
    /// Supported event channels.
    pub supported_channels: Vec<String>,
}

/// A WebSocket event frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketEvent {
    /// Event type identifier (e.g., "sandbox.created").
    pub event_type: String,
    /// Event payload.
    pub payload: serde_json::Value,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
}

/// Threshold-based alert configuration for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    /// CPU usage threshold percentage.
    pub cpu_threshold_pct: f64,
    /// Memory usage threshold percentage.
    pub memory_threshold_pct: f64,
    /// Maximum execution duration before alerting.
    pub max_execution_secs: u64,
    /// Maximum error rate (0.0-1.0).
    pub max_error_rate: f64,
    /// Whether alerting is enabled.
    pub enabled: bool,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            cpu_threshold_pct: 80.0,
            memory_threshold_pct: 85.0,
            max_execution_secs: 300,
            max_error_rate: 0.1,
            enabled: true,
        }
    }
}

/// Execution history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionHistoryEntry {
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Whether execution succeeded.
    pub success: bool,
    /// Unix timestamp.
    pub timestamp: u64,
}

/// Response for execution history endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionHistoryResponse {
    /// History entries.
    pub entries: Vec<ExecutionHistoryEntry>,
    /// Total events available.
    pub total: usize,
}

/// A single cell in the resource heatmap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatmapCell {
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Memory usage as percentage of a reference limit.
    pub memory_pct: f64,
    /// Fuel usage as percentage of a reference limit.
    pub fuel_pct: f64,
    /// Number of executions.
    pub run_count: u64,
    /// Combined intensity (0.0 - 100.0).
    pub intensity: f64,
}

/// Response for resource heatmap endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceHeatmapResponse {
    /// Heatmap cells, one per sandbox.
    pub cells: Vec<HeatmapCell>,
}

#[cfg(test)]
#[cfg(feature = "observability")]
mod tests {
    use super::*;
    use crate::dashboard::{DashboardOverview, ResourceSummary, SandboxSummary};
    use crate::resource::ResourceUsage;
    use std::time::Duration;

    fn setup_router() -> DashboardRouter {
        let state = Arc::new(DashboardState::new(100));
        DashboardRouter::new(state)
    }

    #[test]
    fn test_health_endpoint() {
        let router = setup_router();
        let json = router.handle_health();
        let resp: ApiResponse<HealthResponse> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap().status, "healthy");
    }

    #[test]
    fn test_overview_endpoint() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash123".to_string());

        let router = DashboardRouter::new(state);
        let json = router.handle_overview();
        let resp: ApiResponse<DashboardOverview> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data.total_sandboxes, 1);
        assert_eq!(data.total_created, 1);
    }

    #[test]
    fn test_list_sandboxes_endpoint() {
        let state = Arc::new(DashboardState::new(100));
        let id1 = SandboxId::new();
        let id2 = SandboxId::new();
        state.register_sandbox(id1, "hash1".to_string());
        state.register_sandbox(id2, "hash2".to_string());

        let router = DashboardRouter::new(state);
        let json = router.handle_list_sandboxes();
        let resp: ApiResponse<Vec<SandboxSummary>> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap().len(), 2);
    }

    #[test]
    fn test_get_sandbox_found() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash".to_string());

        let router = DashboardRouter::new(state);
        let json = router.handle_get_sandbox(&id);
        let resp: ApiResponse<SandboxSummary> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap().module_hash, "hash");
    }

    #[test]
    fn test_get_sandbox_not_found() {
        let router = setup_router();
        let id = SandboxId::new();
        let json = router.handle_get_sandbox(&id);
        let resp: ApiResponse<()> = serde_json::from_str(&json).unwrap();
        assert!(!resp.ok);
        assert!(resp.error.unwrap().contains("not found"));
    }

    #[test]
    fn test_resources_endpoint() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash".to_string());
        state.record_run(&id, Duration::from_millis(10), ResourceUsage::default(), true);

        let router = DashboardRouter::new(state);
        let json = router.handle_resources();
        let resp: ApiResponse<ResourceSummary> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
    }

    #[test]
    fn test_events_endpoint() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash".to_string());

        let router = DashboardRouter::new(state);
        let json = router.handle_events(Some(10));
        let resp: ApiResponse<EventsResponse> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        assert!(resp.data.unwrap().count > 0);
    }

    #[test]
    fn test_dispatch_routing() {
        let router = setup_router();

        // Valid routes
        let json = router.dispatch(HttpMethod::Get, "/api/v1/health");
        assert!(json.contains("healthy"));

        let json = router.dispatch(HttpMethod::Get, "/api/v1/overview");
        assert!(json.contains("active_sandboxes"));

        // Invalid route
        let json = router.dispatch(HttpMethod::Get, "/api/v1/unknown");
        assert!(json.contains("Not found"));
    }

    #[test]
    fn test_route_listing() {
        let router = setup_router();
        let routes = router.routes();
        assert!(routes.len() >= 7);
        assert!(routes.iter().any(|r| r.path == "/api/v1/health"));
        assert!(routes.iter().any(|r| r.path == "/api/v1/overview"));
        assert!(routes.iter().any(|r| r.path == "/api/v1/sandboxes"));
    }

    #[test]
    fn test_api_response_success() {
        let resp = ApiResponse::success("hello");
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap(), "hello");
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_api_response_error() {
        let resp = ApiResponse::<()>::error("bad request");
        assert!(!resp.ok);
        assert!(resp.data.is_none());
        assert_eq!(resp.error.unwrap(), "bad request");
    }

    #[test]
    fn test_execution_history_endpoint() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash".to_string());
        state.record_run(&id, Duration::from_millis(50), ResourceUsage::default(), true);

        let router = DashboardRouter::new(state);
        let json = router.handle_execution_history(Some(50));
        let resp: ApiResponse<ExecutionHistoryResponse> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data.entries.len(), 1);
        assert!(data.entries[0].success);
    }

    #[test]
    fn test_resource_heatmap_endpoint() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash".to_string());
        state.record_run(&id, Duration::from_millis(10), ResourceUsage::default(), true);

        let router = DashboardRouter::new(state);
        let json = router.handle_resource_heatmap();
        let resp: ApiResponse<ResourceHeatmapResponse> = serde_json::from_str(&json).unwrap();
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data.cells.len(), 1);
    }

    #[test]
    fn test_dispatch_new_routes() {
        let state = Arc::new(DashboardState::new(100));
        let id = SandboxId::new();
        state.register_sandbox(id, "hash".to_string());
        state.record_run(&id, Duration::from_millis(10), ResourceUsage::default(), true);

        let router = DashboardRouter::new(state);

        let json = router.dispatch(HttpMethod::Get, "/api/v1/history");
        assert!(json.contains("entries"));

        let json = router.dispatch(HttpMethod::Get, "/api/v1/resources/heatmap");
        assert!(json.contains("cells"));
    }
}
