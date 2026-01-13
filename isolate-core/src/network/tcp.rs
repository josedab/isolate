//! TCP connection management with policy enforcement.

use super::policy::{
    ConnectionCounter, NetworkPolicy, PolicyAction, PolicyAuditLog, PolicyDecision, RateLimiter,
};
use crate::error::{Error, Result};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

/// Configuration for TCP connections.
#[derive(Debug, Clone)]
pub struct TcpConnectionConfig {
    /// Maximum concurrent connections.
    pub max_connections: usize,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Read timeout per operation.
    pub read_timeout: Duration,
    /// Write timeout per operation.
    pub write_timeout: Duration,
    /// Maximum bandwidth per connection (bytes/sec, 0 = unlimited).
    pub max_bandwidth: u64,
    /// Whether to require TLS.
    pub require_tls: bool,
}

impl Default for TcpConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            max_bandwidth: 0,
            require_tls: true,
        }
    }
}

/// Statistics for a single TCP connection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionStats {
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Connection duration.
    pub duration: Duration,
    /// Remote address.
    pub remote_addr: String,
}

/// Managed TCP connection pool with policy enforcement.
pub struct TcpConnectionPool {
    /// Network policy.
    policy: NetworkPolicy,
    /// Connection configuration.
    config: TcpConnectionConfig,
    /// Active connection counter.
    connections: ConnectionCounter,
    /// Rate limiter for new connections.
    rate_limiter: Option<RateLimiter>,
    /// Audit log.
    audit_log: Arc<PolicyAuditLog>,
    /// Per-connection statistics.
    stats: Mutex<HashMap<u64, ConnectionStats>>,
    /// Total connections ever created.
    total_created: AtomicU64,
    /// Total connections denied.
    total_denied: AtomicU64,
}

impl TcpConnectionPool {
    /// Create a new TCP connection pool.
    pub fn new(policy: NetworkPolicy, config: TcpConnectionConfig) -> Self {
        let rate_limiter = policy.rate_limit().map(|rl| {
            RateLimiter::new(super::policy::RateLimitConfig {
                max_requests: rl.max_requests,
                window: rl.window,
            })
        });

        let max_conn = config.max_connections.min(policy.max_connections());
        Self {
            policy,
            config,
            connections: ConnectionCounter::new(max_conn),
            rate_limiter,
            audit_log: Arc::new(PolicyAuditLog::default()),
            stats: Mutex::new(HashMap::new()),
            total_created: AtomicU64::new(0),
            total_denied: AtomicU64::new(0),
        }
    }

    /// Request a connection to the given address.
    pub async fn connect(&self, host: &str, port: u16) -> Result<ConnectionGuard> {
        // Check policy
        if !self.policy.allows_tcp(host, port) {
            self.total_denied.fetch_add(1, Ordering::Relaxed);
            self.audit_log.record(PolicyDecision {
                operation: "tcp_connect".to_string(),
                target: format!("{}:{}", host, port),
                action: PolicyAction::Deny,
                matched_rule: None,
                timestamp: chrono::Utc::now(),
            });
            return Err(Error::NetworkAccessDenied {
                host: format!("TCP connection to {}:{} denied by policy", host, port),
            });
        }

        // Check TLS requirement
        if self.config.require_tls && port != 443 && port != 8443 {
            return Err(Error::NetworkAccessDenied {
                host: format!("TLS required but port {} is not a standard TLS port", port),
            });
        }

        // Check rate limit
        if let Some(ref limiter) = self.rate_limiter {
            if !limiter.check() {
                self.total_denied.fetch_add(1, Ordering::Relaxed);
                return Err(Error::NetworkAccessDenied {
                    host: "Connection rate limit exceeded".to_string(),
                });
            }
        }

        // Check connection limit
        if !self.connections.acquire() {
            self.total_denied.fetch_add(1, Ordering::Relaxed);
            return Err(Error::NetworkAccessDenied {
                host: format!(
                    "Maximum concurrent connections ({}) exceeded",
                    self.config.max_connections
                ),
            });
        }

        let conn_id = self.total_created.fetch_add(1, Ordering::Relaxed);

        self.audit_log.record(PolicyDecision {
            operation: "tcp_connect".to_string(),
            target: format!("{}:{}", host, port),
            action: PolicyAction::Allow,
            matched_rule: None,
            timestamp: chrono::Utc::now(),
        });

        self.stats.lock().insert(
            conn_id,
            ConnectionStats { remote_addr: format!("{}:{}", host, port), ..Default::default() },
        );

        Ok(ConnectionGuard { id: conn_id, host: host.to_string(), port })
    }

    /// Release a connection (called when ConnectionGuard is dropped).
    pub fn release(&self, id: u64) {
        self.connections.release();
        self.stats.lock().remove(&id);
    }

    /// Get current number of active connections.
    pub fn active_connections(&self) -> u64 {
        self.connections.current()
    }

    /// Get total connections created.
    pub fn total_created(&self) -> u64 {
        self.total_created.load(Ordering::Relaxed)
    }

    /// Get total connections denied.
    pub fn total_denied(&self) -> u64 {
        self.total_denied.load(Ordering::Relaxed)
    }

    /// Get the audit log.
    pub fn audit_log(&self) -> &PolicyAuditLog {
        &self.audit_log
    }

    /// Get pool configuration.
    pub fn config(&self) -> &TcpConnectionConfig {
        &self.config
    }
}

/// RAII guard for a TCP connection slot.
pub struct ConnectionGuard {
    /// Connection ID.
    pub id: u64,
    /// Remote host.
    pub host: String,
    /// Remote port.
    pub port: u16,
}

impl ConnectionGuard {
    /// Get the remote address string.
    pub fn remote_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_config_defaults() {
        let config = TcpConnectionConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert!(config.require_tls);
    }

    #[tokio::test]
    async fn test_pool_policy_deny() {
        let policy = NetworkPolicy::default(); // Default deny
        let config = TcpConnectionConfig::default();
        let pool = TcpConnectionPool::new(policy, config);

        let result = pool.connect("example.com", 443).await;
        assert!(result.is_err());
        assert_eq!(pool.total_denied(), 1);
    }

    #[tokio::test]
    async fn test_pool_connection_limit() {
        let policy = NetworkPolicy::builder().allow_http("*").allow_port(443).build();
        let config =
            TcpConnectionConfig { max_connections: 2, require_tls: false, ..Default::default() };
        let pool = TcpConnectionPool::new(policy, config);

        let _conn1 = pool.connect("a.com", 443).await.unwrap();
        let _conn2 = pool.connect("b.com", 443).await.unwrap();
        let result = pool.connect("c.com", 443).await;
        assert!(result.is_err()); // Connection limit reached
    }

    #[tokio::test]
    async fn test_pool_release() {
        let policy = NetworkPolicy::builder().allow_http("*").allow_port(443).build();
        let config =
            TcpConnectionConfig { max_connections: 1, require_tls: false, ..Default::default() };
        let pool = TcpConnectionPool::new(policy, config);

        let conn = pool.connect("a.com", 443).await.unwrap();
        assert_eq!(pool.active_connections(), 1);

        pool.release(conn.id);
        assert_eq!(pool.active_connections(), 0);

        // Can connect again after release
        let _conn2 = pool.connect("b.com", 443).await.unwrap();
        assert_eq!(pool.active_connections(), 1);
    }
}
