//! Token usage tracking and cost estimation for LLM interactions.
//!
//! Provides structures to monitor token consumption across a session,
//! enforce token budgets, and estimate costs based on provider pricing.

use super::provider::LlmProvider;
use serde::{Deserialize, Serialize};

/// Token usage for a single LLM interaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: u64,
    /// Number of tokens in the completion.
    pub completion_tokens: u64,
    /// Total tokens used (prompt + completion).
    pub total_tokens: u64,
    /// Tokens consumed by function call arguments and results.
    pub function_call_tokens: u64,
}

/// A budget for limiting token usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Maximum tokens allowed.
    pub max_tokens: u64,
    /// Tokens used so far.
    pub used_tokens: u64,
    /// Tokens reserved for overhead (e.g., system prompts).
    pub reserved_tokens: u64,
}

impl TokenBudget {
    /// Create a new token budget with the given maximum.
    pub fn new(max_tokens: u64) -> Self {
        Self { max_tokens, used_tokens: 0, reserved_tokens: 0 }
    }

    /// Create a budget with reserved tokens.
    pub fn with_reserved(max_tokens: u64, reserved_tokens: u64) -> Self {
        Self { max_tokens, used_tokens: 0, reserved_tokens }
    }

    /// Remaining tokens available (accounting for reserved).
    pub fn remaining(&self) -> u64 {
        self.max_tokens.saturating_sub(self.used_tokens).saturating_sub(self.reserved_tokens)
    }

    /// Whether the budget has been exceeded.
    pub fn is_exceeded(&self) -> bool {
        self.used_tokens + self.reserved_tokens >= self.max_tokens
    }
}

/// Estimated cost for LLM usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Cost of input/prompt tokens in USD.
    pub input_cost_usd: f64,
    /// Cost of output/completion tokens in USD.
    pub output_cost_usd: f64,
    /// Total estimated cost in USD.
    pub total_cost_usd: f64,
}

/// Pricing information for a provider model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingTier {
    /// Price per 1,000 input tokens in USD.
    pub input_price_per_1k: f64,
    /// Price per 1,000 output tokens in USD.
    pub output_price_per_1k: f64,
    /// Provider this pricing applies to.
    pub provider: LlmProvider,
}

impl PricingTier {
    /// Default pricing for OpenAI GPT-4o.
    pub fn openai_gpt4o() -> Self {
        Self {
            input_price_per_1k: 0.0025,
            output_price_per_1k: 0.01,
            provider: LlmProvider::OpenAi,
        }
    }

    /// Default pricing for Anthropic Claude 3.5 Sonnet.
    pub fn anthropic_claude() -> Self {
        Self {
            input_price_per_1k: 0.003,
            output_price_per_1k: 0.015,
            provider: LlmProvider::Anthropic,
        }
    }

    /// Estimate cost for the given token counts.
    pub fn estimate(&self, input_tokens: u64, output_tokens: u64) -> CostEstimate {
        let input_cost = (input_tokens as f64 / 1000.0) * self.input_price_per_1k;
        let output_cost = (output_tokens as f64 / 1000.0) * self.output_price_per_1k;
        CostEstimate {
            input_cost_usd: input_cost,
            output_cost_usd: output_cost,
            total_cost_usd: input_cost + output_cost,
        }
    }
}

/// Tracks cumulative token usage and costs across a session.
#[derive(Debug, Clone)]
pub struct TokenTracker {
    /// Cumulative token usage.
    cumulative: TokenUsage,
    /// Optional token budget.
    budget: Option<TokenBudget>,
    /// Pricing tier for cost estimation.
    pricing: PricingTier,
    /// Number of interactions recorded.
    interaction_count: u64,
}

impl TokenTracker {
    /// Create a new tracker with the given pricing tier.
    pub fn new(pricing: PricingTier) -> Self {
        Self { cumulative: TokenUsage::default(), budget: None, pricing, interaction_count: 0 }
    }

    /// Create a tracker with a token budget.
    pub fn with_budget(pricing: PricingTier, budget: TokenBudget) -> Self {
        Self {
            cumulative: TokenUsage::default(),
            budget: Some(budget),
            pricing,
            interaction_count: 0,
        }
    }

    /// Record usage from a single LLM interaction.
    pub fn record_usage(&mut self, usage: TokenUsage) {
        self.cumulative.prompt_tokens += usage.prompt_tokens;
        self.cumulative.completion_tokens += usage.completion_tokens;
        self.cumulative.total_tokens += usage.total_tokens;
        self.cumulative.function_call_tokens += usage.function_call_tokens;
        self.interaction_count += 1;

        if let Some(ref mut budget) = self.budget {
            budget.used_tokens += usage.total_tokens;
        }
    }

    /// Estimate the total cost incurred so far.
    pub fn estimate_cost(&self) -> CostEstimate {
        self.pricing.estimate(self.cumulative.prompt_tokens, self.cumulative.completion_tokens)
    }

    /// Remaining tokens in the budget, or `u64::MAX` if no budget is set.
    pub fn remaining_budget(&self) -> u64 {
        self.budget.as_ref().map(|b| b.remaining()).unwrap_or(u64::MAX)
    }

    /// Whether the token budget has been exceeded.
    pub fn is_budget_exceeded(&self) -> bool {
        self.budget.as_ref().map(|b| b.is_exceeded()).unwrap_or(false)
    }

    /// Get the cumulative token usage.
    pub fn total_usage(&self) -> TokenUsage {
        self.cumulative.clone()
    }

    /// Reset the tracker to zero usage (budget limits are preserved).
    pub fn reset(&mut self) {
        self.cumulative = TokenUsage::default();
        self.interaction_count = 0;
        if let Some(ref mut budget) = self.budget {
            budget.used_tokens = 0;
        }
    }

    /// Number of interactions recorded.
    pub fn interaction_count(&self) -> u64 {
        self.interaction_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.function_call_tokens, 0);
    }

    #[test]
    fn test_token_budget_remaining() {
        let mut budget = TokenBudget::new(10_000);
        assert_eq!(budget.remaining(), 10_000);
        assert!(!budget.is_exceeded());

        budget.used_tokens = 7_000;
        assert_eq!(budget.remaining(), 3_000);
        assert!(!budget.is_exceeded());

        budget.used_tokens = 10_000;
        assert_eq!(budget.remaining(), 0);
        assert!(budget.is_exceeded());
    }

    #[test]
    fn test_token_budget_with_reserved() {
        let budget = TokenBudget::with_reserved(10_000, 2_000);
        assert_eq!(budget.remaining(), 8_000);
        assert!(!budget.is_exceeded());
    }

    #[test]
    fn test_pricing_tier_openai() {
        let pricing = PricingTier::openai_gpt4o();
        assert_eq!(pricing.provider, LlmProvider::OpenAi);
        let estimate = pricing.estimate(1_000, 500);
        assert!((estimate.input_cost_usd - 0.0025).abs() < 1e-10);
        assert!((estimate.output_cost_usd - 0.005).abs() < 1e-10);
        assert!((estimate.total_cost_usd - 0.0075).abs() < 1e-10);
    }

    #[test]
    fn test_pricing_tier_anthropic() {
        let pricing = PricingTier::anthropic_claude();
        assert_eq!(pricing.provider, LlmProvider::Anthropic);
        let estimate = pricing.estimate(2_000, 1_000);
        assert!((estimate.input_cost_usd - 0.006).abs() < 1e-10);
        assert!((estimate.output_cost_usd - 0.015).abs() < 1e-10);
    }

    #[test]
    fn test_token_tracker_record_usage() {
        let mut tracker = TokenTracker::new(PricingTier::openai_gpt4o());

        tracker.record_usage(TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            function_call_tokens: 20,
        });

        tracker.record_usage(TokenUsage {
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
            function_call_tokens: 30,
        });

        let total = tracker.total_usage();
        assert_eq!(total.prompt_tokens, 300);
        assert_eq!(total.completion_tokens, 150);
        assert_eq!(total.total_tokens, 450);
        assert_eq!(total.function_call_tokens, 50);
        assert_eq!(tracker.interaction_count(), 2);
    }

    #[test]
    fn test_token_tracker_with_budget() {
        let budget = TokenBudget::new(500);
        let mut tracker = TokenTracker::with_budget(PricingTier::openai_gpt4o(), budget);

        assert_eq!(tracker.remaining_budget(), 500);
        assert!(!tracker.is_budget_exceeded());

        tracker.record_usage(TokenUsage {
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
            function_call_tokens: 0,
        });

        assert_eq!(tracker.remaining_budget(), 200);
        assert!(!tracker.is_budget_exceeded());

        tracker.record_usage(TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 100,
            total_tokens: 200,
            function_call_tokens: 0,
        });

        assert_eq!(tracker.remaining_budget(), 0);
        assert!(tracker.is_budget_exceeded());
    }

    #[test]
    fn test_token_tracker_estimate_cost() {
        let mut tracker = TokenTracker::new(PricingTier::openai_gpt4o());
        tracker.record_usage(TokenUsage {
            prompt_tokens: 1_000,
            completion_tokens: 500,
            total_tokens: 1_500,
            function_call_tokens: 0,
        });

        let cost = tracker.estimate_cost();
        assert!((cost.input_cost_usd - 0.0025).abs() < 1e-10);
        assert!((cost.output_cost_usd - 0.005).abs() < 1e-10);
        assert!((cost.total_cost_usd - 0.0075).abs() < 1e-10);
    }

    #[test]
    fn test_token_tracker_reset() {
        let budget = TokenBudget::new(1_000);
        let mut tracker = TokenTracker::with_budget(PricingTier::openai_gpt4o(), budget);

        tracker.record_usage(TokenUsage {
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
            function_call_tokens: 0,
        });

        assert_eq!(tracker.total_usage().total_tokens, 300);
        assert_eq!(tracker.remaining_budget(), 700);

        tracker.reset();

        assert_eq!(tracker.total_usage().total_tokens, 0);
        assert_eq!(tracker.remaining_budget(), 1_000);
        assert_eq!(tracker.interaction_count(), 0);
    }

    #[test]
    fn test_token_tracker_no_budget() {
        let tracker = TokenTracker::new(PricingTier::openai_gpt4o());
        assert_eq!(tracker.remaining_budget(), u64::MAX);
        assert!(!tracker.is_budget_exceeded());
    }

    #[test]
    fn test_token_usage_serde_roundtrip() {
        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            function_call_tokens: 20,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_tokens, 100);
        assert_eq!(deserialized.total_tokens, 150);
    }
}
