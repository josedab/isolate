//! Module review and rating system.
//!
//! User ratings, text reviews, and moderation for marketplace modules.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// A user review for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    pub module_id: String,
    pub author_id: String,
    pub rating: u8,
    pub title: String,
    pub body: String,
    pub created_at: u64,
    pub status: ReviewStatus,
    pub helpful_count: u32,
}

/// Review moderation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStatus {
    Pending,
    Approved,
    Rejected,
    Flagged,
}

/// Aggregate rating statistics for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingStats {
    pub module_id: String,
    pub total_reviews: usize,
    pub average_rating: f64,
    pub distribution: [u32; 5],
}

/// Review system managing ratings and moderation.
#[derive(Clone)]
pub struct ReviewSystem {
    inner: Arc<ReviewSystemInner>,
}

struct ReviewSystemInner {
    reviews: RwLock<Vec<Review>>,
    next_id: parking_lot::Mutex<u64>,
}

impl ReviewSystem {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ReviewSystemInner {
                reviews: RwLock::new(Vec::new()),
                next_id: parking_lot::Mutex::new(1),
            }),
        }
    }

    /// Submit a new review (rating 1-5).
    pub fn submit(&self, module_id: &str, author_id: &str, rating: u8, title: &str, body: &str, timestamp: u64) -> Result<String, ReviewError> {
        if !(1..=5).contains(&rating) {
            return Err(ReviewError::InvalidRating(rating));
        }
        if title.is_empty() {
            return Err(ReviewError::EmptyTitle);
        }

        // Check for duplicate review by same author
        let reviews = self.inner.reviews.read();
        if reviews.iter().any(|r| r.module_id == module_id && r.author_id == author_id && r.status != ReviewStatus::Rejected) {
            return Err(ReviewError::DuplicateReview);
        }
        drop(reviews);

        let mut id_counter = self.inner.next_id.lock();
        let id = format!("review-{}", *id_counter);
        *id_counter += 1;

        self.inner.reviews.write().push(Review {
            id: id.clone(),
            module_id: module_id.to_string(),
            author_id: author_id.to_string(),
            rating,
            title: title.to_string(),
            body: body.to_string(),
            created_at: timestamp,
            status: ReviewStatus::Pending,
            helpful_count: 0,
        });

        Ok(id)
    }

    /// Approve a review (moderation).
    pub fn approve(&self, review_id: &str) -> bool {
        self.set_status(review_id, ReviewStatus::Approved)
    }

    /// Reject a review (moderation).
    pub fn reject(&self, review_id: &str) -> bool {
        self.set_status(review_id, ReviewStatus::Rejected)
    }

    /// Flag a review for moderation.
    pub fn flag(&self, review_id: &str) -> bool {
        self.set_status(review_id, ReviewStatus::Flagged)
    }

    fn set_status(&self, review_id: &str, status: ReviewStatus) -> bool {
        let mut reviews = self.inner.reviews.write();
        if let Some(r) = reviews.iter_mut().find(|r| r.id == review_id) {
            r.status = status;
            true
        } else {
            false
        }
    }

    /// Mark a review as helpful.
    pub fn mark_helpful(&self, review_id: &str) -> bool {
        let mut reviews = self.inner.reviews.write();
        if let Some(r) = reviews.iter_mut().find(|r| r.id == review_id) {
            r.helpful_count += 1;
            true
        } else {
            false
        }
    }

    /// Get approved reviews for a module, sorted by most helpful.
    pub fn get_reviews(&self, module_id: &str) -> Vec<Review> {
        let reviews = self.inner.reviews.read();
        let mut result: Vec<Review> = reviews
            .iter()
            .filter(|r| r.module_id == module_id && r.status == ReviewStatus::Approved)
            .cloned()
            .collect();
        result.sort_by(|a, b| b.helpful_count.cmp(&a.helpful_count));
        result
    }

    /// Get rating statistics for a module (approved reviews only).
    pub fn rating_stats(&self, module_id: &str) -> RatingStats {
        let reviews = self.inner.reviews.read();
        let approved: Vec<&Review> = reviews
            .iter()
            .filter(|r| r.module_id == module_id && r.status == ReviewStatus::Approved)
            .collect();

        let total = approved.len();
        let mut distribution = [0u32; 5];
        let mut sum = 0u64;

        for r in &approved {
            if r.rating >= 1 && r.rating <= 5 {
                distribution[(r.rating - 1) as usize] += 1;
                sum += r.rating as u64;
            }
        }

        let average = if total > 0 { sum as f64 / total as f64 } else { 0.0 };

        RatingStats {
            module_id: module_id.to_string(),
            total_reviews: total,
            average_rating: average,
            distribution,
        }
    }

    /// Get reviews pending moderation.
    pub fn pending_reviews(&self) -> Vec<Review> {
        self.inner.reviews.read()
            .iter()
            .filter(|r| r.status == ReviewStatus::Pending)
            .cloned()
            .collect()
    }

    /// Get flagged reviews needing attention.
    pub fn flagged_reviews(&self) -> Vec<Review> {
        self.inner.reviews.read()
            .iter()
            .filter(|r| r.status == ReviewStatus::Flagged)
            .cloned()
            .collect()
    }

    /// Total review count across all modules.
    pub fn total_count(&self) -> usize {
        self.inner.reviews.read().len()
    }
}

/// Errors from the review system.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("rating must be 1-5, got {0}")]
    InvalidRating(u8),
    #[error("review title cannot be empty")]
    EmptyTitle,
    #[error("author already reviewed this module")]
    DuplicateReview,
}

impl Default for ReviewSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_and_approve() {
        let sys = ReviewSystem::new();
        let id = sys.submit("mod-a", "user-1", 5, "Great!", "Works perfectly", 1000).unwrap();
        assert!(sys.approve(&id));

        let reviews = sys.get_reviews("mod-a");
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].rating, 5);
    }

    #[test]
    fn test_invalid_rating() {
        let sys = ReviewSystem::new();
        assert!(matches!(
            sys.submit("m", "u", 0, "Bad", "", 0),
            Err(ReviewError::InvalidRating(0))
        ));
        assert!(matches!(
            sys.submit("m", "u", 6, "Bad", "", 0),
            Err(ReviewError::InvalidRating(6))
        ));
    }

    #[test]
    fn test_empty_title() {
        let sys = ReviewSystem::new();
        assert!(matches!(
            sys.submit("m", "u", 3, "", "body", 0),
            Err(ReviewError::EmptyTitle)
        ));
    }

    #[test]
    fn test_duplicate_review() {
        let sys = ReviewSystem::new();
        sys.submit("m", "u1", 4, "First", "", 0).unwrap();
        assert!(matches!(
            sys.submit("m", "u1", 5, "Again", "", 0),
            Err(ReviewError::DuplicateReview)
        ));
    }

    #[test]
    fn test_rating_stats() {
        let sys = ReviewSystem::new();
        let r1 = sys.submit("m", "u1", 5, "Five", "", 0).unwrap();
        let r2 = sys.submit("m", "u2", 3, "Three", "", 0).unwrap();
        let r3 = sys.submit("m", "u3", 4, "Four", "", 0).unwrap();
        sys.approve(&r1);
        sys.approve(&r2);
        sys.approve(&r3);

        let stats = sys.rating_stats("m");
        assert_eq!(stats.total_reviews, 3);
        assert!((stats.average_rating - 4.0).abs() < 0.01);
        assert_eq!(stats.distribution, [0, 0, 1, 1, 1]);
    }

    #[test]
    fn test_moderation_workflow() {
        let sys = ReviewSystem::new();
        let id = sys.submit("m", "u", 1, "Spam", "Buy crypto!", 0).unwrap();
        assert_eq!(sys.pending_reviews().len(), 1);

        sys.flag(&id);
        assert_eq!(sys.flagged_reviews().len(), 1);
        assert_eq!(sys.pending_reviews().len(), 0);

        sys.reject(&id);
        assert!(sys.get_reviews("m").is_empty());
    }

    #[test]
    fn test_helpful_sorting() {
        let sys = ReviewSystem::new();
        let r1 = sys.submit("m", "u1", 4, "Good", "", 0).unwrap();
        let r2 = sys.submit("m", "u2", 5, "Best!", "", 0).unwrap();
        sys.approve(&r1);
        sys.approve(&r2);
        sys.mark_helpful(&r2);
        sys.mark_helpful(&r2);
        sys.mark_helpful(&r1);

        let reviews = sys.get_reviews("m");
        assert_eq!(reviews[0].id, r2); // most helpful first
        assert_eq!(reviews[0].helpful_count, 2);
    }

    #[test]
    fn test_no_reviews_stats() {
        let sys = ReviewSystem::new();
        let stats = sys.rating_stats("empty");
        assert_eq!(stats.total_reviews, 0);
        assert_eq!(stats.average_rating, 0.0);
    }

    #[test]
    fn test_rejected_allows_resubmit() {
        let sys = ReviewSystem::new();
        let id = sys.submit("m", "u1", 1, "Bad", "", 0).unwrap();
        sys.reject(&id);
        // Can submit again after rejection
        assert!(sys.submit("m", "u1", 4, "Changed mind", "", 100).is_ok());
    }
}
