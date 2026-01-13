//! Gateway request/response types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// API error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// HTTP status code.
    pub status: u16,
    /// Error code for programmatic handling.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional suggestion for fixing the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl ApiError {
    /// Create a 400 Bad Request error.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code: "bad_request".to_string(),
            message: message.into(),
            suggestion: None,
        }
    }

    /// Create a 401 Unauthorized error.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: 401,
            code: "unauthorized".to_string(),
            message: message.into(),
            suggestion: Some("Provide a valid API key in the Authorization header.".into()),
        }
    }

    /// Create a 404 Not Found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            code: "not_found".to_string(),
            message: message.into(),
            suggestion: None,
        }
    }

    /// Create a 429 Too Many Requests error.
    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self {
            status: 429,
            code: "rate_limited".to_string(),
            message: format!("Rate limit exceeded. Retry after {} seconds.", retry_after_secs),
            suggestion: Some(
                "Reduce request frequency or contact support for higher limits.".into(),
            ),
        }
    }

    /// Create a 500 Internal Server Error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            code: "internal_error".to_string(),
            message: message.into(),
            suggestion: Some("This is a server error. Please report this issue.".into()),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.status, self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Generic API response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T: Serialize> {
    /// Whether the request was successful.
    pub success: bool,
    /// Response data (present on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// Error details (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a success response.
    pub fn ok(data: T) -> Self {
        Self { success: true, data: Some(data), error: None }
    }

    /// Create an error response.
    pub fn err(error: ApiError) -> Self {
        Self { success: false, data: None, error: Some(error) }
    }
}

/// Request to create a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSandboxRequest {
    /// Base64-encoded WASM module bytes.
    pub module_base64: String,
    /// Capabilities to grant (as string descriptors).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Memory limit in bytes.
    #[serde(default)]
    pub memory_limit: Option<usize>,
    /// CPU fuel limit.
    #[serde(default)]
    pub fuel: Option<u64>,
    /// Wall time limit in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Entry point function name.
    #[serde(default)]
    pub entry_point: Option<String>,
}

/// Request to run a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSandboxRequest {
    /// Base64-encoded input data.
    #[serde(default)]
    pub input_base64: Option<String>,
    /// Whether to stream output via SSE.
    #[serde(default)]
    pub stream: bool,
}

/// Response from running a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSandboxResponse {
    /// Exit code.
    pub exit_code: i32,
    /// Stdout output (UTF-8 string).
    pub stdout: String,
    /// Stderr output (UTF-8 string).
    pub stderr: String,
    /// Execution duration in milliseconds.
    pub duration_ms: f64,
    /// Resource usage summary.
    pub resource_usage: ResourceUsageSummary,
}

/// Summary of resource usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsageSummary {
    /// Fuel consumed.
    pub fuel_consumed: Option<u64>,
    /// Peak memory in bytes.
    pub peak_memory_bytes: Option<u64>,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Total bytes written.
    pub bytes_written: u64,
}

/// Information about a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    /// Sandbox ID.
    pub id: String,
    /// Current state.
    pub state: String,
    /// Module hash.
    pub module_hash: String,
    /// When the sandbox was created (ISO 8601).
    pub created_at: String,
    /// Granted capabilities.
    pub capabilities: Vec<String>,
    /// Age in seconds.
    pub age_secs: f64,
}

/// Response for listing sandboxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSandboxesResponse {
    /// List of sandbox summaries.
    pub sandboxes: Vec<SandboxInfo>,
    /// Total count.
    pub total: usize,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Service version.
    pub version: String,
    /// Uptime in seconds.
    pub uptime_secs: f64,
    /// Number of active sandboxes.
    pub active_sandboxes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_constructors() {
        let err = ApiError::bad_request("invalid input");
        assert_eq!(err.status, 400);
        assert_eq!(err.code, "bad_request");

        let err = ApiError::not_found("sandbox xyz");
        assert_eq!(err.status, 404);

        let err = ApiError::rate_limited(60);
        assert_eq!(err.status, 429);

        let err = ApiError::internal("oops");
        assert_eq!(err.status, 500);
    }

    #[test]
    fn test_api_response_ok() {
        let resp = ApiResponse::ok("hello");
        assert!(resp.success);
        assert!(resp.data.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_api_response_err() {
        let resp: ApiResponse<String> = ApiResponse::err(ApiError::bad_request("fail"));
        assert!(!resp.success);
        assert!(resp.data.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_create_request_serde() {
        let json = r#"{
            "module_base64": "AGFzbQEAAAA=",
            "capabilities": ["stdio:stdout"],
            "memory_limit": 134217728,
            "fuel": 1000000,
            "timeout_secs": 30,
            "env": {"KEY": "value"},
            "args": ["--verbose"]
        }"#;

        let req: CreateSandboxRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.module_base64, "AGFzbQEAAAA=");
        assert_eq!(req.capabilities, vec!["stdio:stdout"]);
        assert_eq!(req.memory_limit, Some(134217728));
    }

    #[test]
    fn test_run_response_serde() {
        let resp = RunSandboxResponse {
            exit_code: 0,
            stdout: "hello".into(),
            stderr: String::new(),
            duration_ms: 42.5,
            resource_usage: ResourceUsageSummary {
                fuel_consumed: Some(1000),
                peak_memory_bytes: Some(1024),
                bytes_read: 0,
                bytes_written: 5,
            },
        };

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: RunSandboxResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.exit_code, 0);
        assert_eq!(parsed.stdout, "hello");
    }
}
