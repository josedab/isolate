//! Per-sandbox rate limiting and quota management.
//!
//! Provides token bucket rate limiting and sliding window quota enforcement
//! for multi-tenant sandbox environments.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::ratelimit::{RateLimiter, RateLimitConfig, QuotaConfig};
//! use std::time::Duration;
//!
//! let config = RateLimitConfig {
//!     requests_per_second: Some(100),
//!     burst_size: Some(20),
//!     quota: Some(QuotaConfig {
//!         max_executions_per_hour: Some(1000),
//!         max_bandwidth_bytes_per_hour: Some(100 * 1024 * 1024),
//!     }),
//! };
//!
//! let limiter = RateLimiter::new(config);
//! assert!(limiter.try_acquire().is_ok());
//! ```

pub mod circuit_breaker;
pub mod ddos;
mod quota;
mod token_bucket;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use ddos::{DdosConfig, DdosProtection, IpReputation};
pub use quota::{QuotaConfig, QuotaEnforcer, QuotaStatus, QuotaUsage};
pub use token_bucket::TokenBucket;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration for rate limiting a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum sustained requests per second. None means unlimited.
    pub requests_per_second: Option<u32>,
    /// Burst size (tokens above the sustained rate). None uses requests_per_second.
    pub burst_size: Option<u32>,
    /// Quota configuration for time-windowed limits.
    pub quota: Option<QuotaConfig>,
}

impl RateLimitConfig {
    /// Create a rate limit config with the given sustained rate.
    pub fn with_rate(rps: u32) -> Self {
        Self { requests_per_second: Some(rps), burst_size: Some(rps), quota: None }
    }

    /// Set burst size.
    pub fn burst(mut self, burst: u32) -> Self {
        self.burst_size = Some(burst);
        self
    }

    /// Set quota configuration.
    pub fn with_quota(mut self, quota: QuotaConfig) -> Self {
        self.quota = Some(quota);
        self
    }

    /// Check if any rate limiting is configured.
    pub fn is_enabled(&self) -> bool {
        self.requests_per_second.is_some() || self.quota.is_some()
    }
}

/// Combined rate limiter with token bucket and quota enforcement.
pub struct RateLimiter {
    config: RateLimitConfig,
    bucket: Option<TokenBucket>,
    quota: Option<QuotaEnforcer>,
}

impl RateLimiter {
    /// Create a new rate limiter from configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        let bucket = config.requests_per_second.map(|rps| {
            let burst = config.burst_size.unwrap_or(rps);
            TokenBucket::new(rps as f64, burst as u64)
        });

        let quota = config.quota.as_ref().map(|q| QuotaEnforcer::new(q.clone()));

        Self { config, bucket, quota }
    }

    /// Try to acquire a permit for one request. Returns error if rate limited.
    ///
    /// Quota is checked BEFORE consuming a bucket token to prevent token loss
    /// when the quota would reject anyway.
    pub fn try_acquire(&self) -> Result<()> {
        // Check quota first (non-destructive check) — prevents wasting
        // bucket tokens on requests that will be rejected by quota.
        if let Some(ref quota) = self.quota {
            quota.check_execution()?;
        }

        if let Some(ref bucket) = self.bucket {
            if !bucket.try_acquire(1) {
                return Err(Error::Execution(
                    "Rate limit exceeded: too many requests per second".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Record bandwidth usage for quota tracking.
    pub fn record_bandwidth(&self, bytes: u64) -> Result<()> {
        if let Some(ref quota) = self.quota {
            quota.record_bandwidth(bytes)?;
        }
        Ok(())
    }

    /// Record a completed execution for quota tracking.
    pub fn record_execution(&self) {
        if let Some(ref quota) = self.quota {
            quota.record_execution();
        }
    }

    /// Get current quota usage status, if quotas are configured.
    pub fn quota_status(&self) -> Option<QuotaStatus> {
        self.quota.as_ref().map(|q| q.status())
    }

    /// Get the rate limit configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Check if rate limiting is active.
    pub fn is_enabled(&self) -> bool {
        self.config.is_enabled()
    }

    /// Get remaining tokens in the bucket (approximate).
    pub fn remaining_tokens(&self) -> Option<u64> {
        self.bucket.as_ref().map(|b| b.available_tokens())
    }
}

/// A shared rate limiter that can be cloned across threads.
#[derive(Clone)]
pub struct SharedRateLimiter {
    inner: Arc<RateLimiter>,
}

impl SharedRateLimiter {
    /// Create a new shared rate limiter.
    pub fn new(config: RateLimitConfig) -> Self {
        Self { inner: Arc::new(RateLimiter::new(config)) }
    }

    /// Try to acquire a permit.
    pub fn try_acquire(&self) -> Result<()> {
        self.inner.try_acquire()
    }

    /// Record bandwidth usage.
    pub fn record_bandwidth(&self, bytes: u64) -> Result<()> {
        self.inner.record_bandwidth(bytes)
    }

    /// Record a completed execution.
    pub fn record_execution(&self) {
        self.inner.record_execution();
    }

    /// Get quota status.
    pub fn quota_status(&self) -> Option<QuotaStatus> {
        self.inner.quota_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_no_config() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        assert!(!limiter.is_enabled());
        assert!(limiter.try_acquire().is_ok());
    }

    #[test]
    fn test_rate_limiter_with_rate() {
        let config = RateLimitConfig::with_rate(10);
        let limiter = RateLimiter::new(config);
        assert!(limiter.is_enabled());

        // Should succeed up to burst
        for _ in 0..10 {
            assert!(limiter.try_acquire().is_ok());
        }
        // Should fail after burst exhausted
        assert!(limiter.try_acquire().is_err());
    }

    #[test]
    fn test_rate_limiter_with_quota() {
        let config = RateLimitConfig {
            requests_per_second: None,
            burst_size: None,
            quota: Some(QuotaConfig {
                max_executions_per_hour: Some(5),
                max_bandwidth_bytes_per_hour: None,
            }),
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..5 {
            assert!(limiter.try_acquire().is_ok());
            limiter.record_execution();
        }
        assert!(limiter.try_acquire().is_err());
    }

    #[test]
    fn test_shared_rate_limiter() {
        let limiter = SharedRateLimiter::new(RateLimitConfig::with_rate(5));
        let limiter2 = limiter.clone();

        // Both clones share the same state
        for _ in 0..3 {
            assert!(limiter.try_acquire().is_ok());
        }
        for _ in 0..2 {
            assert!(limiter2.try_acquire().is_ok());
        }
        assert!(limiter.try_acquire().is_err());
    }
}
