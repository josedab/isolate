//! # Multi-Region Geo-Replication & Failover
//!
//! Cross-region configuration/module replication with automatic failover,
//! health monitoring, and consistency controls.

#![allow(missing_docs)]
mod failover;
mod region;
mod replication;

pub use failover::{FailoverController, FailoverEvent, FailoverPolicy};
pub use region::{RegionHealth, RegionId, RegionInfo, RegionRegistry, RegionStatus};
pub use replication::{ReplicatedItem, ReplicationConfig, ReplicationState, ReplicationTracker};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_region_replication_flow() {
        let registry = RegionRegistry::new();
        registry.register(RegionInfo::new("us-east-1", true));
        registry.register(RegionInfo::new("eu-west-1", false));
        registry.register(RegionInfo::new("ap-southeast-1", false));

        let tracker = ReplicationTracker::new(ReplicationConfig::default());
        tracker.add_item(ReplicatedItem::config("sandbox-cfg-1", b"config data"));

        // Simulate replication to all regions
        let regions = registry.list_healthy();
        assert_eq!(regions.len(), 3);

        for region in &regions {
            tracker.mark_replicated("sandbox-cfg-1", &region.id);
        }

        let state = tracker.get_state("sandbox-cfg-1").unwrap();
        assert_eq!(state.replicated_regions.len(), 3);
    }
}
