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
    /// Map of API key → role. If empty, auth is disabled with a warning logged at startup.
    api_keys: HashMap<String, Role>,
    /// Whether RBAC is enabled. When false, any valid key gets Admin.
    rbac_enabled: bool,
    /// Whether auth is explicitly disabled (no keys configured).
    auth_disabled: bool,
}

impl AuthInterceptor {
    /// Create a new interceptor with a single API key (no RBAC).
    ///
    /// When `api_key` is None, authentication is disabled — all requests
    /// are allowed. This is intentional for development but should never
    /// be used in production.
    pub fn new(api_key: Option<String>) -> Self {
        let mut api_keys = HashMap::new();
        let auth_disabled = api_key.is_none();
        if let Some(key) = api_key {
            api_keys.insert(key, Role::Admin);
        }
        Self { api_keys, rbac_enabled: false, auth_disabled }
    }

    /// Create an interceptor with multiple keys mapped to roles (RBAC enabled).
    #[allow(dead_code)]
    pub fn with_rbac(api_keys: HashMap<String, Role>) -> Self {
        let rbac_enabled = !api_keys.is_empty();
        Self { api_keys, rbac_enabled, auth_disabled: false }
    }

    /// Returns true if authentication is disabled (no keys configured).
    #[allow(dead_code)]
    pub fn is_auth_disabled(&self) -> bool {
        self.auth_disabled
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        // When auth is explicitly disabled (no keys configured), allow all.
        // This is intentional for development; in production, configure keys.
        if self.auth_disabled {
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

        // RBAC check: extract method from request metadata
        if self.rbac_enabled {
            let method = req.metadata().get("x-method").and_then(|v| v.to_str().ok());
            match method {
                Some(method) => {
                    if !role.can_access(method) {
                        return Err(Status::permission_denied(format!(
                            "{role:?} role cannot access {method}"
                        )));
                    }
                }
                None => {
                    return Err(Status::permission_denied(
                        "x-method header required when RBAC is enabled",
                    ));
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

    #[test]
    fn test_rbac_missing_method_header_rejected() {
        let mut keys = HashMap::new();
        keys.insert("key".to_string(), Role::Admin);
        let mut interceptor = AuthInterceptor::with_rbac(keys);

        let mut req = Request::new(());
        req.metadata_mut().insert("x-api-key", "key".parse().unwrap());
        // No x-method header
        let err = interceptor.call(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn test_auth_disabled_flag() {
        let interceptor = AuthInterceptor::new(None);
        assert!(interceptor.is_auth_disabled());

        let interceptor = AuthInterceptor::new(Some("key".into()));
        assert!(!interceptor.is_auth_disabled());
    }

    #[test]
    fn test_with_rbac_empty_keys_rejects() {
        let mut interceptor = AuthInterceptor::with_rbac(HashMap::new());
        let req = Request::new(());
        // Empty RBAC keys means no valid key exists — auth not disabled, so reject
        let err = interceptor.call(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }
}
