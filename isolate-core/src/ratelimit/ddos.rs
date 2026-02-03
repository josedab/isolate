//! DDoS protection with IP reputation scoring and adaptive rate limiting.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// IP reputation score (0 = malicious, 100 = trusted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpReputation {
    pub ip: String,
    pub score: u32,
    pub total_requests: u64,
    pub blocked_requests: u64,
    pub last_seen_epoch_ms: u64,
}

/// Configuration for DDoS protection.
#[derive(Debug, Clone)]
pub struct DdosConfig {
    /// Requests per second threshold for triggering protection.
    pub rps_threshold: u64,
    /// Score below which an IP is auto-blocked.
    pub block_score_threshold: u32,
    /// Score penalty for each blocked request.
    pub penalty_per_block: u32,
    /// Score recovery per successful request.
    pub recovery_per_success: u32,
    /// Initial score for unknown IPs.
    pub initial_score: u32,
    /// Temporary ban duration for blocked IPs.
    pub ban_duration: Duration,
}

impl Default for DdosConfig {
    fn default() -> Self {
        Self {
            rps_threshold: 1000,
            block_score_threshold: 20,
            penalty_per_block: 10,
            recovery_per_success: 1,
            initial_score: 80,
            ban_duration: Duration::from_secs(300),
        }
    }
}

struct IpState {
    score: u32,
    total_requests: u64,
    blocked_requests: u64,
    banned_until: Option<Instant>,
    last_seen: Instant,
}

/// DDoS protection engine with IP reputation tracking.
pub struct DdosProtection {
    config: DdosConfig,
    ips: dashmap::DashMap<String, IpState>,
    total_blocked: AtomicU64,
    total_allowed: AtomicU64,
}

impl DdosProtection {
    pub fn new(config: DdosConfig) -> Self {
        Self {
            config,
            ips: dashmap::DashMap::new(),
            total_blocked: AtomicU64::new(0),
            total_allowed: AtomicU64::new(0),
        }
    }

    /// Check if a request from an IP should be allowed.
    pub fn check_ip(&self, ip: &str) -> bool {
        let mut entry = self
            .ips
            .entry(ip.to_string())
            .or_insert_with(|| IpState {
                score: self.config.initial_score,
                total_requests: 0,
                blocked_requests: 0,
                banned_until: None,
                last_seen: Instant::now(),
            });

        let state = entry.value_mut();
        state.total_requests += 1;
        state.last_seen = Instant::now();

        // Check temporary ban
        if let Some(until) = state.banned_until {
            if Instant::now() < until {
                state.blocked_requests += 1;
                self.total_blocked.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            state.banned_until = None;
        }

        // Check score threshold
        if state.score < self.config.block_score_threshold {
            state.blocked_requests += 1;
            state.banned_until = Some(Instant::now() + self.config.ban_duration);
            self.total_blocked.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        self.total_allowed.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Record a successful request (improves reputation).
    pub fn record_success(&self, ip: &str) {
        if let Some(mut entry) = self.ips.get_mut(ip) {
            let score = entry.score + self.config.recovery_per_success;
            entry.score = score.min(100);
        }
    }

    /// Record a suspicious/bad request (degrades reputation).
    pub fn record_violation(&self, ip: &str) {
        if let Some(mut entry) = self.ips.get_mut(ip) {
            entry.score = entry.score.saturating_sub(self.config.penalty_per_block);
        }
    }

    /// Get reputation info for an IP.
    pub fn get_reputation(&self, ip: &str) -> Option<IpReputation> {
        self.ips.get(ip).map(|e| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            IpReputation {
                ip: ip.to_string(),
                score: e.score,
                total_requests: e.total_requests,
                blocked_requests: e.blocked_requests,
                last_seen_epoch_ms: now,
            }
        })
    }

    /// Manually whitelist an IP (set score to 100).
    pub fn whitelist(&self, ip: &str) {
        if let Some(mut entry) = self.ips.get_mut(ip) {
            entry.score = 100;
            entry.banned_until = None;
        }
    }

    /// Manually blacklist an IP (set score to 0 and ban).
    pub fn blacklist(&self, ip: &str) {
        let mut entry = self
            .ips
            .entry(ip.to_string())
            .or_insert_with(|| IpState {
                score: 0,
                total_requests: 0,
                blocked_requests: 0,
                banned_until: None,
                last_seen: Instant::now(),
            });
        entry.score = 0;
        entry.banned_until = Some(Instant::now() + self.config.ban_duration);
    }

    /// Total blocked requests across all IPs.
    pub fn total_blocked(&self) -> u64 {
        self.total_blocked.load(Ordering::Relaxed)
    }

    /// Total allowed requests across all IPs.
    pub fn total_allowed(&self) -> u64 {
        self.total_allowed.load(Ordering::Relaxed)
    }

    /// Number of tracked IPs.
    pub fn tracked_ips(&self) -> usize {
        self.ips.len()
    }
}

impl Default for DdosProtection {
    fn default() -> Self {
        Self::new(DdosConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_traffic_allowed() {
        let ddos = DdosProtection::new(DdosConfig::default());
        assert!(ddos.check_ip("192.168.1.1"));
        assert_eq!(ddos.total_allowed(), 1);
    }

    #[test]
    fn test_reputation_degradation() {
        let ddos = DdosProtection::new(DdosConfig {
            initial_score: 30,
            penalty_per_block: 10,
            block_score_threshold: 20,
            ..Default::default()
        });

        ddos.check_ip("10.0.0.1");
        ddos.record_violation("10.0.0.1");
        let rep = ddos.get_reputation("10.0.0.1").unwrap();
        assert_eq!(rep.score, 20); // 30 - 10

        ddos.record_violation("10.0.0.1");
        let rep = ddos.get_reputation("10.0.0.1").unwrap();
        assert_eq!(rep.score, 10); // below threshold

        assert!(!ddos.check_ip("10.0.0.1"));
    }

    #[test]
    fn test_reputation_recovery() {
        let ddos = DdosProtection::new(DdosConfig {
            initial_score: 50,
            recovery_per_success: 5,
            ..Default::default()
        });
        ddos.check_ip("10.0.0.1");
        ddos.record_success("10.0.0.1");
        let rep = ddos.get_reputation("10.0.0.1").unwrap();
        assert_eq!(rep.score, 55);
    }

    #[test]
    fn test_score_capped_at_100() {
        let ddos = DdosProtection::new(DdosConfig {
            initial_score: 95,
            recovery_per_success: 10,
            ..Default::default()
        });
        ddos.check_ip("10.0.0.1");
        ddos.record_success("10.0.0.1");
        let rep = ddos.get_reputation("10.0.0.1").unwrap();
        assert_eq!(rep.score, 100);
    }

    #[test]
    fn test_blacklist() {
        let ddos = DdosProtection::new(DdosConfig::default());
        ddos.check_ip("10.0.0.1");
        ddos.blacklist("10.0.0.1");
        assert!(!ddos.check_ip("10.0.0.1"));
    }

    #[test]
    fn test_whitelist() {
        let ddos = DdosProtection::new(DdosConfig {
            initial_score: 5,
            block_score_threshold: 20,
            ..Default::default()
        });
        ddos.check_ip("10.0.0.1");
        assert!(!ddos.check_ip("10.0.0.1")); // blocked due to low score

        ddos.whitelist("10.0.0.1");
        assert!(ddos.check_ip("10.0.0.1")); // whitelisted
    }

    #[test]
    fn test_tracked_ips() {
        let ddos = DdosProtection::new(DdosConfig::default());
        ddos.check_ip("1.1.1.1");
        ddos.check_ip("2.2.2.2");
        ddos.check_ip("3.3.3.3");
        assert_eq!(ddos.tracked_ips(), 3);
    }

    #[test]
    fn test_unknown_ip_reputation() {
        let ddos = DdosProtection::new(DdosConfig::default());
        assert!(ddos.get_reputation("unknown").is_none());
    }
}
