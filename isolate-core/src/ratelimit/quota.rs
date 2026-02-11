//! Time-windowed quota enforcement.

use crate::error::{Error, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Quota configuration for time-windowed limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// Maximum sandbox executions per hour. None means unlimited.
    pub max_executions_per_hour: Option<u64>,
    /// Maximum bandwidth (bytes transferred) per hour. None means unlimited.
    pub max_bandwidth_bytes_per_hour: Option<u64>,
}

impl QuotaConfig {
    /// Create a quota config with execution limit.
    pub fn with_executions(max_per_hour: u64) -> Self {
        Self { max_executions_per_hour: Some(max_per_hour), ..Default::default() }
    }

    /// Create a quota config with bandwidth limit.
    pub fn with_bandwidth(max_bytes_per_hour: u64) -> Self {
        Self { max_bandwidth_bytes_per_hour: Some(max_bytes_per_hour), ..Default::default() }
    }
}

/// Enforces quotas using sliding window counters.
pub struct QuotaEnforcer {
    config: QuotaConfig,
    state: Mutex<QuotaState>,
}

struct QuotaState {
    window_start: Instant,
    executions: u64,
    bandwidth_bytes: u64,
}

impl QuotaEnforcer {
    const WINDOW_DURATION: Duration = Duration::from_secs(3600); // 1 hour

    /// Create a new quota enforcer.
    pub fn new(config: QuotaConfig) -> Self {
        Self {
            config,
            state: Mutex::new(QuotaState {
                window_start: Instant::now(),
                executions: 0,
                bandwidth_bytes: 0,
            }),
        }
    }

    /// Check if an execution is allowed under the quota.
    pub fn check_execution(&self) -> Result<()> {
        let mut state = self.state.lock();
        self.maybe_reset_window(&mut state);

        if let Some(max) = self.config.max_executions_per_hour {
            if state.executions >= max {
                return Err(Error::Execution(format!(
                    "Execution quota exceeded: {}/{} per hour",
                    state.executions, max
                )));
            }
        }
        Ok(())
    }

    /// Record a completed execution.
    pub fn record_execution(&self) {
        let mut state = self.state.lock();
        self.maybe_reset_window(&mut state);
        state.executions += 1;
    }

    /// Record bandwidth usage and check quota.
    pub fn record_bandwidth(&self, bytes: u64) -> Result<()> {
        let mut state = self.state.lock();
        self.maybe_reset_window(&mut state);

        let new_total = state.bandwidth_bytes.saturating_add(bytes);
        if let Some(max) = self.config.max_bandwidth_bytes_per_hour {
            if new_total > max {
                return Err(Error::Execution(format!(
                    "Bandwidth quota exceeded: {} > {} bytes per hour",
                    new_total, max
                )));
            }
        }
        state.bandwidth_bytes = new_total;
        Ok(())
    }

    /// Get current quota status.
    pub fn status(&self) -> QuotaStatus {
        let state = self.state.lock();
        QuotaStatus {
            executions_used: state.executions,
            executions_limit: self.config.max_executions_per_hour,
            bandwidth_used: state.bandwidth_bytes,
            bandwidth_limit: self.config.max_bandwidth_bytes_per_hour,
            window_remaining: Self::WINDOW_DURATION
                .checked_sub(state.window_start.elapsed())
                .unwrap_or(Duration::ZERO),
        }
    }

    fn maybe_reset_window(&self, state: &mut QuotaState) {
        if state.window_start.elapsed() >= Self::WINDOW_DURATION {
            state.window_start = Instant::now();
            state.executions = 0;
            state.bandwidth_bytes = 0;
        }
    }
}

/// Current quota usage status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    /// Executions used in current window.
    pub executions_used: u64,
    /// Execution limit (if set).
    pub executions_limit: Option<u64>,
    /// Bandwidth used in current window (bytes).
    pub bandwidth_used: u64,
    /// Bandwidth limit (if set, bytes).
    pub bandwidth_limit: Option<u64>,
    /// Time remaining in current window.
    pub window_remaining: Duration,
}

/// Usage snapshot for reporting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Total executions tracked.
    pub total_executions: u64,
    /// Total bandwidth tracked (bytes).
    pub total_bandwidth: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_enforcer_execution_limit() {
        let config = QuotaConfig::with_executions(3);
        let enforcer = QuotaEnforcer::new(config);

        // Allow up to 3 executions
        for _ in 0..3 {
            assert!(enforcer.check_execution().is_ok());
            enforcer.record_execution();
        }

        // 4th should fail
        assert!(enforcer.check_execution().is_err());
    }

    #[test]
    fn test_quota_enforcer_bandwidth_limit() {
        let config = QuotaConfig::with_bandwidth(1000);
        let enforcer = QuotaEnforcer::new(config);

        assert!(enforcer.record_bandwidth(500).is_ok());
        assert!(enforcer.record_bandwidth(400).is_ok());
        assert!(enforcer.record_bandwidth(200).is_err()); // 1100 > 1000
    }

    #[test]
    fn test_quota_status() {
        let config = QuotaConfig {
            max_executions_per_hour: Some(100),
            max_bandwidth_bytes_per_hour: Some(1024),
        };
        let enforcer = QuotaEnforcer::new(config);

        enforcer.record_execution();
        enforcer.record_execution();
        enforcer.record_bandwidth(256).unwrap();

        let status = enforcer.status();
        assert_eq!(status.executions_used, 2);
        assert_eq!(status.executions_limit, Some(100));
        assert_eq!(status.bandwidth_used, 256);
        assert!(status.window_remaining.as_secs() > 3500);
    }

    #[test]
    fn test_quota_no_limits() {
        let config = QuotaConfig::default();
        let enforcer = QuotaEnforcer::new(config);

        // Everything should pass with no limits
        for _ in 0..1000 {
            assert!(enforcer.check_execution().is_ok());
            enforcer.record_execution();
        }
        assert!(enforcer.record_bandwidth(u64::MAX / 2).is_ok());
    }
}
