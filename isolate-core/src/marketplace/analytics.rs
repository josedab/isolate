//! Download and usage analytics for the marketplace.
//!
//! Tracks module downloads, star counts, and computes trending/top-downloaded
//! rankings.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A single download event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub module_name: String,
    pub version: String,
    pub downloaded_at: DateTime<Utc>,
    pub client_info: Option<String>,
}

/// Aggregate statistics for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleStats {
    pub module_name: String,
    pub total_downloads: u64,
    pub downloads_last_30d: u64,
    pub downloads_last_7d: u64,
    pub unique_users: u64,
    pub first_published: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub version_count: u32,
    pub star_count: u64,
}

/// A module on the trending list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingModule {
    pub module_name: String,
    pub growth_rate: f64,
    pub total_downloads: u64,
    pub category: String,
}

/// In-memory analytics tracker.
pub struct AnalyticsTracker {
    records: Vec<DownloadRecord>,
    stars: HashMap<String, HashSet<String>>,
}

impl AnalyticsTracker {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self { records: Vec::new(), stars: HashMap::new() }
    }

    /// Record a download event.
    pub fn record_download(&mut self, record: DownloadRecord) {
        self.records.push(record);
    }

    /// Star a module for a user.
    pub fn record_star(&mut self, module_name: &str, user_id: &str) {
        self.stars.entry(module_name.to_string()).or_default().insert(user_id.to_string());
    }

    /// Remove a star.
    pub fn remove_star(&mut self, module_name: &str, user_id: &str) {
        if let Some(users) = self.stars.get_mut(module_name) {
            users.remove(user_id);
        }
    }

    /// Compute aggregate stats for a module.
    pub fn get_stats(&self, module_name: &str) -> Option<ModuleStats> {
        let module_records: Vec<&DownloadRecord> =
            self.records.iter().filter(|r| r.module_name == module_name).collect();

        if module_records.is_empty() {
            return None;
        }

        let now = Utc::now();
        let cutoff_30d = now - Duration::days(30);
        let cutoff_7d = now - Duration::days(7);

        let total_downloads = module_records.len() as u64;
        let downloads_last_30d =
            module_records.iter().filter(|r| r.downloaded_at >= cutoff_30d).count() as u64;
        let downloads_last_7d =
            module_records.iter().filter(|r| r.downloaded_at >= cutoff_7d).count() as u64;

        let unique_users = module_records
            .iter()
            .filter_map(|r| r.client_info.as_deref())
            .collect::<HashSet<_>>()
            .len() as u64;

        let first_published = module_records.iter().map(|r| r.downloaded_at).min().unwrap();
        let last_updated = module_records.iter().map(|r| r.downloaded_at).max().unwrap();

        let version_count =
            module_records.iter().map(|r| r.version.as_str()).collect::<HashSet<_>>().len() as u32;

        let star_count = self.stars.get(module_name).map(|s| s.len() as u64).unwrap_or(0);

        Some(ModuleStats {
            module_name: module_name.to_string(),
            total_downloads,
            downloads_last_30d,
            downloads_last_7d,
            unique_users,
            first_published,
            last_updated,
            version_count,
            star_count,
        })
    }

    /// Get trending modules ranked by 7-day growth rate.
    pub fn trending(&self, limit: usize) -> Vec<TrendingModule> {
        let now = Utc::now();
        let cutoff_7d = now - Duration::days(7);
        let cutoff_14d = now - Duration::days(14);

        // Group downloads by module
        let mut last_7d: HashMap<String, u64> = HashMap::new();
        let mut prev_7d: HashMap<String, u64> = HashMap::new();
        let mut totals: HashMap<String, u64> = HashMap::new();

        for r in &self.records {
            *totals.entry(r.module_name.clone()).or_default() += 1;
            if r.downloaded_at >= cutoff_7d {
                *last_7d.entry(r.module_name.clone()).or_default() += 1;
            } else if r.downloaded_at >= cutoff_14d {
                *prev_7d.entry(r.module_name.clone()).or_default() += 1;
            }
        }

        let mut trending: Vec<TrendingModule> = totals
            .keys()
            .map(|name| {
                let recent = *last_7d.get(name).unwrap_or(&0) as f64;
                let previous = *prev_7d.get(name).unwrap_or(&0) as f64;
                let growth_rate = if previous > 0.0 {
                    ((recent - previous) / previous) * 100.0
                } else if recent > 0.0 {
                    100.0
                } else {
                    0.0
                };
                TrendingModule {
                    module_name: name.clone(),
                    growth_rate,
                    total_downloads: *totals.get(name).unwrap_or(&0),
                    category: String::new(),
                }
            })
            .collect();

        trending.sort_by(|a, b| {
            b.growth_rate.partial_cmp(&a.growth_rate).unwrap_or(std::cmp::Ordering::Equal)
        });
        trending.truncate(limit);
        trending
    }

    /// Get the top downloaded modules.
    pub fn top_downloaded(&self, limit: usize) -> Vec<(String, u64)> {
        let mut counts: HashMap<String, u64> = HashMap::new();
        for r in &self.records {
            *counts.entry(r.module_name.clone()).or_default() += 1;
        }

        let mut sorted: Vec<(String, u64)> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(limit);
        sorted
    }
}

impl Default for AnalyticsTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(
        name: &str,
        version: &str,
        days_ago: i64,
        client: Option<&str>,
    ) -> DownloadRecord {
        DownloadRecord {
            module_name: name.to_string(),
            version: version.to_string(),
            downloaded_at: Utc::now() - Duration::days(days_ago),
            client_info: client.map(String::from),
        }
    }

    #[test]
    fn test_record_and_get_stats() {
        let mut tracker = AnalyticsTracker::new();
        tracker.record_download(make_record("mod-a", "1.0.0", 1, Some("user1")));
        tracker.record_download(make_record("mod-a", "1.0.0", 2, Some("user2")));

        let stats = tracker.get_stats("mod-a").unwrap();
        assert_eq!(stats.total_downloads, 2);
        assert_eq!(stats.unique_users, 2);
        assert_eq!(stats.version_count, 1);
    }

    #[test]
    fn test_stats_nonexistent_module() {
        let tracker = AnalyticsTracker::new();
        assert!(tracker.get_stats("nonexistent").is_none());
    }

    #[test]
    fn test_downloads_time_windows() {
        let mut tracker = AnalyticsTracker::new();
        tracker.record_download(make_record("mod-a", "1.0.0", 1, None)); // within 7d
        tracker.record_download(make_record("mod-a", "1.0.0", 10, None)); // within 30d
        tracker.record_download(make_record("mod-a", "1.0.0", 60, None)); // older

        let stats = tracker.get_stats("mod-a").unwrap();
        assert_eq!(stats.total_downloads, 3);
        assert_eq!(stats.downloads_last_7d, 1);
        assert_eq!(stats.downloads_last_30d, 2);
    }

    #[test]
    fn test_star_and_unstar() {
        let mut tracker = AnalyticsTracker::new();
        tracker.record_download(make_record("mod-a", "1.0.0", 0, None));
        tracker.record_star("mod-a", "user1");
        tracker.record_star("mod-a", "user2");

        let stats = tracker.get_stats("mod-a").unwrap();
        assert_eq!(stats.star_count, 2);

        tracker.remove_star("mod-a", "user1");
        let stats = tracker.get_stats("mod-a").unwrap();
        assert_eq!(stats.star_count, 1);
    }

    #[test]
    fn test_star_idempotent() {
        let mut tracker = AnalyticsTracker::new();
        tracker.record_download(make_record("mod-a", "1.0.0", 0, None));
        tracker.record_star("mod-a", "user1");
        tracker.record_star("mod-a", "user1"); // duplicate

        let stats = tracker.get_stats("mod-a").unwrap();
        assert_eq!(stats.star_count, 1);
    }

    #[test]
    fn test_top_downloaded() {
        let mut tracker = AnalyticsTracker::new();
        for _ in 0..5 {
            tracker.record_download(make_record("popular", "1.0.0", 1, None));
        }
        for _ in 0..2 {
            tracker.record_download(make_record("niche", "1.0.0", 1, None));
        }

        let top = tracker.top_downloaded(10);
        assert_eq!(top[0].0, "popular");
        assert_eq!(top[0].1, 5);
        assert_eq!(top[1].0, "niche");
        assert_eq!(top[1].1, 2);
    }

    #[test]
    fn test_trending() {
        let mut tracker = AnalyticsTracker::new();
        // Recent downloads for mod-a (within 7 days)
        for _ in 0..10 {
            tracker.record_download(make_record("mod-a", "1.0.0", 1, None));
        }
        // Older downloads for mod-b (8-14 days ago)
        for _ in 0..10 {
            tracker.record_download(make_record("mod-b", "1.0.0", 10, None));
        }

        let trending = tracker.trending(10);
        assert!(!trending.is_empty());
        // mod-a should have higher growth rate (all recent)
        let mod_a = trending.iter().find(|t| t.module_name == "mod-a").unwrap();
        assert!(mod_a.growth_rate > 0.0);
    }

    #[test]
    fn test_multiple_versions() {
        let mut tracker = AnalyticsTracker::new();
        tracker.record_download(make_record("mod-a", "1.0.0", 0, None));
        tracker.record_download(make_record("mod-a", "1.1.0", 0, None));
        tracker.record_download(make_record("mod-a", "2.0.0", 0, None));

        let stats = tracker.get_stats("mod-a").unwrap();
        assert_eq!(stats.version_count, 3);
    }
}
