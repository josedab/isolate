//! gRPC authentication and authorization interceptor.
//!
//! Supports API-key authentication via the `x-api-key` metadata header and
//! optional role-based access control via the `x-role` header.
//!
//! Roles: `admin` (all ops), `operator` (create/run/terminate), `viewer` (read-only).
//! When RBAC is disabled, all authenticated requests have full access.

use std::collections::HashMap;
use tonic::{service::Interceptor, Request, Status};

/// Access role for RBAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Role {
    /// Full access to all operations.
    Admin,
    /// Can create, run, terminate sandboxes.
    Operator,
    /// Read-only: get, list, metrics only.
    Viewer,
}

impl Role {
    #[allow(dead_code)]
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Role::Admin),
            "operator" => Some(Role::Operator),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }

    /// Check if this role can perform the given gRPC method.
    pub fn can_access(&self, method: &str) -> bool {
        match self {
            Role::Admin => true,
            Role::Operator => !method.contains("GetMetrics"),
            Role::Viewer => {
                method.contains("GetSandbox")
                    || method.contains("ListSandboxes")
                    || method.contains("GetMetrics")
                    || method.contains("StreamOutput")
            }
        }
    }
}

/// API-key based gRPC interceptor with optional RBAC.
#[derive(Clone)]
pub struct AuthInterceptor {
    /// Map of API key → role. If empty, all requests are allowed.
    api_keys: HashMap<String, Role>,
    /// Whether RBAC is enabled. When false, any valid key gets Admin.
    rbac_enabled: bool,
}

impl AuthInterceptor {
    /// Create a new interceptor with a single API key (no RBAC).
    pub fn new(api_key: Option<String>) -> Self {
        let mut api_keys = HashMap::new();
        if let Some(key) = api_key {
            api_keys.insert(key, Role::Admin);
        }
        Self { api_keys, rbac_enabled: false }
    }

    /// Create an interceptor with multiple keys mapped to roles (RBAC enabled).
    #[allow(dead_code)]
    pub fn with_rbac(api_keys: HashMap<String, Role>) -> Self {
        let rbac_enabled = !api_keys.is_empty();
        Self { api_keys, rbac_enabled }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        if self.api_keys.is_empty() {
            return Ok(req);
        }

        let provided = req.metadata().get("x-api-key").and_then(|v| v.to_str().ok());

        let Some(key) = provided else {
            return Err(Status::unauthenticated("missing x-api-key header"));
        };

        // Find matching key (constant-time comparison for each candidate)
        let role = self
            .api_keys
            .iter()
            .find(|(k, _)| constant_time_eq(k.as_bytes(), key.as_bytes()))
            .map(|(_, r)| *r);

        let Some(role) = role else {
            return Err(Status::unauthenticated("invalid API key"));
        };

        // RBAC check: extract method from request URI
        if self.rbac_enabled {
            let method = req.metadata().get("x-method").and_then(|v| v.to_str().ok());
            if let Some(method) = method {
                if !role.can_access(method) {
                    return Err(Status::permission_denied(format!(
                        "{role:?} role cannot access {method}"
                    )));
                }
            }
        }

        Ok(req)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_key_allows_all() {
        let mut interceptor = AuthInterceptor::new(None);
        let req = Request::new(());
        assert!(interceptor.call(req).is_ok());
    }

    #[test]
    fn test_valid_key_passes() {
        let mut interceptor = AuthInterceptor::new(Some("secret".into()));
        let mut req = Request::new(());
        req.metadata_mut().insert("x-api-key", "secret".parse().unwrap());
        assert!(interceptor.call(req).is_ok());
    }

    #[test]
    fn test_invalid_key_rejected() {
        let mut interceptor = AuthInterceptor::new(Some("secret".into()));
        let mut req = Request::new(());
        req.metadata_mut().insert("x-api-key", "wrong".parse().unwrap());
        let err = interceptor.call(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn test_missing_key_rejected() {
        let mut interceptor = AuthInterceptor::new(Some("secret".into()));
        let req = Request::new(());
        let err = interceptor.call(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn test_rbac_viewer_can_list() {
        let mut keys = HashMap::new();
        keys.insert("viewer-key".to_string(), Role::Viewer);
        let mut interceptor = AuthInterceptor::with_rbac(keys);

        let mut req = Request::new(());
        req.metadata_mut().insert("x-api-key", "viewer-key".parse().unwrap());
        req.metadata_mut().insert("x-method", "ListSandboxes".parse().unwrap());
        assert!(interceptor.call(req).is_ok());
    }

    #[test]
    fn test_rbac_viewer_cannot_create() {
        let mut keys = HashMap::new();
        keys.insert("viewer-key".to_string(), Role::Viewer);
        let mut interceptor = AuthInterceptor::with_rbac(keys);

        let mut req = Request::new(());
        req.metadata_mut().insert("x-api-key", "viewer-key".parse().unwrap());
        req.metadata_mut().insert("x-method", "CreateSandbox".parse().unwrap());
        let err = interceptor.call(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn test_role_permissions() {
        assert!(Role::Admin.can_access("CreateSandbox"));
        assert!(Role::Admin.can_access("GetMetrics"));
        assert!(Role::Operator.can_access("CreateSandbox"));
        assert!(Role::Operator.can_access("RunSandbox"));
        assert!(!Role::Viewer.can_access("CreateSandbox"));
        assert!(Role::Viewer.can_access("ListSandboxes"));
        assert!(Role::Viewer.can_access("GetMetrics"));
    }
}
