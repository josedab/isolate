//! Debug Adapter Protocol (DAP) server for live debugging.
//!
//! Implements the Debug Adapter Protocol for integration with IDEs
//! and the Isolate debug CLI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// DAP message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DapMessage {
    /// Request from client to server.
    Request(DapRequest),
    /// Response from server to client.
    Response(DapResponse),
    /// Event from server to client.
    Event(DapEvent),
}

/// A DAP request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DapRequest {
    /// Sequence number.
    pub seq: i64,
    /// Command name.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// A DAP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DapResponse {
    /// Sequence number.
    pub seq: i64,
    /// Request sequence number this responds to.
    pub request_seq: i64,
    /// Whether the request was successful.
    pub success: bool,
    /// Command that was requested.
    pub command: String,
    /// Response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    /// Error message if not successful.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A DAP event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DapEvent {
    /// Sequence number.
    pub seq: i64,
    /// Event type.
    pub event: String,
    /// Event body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// Supported DAP commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DapCommand {
    Initialize,
    Launch,
    Attach,
    SetBreakpoints,
    ConfigurationDone,
    Continue,
    Next,
    StepIn,
    StepOut,
    Pause,
    Disconnect,
    Threads,
    StackTrace,
    Scopes,
    Variables,
    Evaluate,
    Source,
}

impl DapCommand {
    /// Parse from command string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "initialize" => Some(Self::Initialize),
            "launch" => Some(Self::Launch),
            "attach" => Some(Self::Attach),
            "setBreakpoints" => Some(Self::SetBreakpoints),
            "configurationDone" => Some(Self::ConfigurationDone),
            "continue" => Some(Self::Continue),
            "next" => Some(Self::Next),
            "stepIn" => Some(Self::StepIn),
            "stepOut" => Some(Self::StepOut),
            "pause" => Some(Self::Pause),
            "disconnect" => Some(Self::Disconnect),
            "threads" => Some(Self::Threads),
            "stackTrace" => Some(Self::StackTrace),
            "scopes" => Some(Self::Scopes),
            "variables" => Some(Self::Variables),
            "evaluate" => Some(Self::Evaluate),
            "source" => Some(Self::Source),
            _ => None,
        }
    }
}

/// Real-time resource dashboard data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDataPoint {
    /// Timestamp (milliseconds since debug session start).
    pub timestamp_ms: f64,
    /// Fuel consumed at this point.
    pub fuel_consumed: u64,
    /// Current memory usage in bytes.
    pub memory_bytes: u64,
    /// I/O bytes read.
    pub io_read_bytes: u64,
    /// I/O bytes written.
    pub io_write_bytes: u64,
    /// Current execution state.
    pub state: String,
}

/// Resource dashboard for real-time monitoring.
pub struct ResourceDashboard {
    /// Data points collected.
    data_points: Arc<RwLock<Vec<ResourceDataPoint>>>,
    /// Maximum data points to keep.
    max_points: usize,
    /// Session start time.
    start_time: Instant,
    /// Collection interval.
    interval: Duration,
    /// Total fuel at last collection.
    last_fuel: AtomicU64,
    /// Total memory at last collection.
    last_memory: AtomicU64,
}

impl ResourceDashboard {
    /// Create a new resource dashboard.
    pub fn new(max_points: usize, interval: Duration) -> Self {
        Self {
            data_points: Arc::new(RwLock::new(Vec::new())),
            max_points,
            start_time: Instant::now(),
            interval,
            last_fuel: AtomicU64::new(0),
            last_memory: AtomicU64::new(0),
        }
    }

    /// Record a data point.
    pub fn record(&self, fuel: u64, memory: u64, io_read: u64, io_write: u64, state: &str) {
        let point = ResourceDataPoint {
            timestamp_ms: self.start_time.elapsed().as_secs_f64() * 1000.0,
            fuel_consumed: fuel,
            memory_bytes: memory,
            io_read_bytes: io_read,
            io_write_bytes: io_write,
            state: state.to_string(),
        };

        self.last_fuel.store(fuel, Ordering::Relaxed);
        self.last_memory.store(memory, Ordering::Relaxed);

        let mut points = self.data_points.write();
        points.push(point);

        // Evict old points
        if points.len() > self.max_points {
            let drain_count = points.len() - self.max_points;
            points.drain(..drain_count);
        }
    }

    /// Get all data points.
    pub fn data_points(&self) -> Vec<ResourceDataPoint> {
        self.data_points.read().clone()
    }

    /// Get the latest data point.
    pub fn latest(&self) -> Option<ResourceDataPoint> {
        self.data_points.read().last().cloned()
    }

    /// Get summary statistics.
    pub fn summary(&self) -> DashboardSummary {
        let points = self.data_points.read();
        if points.is_empty() {
            return DashboardSummary::default();
        }

        let peak_memory = points.iter().map(|p| p.memory_bytes).max().unwrap_or(0);
        let total_fuel = points.last().map(|p| p.fuel_consumed).unwrap_or(0);
        let duration_ms = points.last().map(|p| p.timestamp_ms).unwrap_or(0.0);
        let fuel_rate =
            if duration_ms > 0.0 { total_fuel as f64 / (duration_ms / 1000.0) } else { 0.0 };

        DashboardSummary {
            data_point_count: points.len(),
            duration_ms,
            peak_memory_bytes: peak_memory,
            total_fuel_consumed: total_fuel,
            fuel_rate_per_sec: fuel_rate,
            total_io_read: points.last().map(|p| p.io_read_bytes).unwrap_or(0),
            total_io_write: points.last().map(|p| p.io_write_bytes).unwrap_or(0),
        }
    }

    /// Clear all data points.
    pub fn clear(&self) {
        self.data_points.write().clear();
    }
}

/// Summary of dashboard data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DashboardSummary {
    /// Number of data points collected.
    pub data_point_count: usize,
    /// Total duration in milliseconds.
    pub duration_ms: f64,
    /// Peak memory usage.
    pub peak_memory_bytes: u64,
    /// Total fuel consumed.
    pub total_fuel_consumed: u64,
    /// Fuel consumption rate (per second).
    pub fuel_rate_per_sec: f64,
    /// Total I/O read bytes.
    pub total_io_read: u64,
    /// Total I/O write bytes.
    pub total_io_write: u64,
}

/// DAP server state.
pub struct DapServer {
    /// Next sequence number.
    next_seq: AtomicI64,
    /// Client capabilities.
    client_capabilities: RwLock<HashMap<String, bool>>,
    /// Server capabilities.
    server_capabilities: HashMap<String, bool>,
    /// Whether initialized.
    initialized: RwLock<bool>,
    /// Resource dashboard.
    dashboard: ResourceDashboard,
}

impl DapServer {
    /// Create a new DAP server.
    pub fn new() -> Self {
        let mut server_caps = HashMap::new();
        server_caps.insert("supportsConfigurationDoneRequest".to_string(), true);
        server_caps.insert("supportsFunctionBreakpoints".to_string(), true);
        server_caps.insert("supportsEvaluateForHovers".to_string(), true);
        server_caps.insert("supportsStepBack".to_string(), true);
        server_caps.insert("supportsRestartFrame".to_string(), false);

        Self {
            next_seq: AtomicI64::new(1),
            client_capabilities: RwLock::new(HashMap::new()),
            server_capabilities: server_caps,
            initialized: RwLock::new(false),
            dashboard: ResourceDashboard::new(10_000, Duration::from_millis(100)),
        }
    }

    /// Get the next sequence number.
    fn next_seq(&self) -> i64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Handle a DAP request and return a response.
    pub fn handle_request(&self, request: &DapRequest) -> DapResponse {
        let command = DapCommand::parse(&request.command);

        match command {
            Some(DapCommand::Initialize) => self.handle_initialize(request),
            Some(DapCommand::Launch) => self.handle_launch(request),
            Some(DapCommand::Disconnect) => self.handle_disconnect(request),
            Some(DapCommand::Threads) => self.handle_threads(request),
            Some(DapCommand::ConfigurationDone) => self.success_response(request, None),
            Some(DapCommand::Continue) => self.success_response(
                request,
                Some(serde_json::json!({
                    "allThreadsContinued": true
                })),
            ),
            _ => DapResponse {
                seq: self.next_seq(),
                request_seq: request.seq,
                success: false,
                command: request.command.clone(),
                body: None,
                message: Some(format!("Unsupported command: {}", request.command)),
            },
        }
    }

    fn handle_initialize(&self, request: &DapRequest) -> DapResponse {
        *self.initialized.write() = true;

        self.success_response(
            request,
            Some(serde_json::json!({
                "supportsConfigurationDoneRequest": true,
                "supportsFunctionBreakpoints": true,
                "supportsEvaluateForHovers": true,
                "supportsStepBack": true,
            })),
        )
    }

    fn handle_launch(&self, request: &DapRequest) -> DapResponse {
        self.success_response(request, None)
    }

    fn handle_disconnect(&self, request: &DapRequest) -> DapResponse {
        *self.initialized.write() = false;
        self.success_response(request, None)
    }

    fn handle_threads(&self, request: &DapRequest) -> DapResponse {
        self.success_response(
            request,
            Some(serde_json::json!({
                "threads": [{
                    "id": 1,
                    "name": "WASM Main Thread"
                }]
            })),
        )
    }

    fn success_response(
        &self,
        request: &DapRequest,
        body: Option<serde_json::Value>,
    ) -> DapResponse {
        DapResponse {
            seq: self.next_seq(),
            request_seq: request.seq,
            success: true,
            command: request.command.clone(),
            body,
            message: None,
        }
    }

    /// Create a DAP event.
    pub fn create_event(
        &self,
        event: impl Into<String>,
        body: Option<serde_json::Value>,
    ) -> DapEvent {
        DapEvent { seq: self.next_seq(), event: event.into(), body }
    }

    /// Get the resource dashboard.
    pub fn dashboard(&self) -> &ResourceDashboard {
        &self.dashboard
    }

    /// Check if the server is initialized.
    pub fn is_initialized(&self) -> bool {
        *self.initialized.read()
    }
}

impl Default for DapServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dap_command_parse() {
        assert_eq!(DapCommand::parse("initialize"), Some(DapCommand::Initialize));
        assert_eq!(DapCommand::parse("continue"), Some(DapCommand::Continue));
        assert_eq!(DapCommand::parse("unknown"), None);
    }

    #[test]
    fn test_dap_server_initialize() {
        let server = DapServer::new();
        assert!(!server.is_initialized());

        let request = DapRequest {
            seq: 1,
            command: "initialize".to_string(),
            arguments: serde_json::json!({}),
        };

        let response = server.handle_request(&request);
        assert!(response.success);
        assert!(server.is_initialized());
    }

    #[test]
    fn test_dap_server_threads() {
        let server = DapServer::new();
        let request =
            DapRequest { seq: 1, command: "threads".to_string(), arguments: serde_json::json!({}) };

        let response = server.handle_request(&request);
        assert!(response.success);
        assert!(response.body.is_some());
    }

    #[test]
    fn test_dap_server_unsupported() {
        let server = DapServer::new();
        let request = DapRequest {
            seq: 1,
            command: "nonExistent".to_string(),
            arguments: serde_json::json!({}),
        };

        let response = server.handle_request(&request);
        assert!(!response.success);
    }

    #[test]
    fn test_dap_server_disconnect() {
        let server = DapServer::new();

        // Initialize first
        server.handle_request(&DapRequest {
            seq: 1,
            command: "initialize".to_string(),
            arguments: serde_json::json!({}),
        });
        assert!(server.is_initialized());

        // Disconnect
        server.handle_request(&DapRequest {
            seq: 2,
            command: "disconnect".to_string(),
            arguments: serde_json::json!({}),
        });
        assert!(!server.is_initialized());
    }

    #[test]
    fn test_resource_dashboard() {
        let dashboard = ResourceDashboard::new(100, Duration::from_millis(100));

        dashboard.record(100, 1024, 0, 0, "running");
        dashboard.record(200, 2048, 10, 5, "running");

        assert_eq!(dashboard.data_points().len(), 2);

        let latest = dashboard.latest().unwrap();
        assert_eq!(latest.fuel_consumed, 200);
        assert_eq!(latest.memory_bytes, 2048);
    }

    #[test]
    fn test_dashboard_summary() {
        let dashboard = ResourceDashboard::new(100, Duration::from_millis(10));

        dashboard.record(0, 1024, 0, 0, "running");
        std::thread::sleep(Duration::from_millis(10));
        dashboard.record(1000, 4096, 100, 50, "running");

        let summary = dashboard.summary();
        assert_eq!(summary.data_point_count, 2);
        assert_eq!(summary.peak_memory_bytes, 4096);
        assert_eq!(summary.total_fuel_consumed, 1000);
        assert!(summary.fuel_rate_per_sec > 0.0);
    }

    #[test]
    fn test_dashboard_eviction() {
        let dashboard = ResourceDashboard::new(3, Duration::from_millis(10));

        for i in 0..5 {
            dashboard.record(i * 100, 1024, 0, 0, "running");
        }

        assert_eq!(dashboard.data_points().len(), 3);
    }

    #[test]
    fn test_create_event() {
        let server = DapServer::new();
        let event = server.create_event(
            "stopped",
            Some(serde_json::json!({
                "reason": "breakpoint",
                "threadId": 1,
            })),
        );

        assert_eq!(event.event, "stopped");
        assert!(event.body.is_some());
    }

    #[test]
    fn test_dap_message_serde() {
        let request = DapRequest {
            seq: 1,
            command: "initialize".to_string(),
            arguments: serde_json::json!({}),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: DapRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, 1);
        assert_eq!(parsed.command, "initialize");
    }
}
