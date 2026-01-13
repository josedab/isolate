//! DNS resolution with policy enforcement.

use super::policy::{NetworkPolicy, PolicyAction, PolicyAuditLog, PolicyDecision};
use crate::error::{Error, Result};

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// DNS resolution result.
#[derive(Debug, Clone)]
pub struct DnsRecord {
    /// The queried domain.
    pub domain: String,
    /// Resolved IP addresses.
    pub addresses: Vec<IpAddr>,
    /// Time-to-live in seconds.
    pub ttl: u64,
    /// When this record was resolved.
    pub resolved_at: Instant,
}

impl DnsRecord {
    /// Check if the record has expired.
    pub fn is_expired(&self) -> bool {
        self.resolved_at.elapsed() > Duration::from_secs(self.ttl)
    }
}

/// DNS resolver with caching and policy enforcement.
pub struct DnsResolver {
    /// Cache of resolved domains.
    cache: Arc<RwLock<HashMap<String, DnsRecord>>>,
    /// Network policy for access control.
    policy: NetworkPolicy,
    /// Audit log for DNS decisions.
    audit_log: Arc<PolicyAuditLog>,
    /// Maximum cache entries.
    max_cache_entries: usize,
    /// Default TTL for cache entries.
    default_ttl: u64,
}

impl DnsResolver {
    /// Create a new DNS resolver with the given policy.
    pub fn new(policy: NetworkPolicy) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            policy,
            audit_log: Arc::new(PolicyAuditLog::default()),
            max_cache_entries: 1000,
            default_ttl: 300, // 5 minutes
        }
    }

    /// Set the maximum cache entries.
    pub fn with_max_cache(mut self, max: usize) -> Self {
        self.max_cache_entries = max;
        self
    }

    /// Set the default TTL.
    pub fn with_default_ttl(mut self, ttl: u64) -> Self {
        self.default_ttl = ttl;
        self
    }

    /// Resolve a domain name, enforcing policy.
    pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
        // Check policy
        if !self.policy.allows_dns(domain) {
            self.audit_log.record(PolicyDecision {
                operation: "dns_resolve".to_string(),
                target: domain.to_string(),
                action: PolicyAction::Deny,
                matched_rule: None,
                timestamp: chrono::Utc::now(),
            });
            return Err(Error::NetworkAccessDenied {
                host: format!("DNS resolution denied for: {}", domain),
            });
        }

        // Check cache
        {
            let cache = self.cache.read();
            if let Some(record) = cache.get(domain) {
                if !record.is_expired() {
                    return Ok(record.addresses.clone());
                }
            }
        }

        // Perform actual resolution using tokio's DNS resolver
        let addrs = tokio::net::lookup_host(format!("{}:0", domain))
            .await
            .map_err(|e| Error::NetworkAccessDenied {
                host: format!("DNS resolution failed for {}: {}", domain, e),
            })?
            .map(|addr| addr.ip())
            .collect::<Vec<_>>();

        // Validate resolved IPs against policy (prevent DNS rebinding)
        for ip in &addrs {
            if !self.policy.allows_ip(ip) {
                self.audit_log.record(PolicyDecision {
                    operation: "dns_rebinding_check".to_string(),
                    target: format!("{} -> {}", domain, ip),
                    action: PolicyAction::Deny,
                    matched_rule: Some("ip_policy".to_string()),
                    timestamp: chrono::Utc::now(),
                });
                return Err(Error::NetworkAccessDenied {
                    host: format!(
                        "DNS rebinding detected: {} resolved to blocked IP {}",
                        domain, ip
                    ),
                });
            }
        }

        // Cache the result
        {
            let mut cache = self.cache.write();
            if cache.len() >= self.max_cache_entries {
                // Evict expired entries first
                cache.retain(|_, v| !v.is_expired());
            }
            if cache.len() < self.max_cache_entries {
                cache.insert(
                    domain.to_string(),
                    DnsRecord {
                        domain: domain.to_string(),
                        addresses: addrs.clone(),
                        ttl: self.default_ttl,
                        resolved_at: Instant::now(),
                    },
                );
            }
        }

        self.audit_log.record(PolicyDecision {
            operation: "dns_resolve".to_string(),
            target: domain.to_string(),
            action: PolicyAction::Allow,
            matched_rule: None,
            timestamp: chrono::Utc::now(),
        });

        Ok(addrs)
    }

    /// Clear the DNS cache.
    pub fn clear_cache(&self) {
        self.cache.write().clear();
    }

    /// Get the number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.cache.read().len()
    }

    /// Get the audit log.
    pub fn audit_log(&self) -> &PolicyAuditLog {
        &self.audit_log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_record_expiry() {
        let record = DnsRecord {
            domain: "example.com".to_string(),
            addresses: vec!["93.184.216.34".parse().unwrap()],
            ttl: 0, // Expires immediately
            resolved_at: Instant::now() - Duration::from_secs(1),
        };
        assert!(record.is_expired());

        let record = DnsRecord {
            domain: "example.com".to_string(),
            addresses: vec!["93.184.216.34".parse().unwrap()],
            ttl: 3600,
            resolved_at: Instant::now(),
        };
        assert!(!record.is_expired());
    }

    #[test]
    fn test_resolver_creation() {
        let policy = NetworkPolicy::builder().allow_http("*.example.com").build();

        let resolver = DnsResolver::new(policy).with_max_cache(100).with_default_ttl(60);

        assert_eq!(resolver.cache_size(), 0);
    }

    #[tokio::test]
    async fn test_resolver_policy_deny() {
        let policy = NetworkPolicy::default(); // Default deny
        let resolver = DnsResolver::new(policy);

        let result = resolver.resolve("example.com").await;
        assert!(result.is_err());
    }
}
