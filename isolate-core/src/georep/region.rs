use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Unique identifier for a geographic region.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct RegionId(String);

impl RegionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RegionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Health metrics for a region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionHealth {
    pub region_id: RegionId,
    pub status: RegionStatus,
    pub latency_ms: u64,
    pub last_heartbeat_epoch_ms: u64,
    pub consecutive_failures: u32,
    pub total_requests: u64,
    pub error_count: u64,
}

/// Information about a registered region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionInfo {
    pub id: RegionId,
    pub is_primary: bool,
    pub status: RegionStatus,
    pub registered_epoch_ms: u64,
}

impl RegionInfo {
    pub fn new(id: impl Into<String>, is_primary: bool) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            id: RegionId::new(id),
            is_primary,
            status: RegionStatus::Healthy,
            registered_epoch_ms: now,
        }
    }
}

/// Registry of known regions with health tracking.
pub struct RegionRegistry {
    regions: dashmap::DashMap<RegionId, RegionInfo>,
    health: dashmap::DashMap<RegionId, RegionHealthState>,
}

struct RegionHealthState {
    consecutive_failures: u32,
    last_heartbeat: Option<Instant>,
    total_requests: u64,
    error_count: u64,
    last_latency_ms: u64,
}

impl RegionRegistry {
    pub fn new() -> Self {
        Self {
            regions: dashmap::DashMap::new(),
            health: dashmap::DashMap::new(),
        }
    }

    pub fn register(&self, info: RegionInfo) {
        let id = info.id.clone();
        self.regions.insert(id.clone(), info);
        self.health.insert(
            id,
            RegionHealthState {
                consecutive_failures: 0,
                last_heartbeat: Some(Instant::now()),
                total_requests: 0,
                error_count: 0,
                last_latency_ms: 0,
            },
        );
    }

    pub fn unregister(&self, id: &RegionId) {
        self.regions.remove(id);
        self.health.remove(id);
    }

    /// Record a successful heartbeat from a region.
    pub fn record_heartbeat(&self, id: &RegionId, latency_ms: u64) {
        if let Some(mut state) = self.health.get_mut(id) {
            state.consecutive_failures = 0;
            state.last_heartbeat = Some(Instant::now());
            state.last_latency_ms = latency_ms;
            state.total_requests += 1;
        }
        if let Some(mut info) = self.regions.get_mut(id) {
            info.status = RegionStatus::Healthy;
        }
    }

    /// Record a failed heartbeat from a region.
    pub fn record_failure(&self, id: &RegionId) {
        if let Some(mut state) = self.health.get_mut(id) {
            state.consecutive_failures += 1;
            state.error_count += 1;
            state.total_requests += 1;
        }
        if let Some(mut info) = self.regions.get_mut(id) {
            let failures = self
                .health
                .get(id)
                .map(|s| s.consecutive_failures)
                .unwrap_or(0);
            info.status = if failures >= 3 {
                RegionStatus::Unhealthy
            } else {
                RegionStatus::Degraded
            };
        }
    }

    /// Get health info for a region.
    pub fn get_health(&self, id: &RegionId) -> Option<RegionHealth> {
        let info = self.regions.get(id)?;
        let state = self.health.get(id)?;

        Some(RegionHealth {
            region_id: id.clone(),
            status: info.status,
            latency_ms: state.last_latency_ms,
            last_heartbeat_epoch_ms: state
                .last_heartbeat
                .map(|_| {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64
                })
                .unwrap_or(0),
            consecutive_failures: state.consecutive_failures,
            total_requests: state.total_requests,
            error_count: state.error_count,
        })
    }

    /// Get the primary region.
    pub fn primary(&self) -> Option<RegionInfo> {
        self.regions
            .iter()
            .find(|e| e.value().is_primary)
            .map(|e| e.value().clone())
    }

    /// List all healthy regions.
    pub fn list_healthy(&self) -> Vec<RegionInfo> {
        self.regions
            .iter()
            .filter(|e| matches!(e.value().status, RegionStatus::Healthy))
            .map(|e| e.value().clone())
            .collect()
    }

    /// List all regions.
    pub fn list_all(&self) -> Vec<RegionInfo> {
        self.regions.iter().map(|e| e.value().clone()).collect()
    }

    /// Number of registered regions.
    pub fn count(&self) -> usize {
        self.regions.len()
    }
}

impl Default for RegionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_regions() {
        let reg = RegionRegistry::new();
        reg.register(RegionInfo::new("us-east-1", true));
        reg.register(RegionInfo::new("eu-west-1", false));
        assert_eq!(reg.count(), 2);
    }

    #[test]
    fn test_primary_region() {
        let reg = RegionRegistry::new();
        reg.register(RegionInfo::new("us-east-1", true));
        reg.register(RegionInfo::new("eu-west-1", false));
        let primary = reg.primary().unwrap();
        assert_eq!(primary.id.as_str(), "us-east-1");
    }

    #[test]
    fn test_heartbeat_tracking() {
        let reg = RegionRegistry::new();
        reg.register(RegionInfo::new("us-east-1", true));
        reg.record_heartbeat(&RegionId::new("us-east-1"), 15);

        let health = reg.get_health(&RegionId::new("us-east-1")).unwrap();
        assert_eq!(health.status, RegionStatus::Healthy);
        assert_eq!(health.latency_ms, 15);
    }

    #[test]
    fn test_failure_tracking() {
        let reg = RegionRegistry::new();
        let id = RegionId::new("us-east-1");
        reg.register(RegionInfo::new("us-east-1", true));

        reg.record_failure(&id);
        let health = reg.get_health(&id).unwrap();
        assert_eq!(health.status, RegionStatus::Degraded);

        reg.record_failure(&id);
        reg.record_failure(&id);
        let health = reg.get_health(&id).unwrap();
        assert_eq!(health.status, RegionStatus::Unhealthy);
    }

    #[test]
    fn test_recovery_after_failure() {
        let reg = RegionRegistry::new();
        let id = RegionId::new("us-east-1");
        reg.register(RegionInfo::new("us-east-1", true));

        reg.record_failure(&id);
        reg.record_failure(&id);
        reg.record_failure(&id);
        assert_eq!(
            reg.get_health(&id).unwrap().status,
            RegionStatus::Unhealthy
        );

        reg.record_heartbeat(&id, 10);
        assert_eq!(reg.get_health(&id).unwrap().status, RegionStatus::Healthy);
    }

    #[test]
    fn test_list_healthy() {
        let reg = RegionRegistry::new();
        reg.register(RegionInfo::new("us-east-1", true));
        reg.register(RegionInfo::new("eu-west-1", false));

        let id = RegionId::new("eu-west-1");
        for _ in 0..3 {
            reg.record_failure(&id);
        }

        let healthy = reg.list_healthy();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id.as_str(), "us-east-1");
    }

    #[test]
    fn test_unregister() {
        let reg = RegionRegistry::new();
        reg.register(RegionInfo::new("us-east-1", true));
        reg.unregister(&RegionId::new("us-east-1"));
        assert_eq!(reg.count(), 0);
    }
}
