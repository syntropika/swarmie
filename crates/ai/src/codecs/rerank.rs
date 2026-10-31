use crate::codecs::completion::Usage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResult {
    pub index: usize,
    pub document: Option<String>,
    pub relevance_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResponse {
    pub results: Vec<RerankResult>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub usage: Usage,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub raw: serde_json::Value,
}

impl RerankResponse {
    pub fn new(results: Vec<RerankResult>) -> Self {
        Self {
            results,
            model: None,
            usage: Usage::default(),
            provider: String::new(),
            response_id: None,
            provider_request_id: None,
            raw: serde_json::Value::Null,
        }
    }
}
