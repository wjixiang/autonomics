use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelObject {
    pub id: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    #[serde(rename = "type")]
    pub object_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelListParams {
    pub before_id: Option<String>,
    pub after_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelList {
    pub data: Vec<ModelObject>,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub max_context_length: u64,
    pub max_output_tokens: u64,
    pub capabilities: Vec<ModelCapability>,
    pub family: String,
    pub generation: String,
    pub supports_vision: bool,
    pub supports_tools: bool,
    pub supports_system_messages: bool,
    pub supports_streaming: bool,
    pub supported_languages: Vec<String>,
    pub training_cutoff: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    TextGeneration,
    Vision,
    ToolUse,
    CodeGeneration,
    Mathematical,
    Creative,
    Analysis,
    Summarization,
    Translation,
    LongContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub model_id: String,
    pub input_price_per_million: f64,
    pub output_price_per_million: f64,
    pub batch_input_price_per_million: Option<f64>,
    pub batch_output_price_per_million: Option<f64>,
    pub cache_write_price_per_million: Option<f64>,
    pub cache_read_price_per_million: Option<f64>,
    pub tier: PricingTier,
    pub currency: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PricingTier {
    Premium,
    Standard,
    Fast,
    Legacy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelComparison {
    pub models: Vec<ModelObject>,
    pub capabilities: Vec<ModelCapabilities>,
    pub pricing: Vec<ModelPricing>,
    pub performance: Vec<ModelPerformance>,
    pub summary: ComparisonSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformance {
    pub model_id: String,
    pub speed_score: u8,
    pub quality_score: u8,
    pub avg_response_time_ms: Option<u64>,
    pub tokens_per_second: Option<f64>,
    pub cost_efficiency_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonSummary {
    pub fastest_model: String,
    pub highest_quality_model: String,
    pub most_cost_effective_model: String,
    pub best_overall_model: String,
    pub key_differences: Vec<String>,
    pub use_case_recommendations: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelRequirements {
    pub max_input_cost_per_token: Option<f64>,
    pub max_output_cost_per_token: Option<f64>,
    pub min_context_length: Option<u64>,
    pub required_capabilities: Vec<ModelCapability>,
    pub preferred_family: Option<String>,
    pub min_speed_score: Option<u8>,
    pub min_quality_score: Option<u8>,
    pub requires_vision: Option<bool>,
    pub requires_tools: Option<bool>,
    pub max_response_time_ms: Option<u64>,
    pub preferred_languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsageRecommendations {
    pub use_case: String,
    pub recommended_models: Vec<ModelRecommendation>,
    pub guidelines: Vec<String>,
    pub recommended_parameters: RecommendedParameters,
    pub pitfalls: Vec<String>,
    pub expected_performance: PerformanceExpectations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecommendation {
    pub model_id: String,
    pub reason: String,
    pub confidence_score: u8,
    pub cost_range: CostRange,
    pub strengths: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedParameters {
    pub temperature_range: (f32, f32),
    pub max_tokens_range: (u32, u32),
    pub top_p_range: Option<(f32, f32)>,
    pub use_streaming: Option<bool>,
    pub system_message_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceExpectations {
    pub response_time_range_ms: (u64, u64),
    pub cost_range: CostRange,
    pub quality_level: QualityLevel,
    pub success_rate_percentage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRange {
    pub min_cost_usd: f64,
    pub max_cost_usd: f64,
    pub typical_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum QualityLevel {
    Excellent,
    Good,
    Acceptable,
    Basic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimation {
    pub model_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub input_cost_usd: f64,
    pub output_cost_usd: f64,
    pub total_cost_usd: f64,
    pub batch_discount_usd: Option<f64>,
    pub cache_savings_usd: Option<f64>,
    pub final_cost_usd: f64,
    pub breakdown: CostBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub cost_per_input_token_usd: f64,
    pub cost_per_output_token_usd: f64,
    pub effective_cost_per_token_usd: f64,
    pub cost_vs_alternatives: HashMap<String, f64>,
}

impl ModelListParams {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn before_id(mut self, before_id: impl Into<String>) -> Self {
        self.before_id = Some(before_id.into());
        self
    }

    #[must_use]
    pub fn after_id(mut self, after_id: impl Into<String>) -> Self {
        self.after_id = Some(after_id.into());
        self
    }

    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit.clamp(1, 1000));
        self
    }
}

impl ModelRequirements {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn max_input_cost_per_token(mut self, cost: f64) -> Self {
        self.max_input_cost_per_token = Some(cost);
        self
    }

    #[must_use]
    pub fn max_output_cost_per_token(mut self, cost: f64) -> Self {
        self.max_output_cost_per_token = Some(cost);
        self
    }

    #[must_use]
    pub fn min_context_length(mut self, length: u64) -> Self {
        self.min_context_length = Some(length);
        self
    }

    #[must_use]
    pub fn require_capability(mut self, capability: ModelCapability) -> Self {
        self.required_capabilities.push(capability);
        self
    }

    #[must_use]
    pub fn capabilities(mut self, capabilities: Vec<ModelCapability>) -> Self {
        self.required_capabilities = capabilities;
        self
    }

    #[must_use]
    pub fn preferred_family(mut self, family: impl Into<String>) -> Self {
        self.preferred_family = Some(family.into());
        self
    }

    #[must_use]
    pub fn require_vision(mut self) -> Self {
        self.requires_vision = Some(true);
        self
    }

    #[must_use]
    pub fn require_tools(mut self) -> Self {
        self.requires_tools = Some(true);
        self
    }

    #[must_use]
    pub fn min_quality_score(mut self, score: u8) -> Self {
        self.min_quality_score = Some(score.min(10));
        self
    }

    #[must_use]
    pub fn min_speed_score(mut self, score: u8) -> Self {
        self.min_speed_score = Some(score.min(10));
        self
    }
}

impl ModelObject {
    #[must_use]
    pub fn is_alias(&self) -> bool {
        self.id.contains("latest") || self.id.ends_with("-0")
    }

    #[must_use]
    pub fn family(&self) -> String {
        let parts: Vec<&str> = self.id.split('-').collect();
        if parts.len() >= 3 {
            format!("{}-{}", parts[0], parts[1])
        } else {
            parts[0].to_string()
        }
    }

    #[must_use]
    pub fn is_family(&self, family: &str) -> bool {
        self.id.starts_with(family)
    }

    #[must_use]
    pub fn model_size(&self) -> Option<String> {
        if self.id.contains("opus") {
            Some("opus".to_string())
        } else if self.id.contains("sonnet") {
            Some("sonnet".to_string())
        } else if self.id.contains("haiku") {
            Some("haiku".to_string())
        } else {
            None
        }
    }
}

impl ModelComparison {
    #[must_use]
    pub fn best_for_speed(&self) -> Option<&ModelObject> {
        self.performance
            .iter()
            .max_by_key(|p| p.speed_score)
            .and_then(|p| self.models.iter().find(|m| m.id == p.model_id))
    }

    #[must_use]
    pub fn best_for_quality(&self) -> Option<&ModelObject> {
        self.performance
            .iter()
            .max_by_key(|p| p.quality_score)
            .and_then(|p| self.models.iter().find(|m| m.id == p.model_id))
    }

    #[must_use]
    pub fn most_cost_effective(&self) -> Option<&ModelObject> {
        self.performance
            .iter()
            .max_by_key(|p| p.cost_efficiency_score)
            .and_then(|p| self.models.iter().find(|m| m.id == p.model_id))
    }
}

impl CostEstimation {
    #[must_use]
    pub fn cost_per_1k_tokens(&self) -> f64 {
        let total_tokens = self.input_tokens + self.output_tokens;
        if total_tokens > 0 {
            (self.final_cost_usd * 1000.0) / total_tokens as f64
        } else {
            0.0
        }
    }

    #[must_use]
    pub fn savings_percentage(&self) -> f64 {
        let original_cost = self.input_cost_usd + self.output_cost_usd;
        if original_cost > 0.0 {
            ((original_cost - self.final_cost_usd) / original_cost) * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_list_params_builder() {
        let params = ModelListParams::new().limit(50).after_id("model_123");

        assert_eq!(params.limit, Some(50));
        assert_eq!(params.after_id, Some("model_123".to_string()));
        assert_eq!(params.before_id, None);
    }

    #[test]
    fn test_model_requirements_builder() {
        let requirements = ModelRequirements::new()
            .max_input_cost_per_token(0.01)
            .min_context_length(100000)
            .require_vision()
            .require_capability(ModelCapability::ToolUse);

        assert_eq!(requirements.max_input_cost_per_token, Some(0.01));
        assert_eq!(requirements.min_context_length, Some(100000));
        assert_eq!(requirements.requires_vision, Some(true));
        assert!(
            requirements
                .required_capabilities
                .contains(&ModelCapability::ToolUse)
        );
    }

    #[test]
    fn test_model_object_methods() {
        let model = ModelObject {
            id: "claude-3-5-sonnet-latest".to_string(),
            display_name: "Claude 3.5 Sonnet".to_string(),
            created_at: Utc::now(),
            object_type: "model".to_string(),
        };

        assert!(model.is_alias());
        assert_eq!(model.family(), "claude-3");
        assert!(model.is_family("claude-3-5"));
        assert_eq!(model.model_size(), Some("sonnet".to_string()));
    }

    #[test]
    fn test_cost_estimation_calculations() {
        let estimation = CostEstimation {
            model_id: "test-model".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
            input_cost_usd: 0.01,
            output_cost_usd: 0.03,
            total_cost_usd: 0.04,
            batch_discount_usd: Some(0.005),
            cache_savings_usd: None,
            final_cost_usd: 0.035,
            breakdown: CostBreakdown {
                cost_per_input_token_usd: 0.00001,
                cost_per_output_token_usd: 0.00006,
                effective_cost_per_token_usd: 0.000023,
                cost_vs_alternatives: HashMap::new(),
            },
        };

        assert!((estimation.cost_per_1k_tokens() - 0.02333).abs() < 0.001);
        assert!((estimation.savings_percentage() - 12.5).abs() < 0.1);
    }

    #[test]
    fn test_limit_validation() {
        let params = ModelListParams::new().limit(2000);
        assert_eq!(params.limit, Some(1000));

        let params = ModelListParams::new().limit(0);
        assert_eq!(params.limit, Some(1));
    }

    #[test]
    fn test_model_capability_serialization() {
        let capability = ModelCapability::Vision;
        let serialized = serde_json::to_string(&capability).unwrap();
        assert_eq!(serialized, "\"vision\"");

        let deserialized: ModelCapability = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ModelCapability::Vision);
    }
}
