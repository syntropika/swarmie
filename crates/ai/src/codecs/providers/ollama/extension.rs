use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OllamaExt;

impl ProviderExtension for OllamaExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = OllamaOptions;
    type Extras = OllamaExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OllamaOptions {
    #[serde(rename = "*")]
    pub shared: OllamaShared,
    #[serde(rename = "ollama.chat")]
    pub chat: OllamaNative,
}

impl ExtensionOptions for OllamaOptions {
    type Ext = OllamaExt;

    fn unsupported(
        &self,
        target: &dyn crate::codecs::completion::ReplayTarget,
        _request: &crate::codecs::completion::CompletionRequest,
    ) -> Vec<(&'static str, String)> {
        let messages = Api::from_static(crate::codecs::providers::anthropic::wire::MESSAGES_API);
        match self.shared.keep_alive {
            Some(_) if target.api() == messages => vec![(
                "keep_alive",
                "Ollama's Anthropic-compatible API does not read it".to_owned(),
            )],
            _ => Vec::new(),
        }
    }
}

impl OllamaOptions {
    pub fn keep_alive(mut self, keep_alive: KeepAlive) -> Self {
        self.shared.keep_alive = Some(keep_alive);
        self
    }

    pub fn num_ctx(mut self, num_ctx: u32) -> Self {
        self.chat.options.num_ctx = Some(num_ctx);
        self
    }

    pub fn num_keep(mut self, num_keep: u32) -> Self {
        self.chat.options.num_keep = Some(num_keep);
        self
    }

    pub fn top_k(mut self, top_k: u32) -> Self {
        self.chat.options.top_k = Some(top_k);
        self
    }

    pub fn min_p(mut self, min_p: f64) -> Self {
        self.chat.options.min_p = Some(min_p);
        self
    }

    pub fn repeat_penalty(mut self, penalty: f64) -> Self {
        self.chat.options.repeat_penalty = Some(penalty);
        self
    }

    pub fn repeat_last_n(mut self, last_n: i32) -> Self {
        self.chat.options.repeat_last_n = Some(last_n);
        self
    }

    pub fn num_gpu(mut self, num_gpu: i32) -> Self {
        self.chat.options.num_gpu = Some(num_gpu);
        self
    }

    pub fn num_thread(mut self, num_thread: u32) -> Self {
        self.chat.options.num_thread = Some(num_thread);
        self
    }

    pub fn logprobs(mut self, logprobs: bool) -> Self {
        self.chat.logprobs = Some(logprobs);
        self
    }

    pub fn top_logprobs(mut self, top_logprobs: u32) -> Self {
        self.chat.top_logprobs = Some(top_logprobs);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OllamaShared {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<KeepAlive>,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum KeepAlive {
    Duration(String),
    Seconds(i64),
}

impl KeepAlive {
    pub fn duration(duration: impl Into<String>) -> Self {
        Self::Duration(duration.into())
    }

    pub fn seconds(seconds: i64) -> Self {
        Self::Seconds(seconds)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OllamaNative {
    #[serde(skip_serializing_if = "ModelOptions::is_empty")]
    pub options: ModelOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ModelOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_keep: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_last_n: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_gpu: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_thread: Option<u32>,
}

impl ModelOptions {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct OllamaExtras {
    pub model: Option<String>,
    pub created_at: Option<String>,
    pub done_reason: Option<String>,
    pub total_duration: Option<u64>,
    pub load_duration: Option<u64>,
    pub prompt_eval_duration: Option<u64>,
    pub eval_duration: Option<u64>,
    pub prompt_eval_count: Option<u64>,
    pub prompt_eval_cached_count: Option<u64>,
    pub eval_count: Option<u64>,
    pub logprobs: Option<Vec<Logprob>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Logprob {
    pub token: Option<String>,
    pub logprob: Option<f64>,
    pub bytes: Option<Vec<u8>>,
    pub top_logprobs: Option<Vec<Logprob>>,
}

impl ReplyExtras for OllamaExtras {
    fn from_reply(_api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        Self::deserialize(raw)
    }
}
