use crate::codecs::providers::anthropic::{self, AnthropicConfig};
use crate::codecs::providers::openai::wire::{Chat, OLLAMA, OpenAIConfig};
use crate::codecs::wire::Secret;
use serde::{Deserialize, Serialize};

use super::OLLAMA_API_BASE_URL;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub base_url: String,
    pub api_key: Secret,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaConfig {
    pub fn new() -> Self {
        Self {
            base_url: OLLAMA_API_BASE_URL.to_owned(),
            api_key: Secret::default(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl AsRef<str>) -> Self {
        self.base_url = base_url.as_ref().trim_end_matches('/').to_owned();
        self
    }

    pub fn with_api_key(mut self, api_key: impl Into<Secret>) -> Self {
        self.api_key = api_key.into();
        self
    }

    pub fn completion(&self, model: impl Into<String>) -> Chat {
        OpenAIConfig::with_key(&OLLAMA, self.api_key.clone())
            .with_base_url(format!("{}/v1", self.base_url))
            .chat(model)
    }

    pub fn anthropic_completion(&self, model: impl Into<String>) -> anthropic::Messages {
        AnthropicConfig::with_key(&anthropic::wire::OLLAMA, self.api_key.clone())
            .with_base_url(&self.base_url)
            .completion(model)
    }

    pub fn native_completion(&self, model: impl Into<String>) -> super::Chat {
        super::Chat::new(self.clone(), model)
    }

    pub(super) fn request(&self, method: http::Method, path: &str) -> http::request::Builder {
        let builder = http::Request::builder()
            .method(method)
            .uri(format!("{}{path}", self.base_url))
            .header(http::header::CONTENT_TYPE, "application/json");
        if self.api_key.is_empty() {
            builder
        } else {
            builder.header(
                http::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key.expose()),
            )
        }
    }
}
