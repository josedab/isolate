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

#![allow(dead_code)]

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
            (HttpMethod::Get, "/api/v1/health") => self.handle_health(),
            (HttpMethod::Get, "/api/v1/alerts") => self.handle_alerts(),
            _ => {
                serde_json::to_string(&ApiResponse::<()>::error(format!("Not found: {}", path)))
                    .unwrap_or_default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
