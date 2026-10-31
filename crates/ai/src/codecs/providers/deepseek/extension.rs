use serde::Serialize;
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;
use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeepSeekExt;

impl ProviderExtension for DeepSeekExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = DeepSeekOptions;
    type Extras = DeepSeekExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DeepSeekOptions {}

impl DeepSeekOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ExtensionOptions for DeepSeekOptions {
    type Ext = DeepSeekExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekExtras {
    pub prompt_cache_hit_tokens: Option<u64>,
    pub prompt_cache_miss_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub system_fingerprint: Option<String>,
}

impl ReplyExtras for DeepSeekExtras {
    fn from_reply(_api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            prompt_cache_hit_tokens: reply_field(raw, "/usage/prompt_cache_hit_tokens")?,
            prompt_cache_miss_tokens: reply_field(raw, "/usage/prompt_cache_miss_tokens")?,
            reasoning_tokens: reply_field(
                raw,
                "/usage/completion_tokens_details/reasoning_tokens",
            )?,
            system_fingerprint: reply_field(raw, "/system_fingerprint")?,
        })
    }
}
