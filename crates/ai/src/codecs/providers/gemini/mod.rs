pub mod completion;
pub mod embedding;
pub mod extension;

pub mod model_listing;
mod options;
pub mod streaming;
pub mod transcription;

pub use completion::ThoughtReplay;
pub use embedding::{EMBEDDING_001, EMBEDDING_004};

pub use model_listing::*;

use crate::codecs::wire::Secret;

pub use completion::PROVIDER_NAME;

pub const BASE_URL: &str = "https://generativelanguage.googleapis.com";

pub const API_KEY_ENV: &str = "GEMINI_API_KEY";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeminiConfig {
    pub api_key: Secret,
    pub base_url: String,
}

impl GeminiConfig {
    pub fn new(api_key: impl Into<Secret>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: BASE_URL.to_owned(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub(crate) fn uri(&self, path: &str) -> String {
        let trimmed = path.trim_start_matches('/');
        let separator = if trimmed.contains('?') { "&" } else { "?" };
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/{trimmed}{separator}key={}", self.api_key.expose())
    }

    pub fn completion(&self, model: impl Into<String>) -> completion::GenerateContent {
        completion::GenerateContent::new(self.clone(), model)
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ImageSize {
    #[serde(rename = "512")]
    FiveTwelve,
    #[serde(rename = "1K")]
    OneK,
    #[serde(rename = "2K")]
    TwoK,
    #[serde(rename = "4K")]
    FourK,
}
