use serde::Serialize;
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;
use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;
use crate::codecs::providers::anthropic::extension::MessagesStop;
use crate::codecs::providers::anthropic::wire::MESSAGES_API;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoonshotExt;

impl ProviderExtension for MoonshotExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = MoonshotOptions;
    type Extras = MoonshotExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MoonshotOptions {
    #[serde(rename = "openai.chat")]
    pub chat: MoonshotChat,
}

impl MoonshotOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chat(mut self, chat: MoonshotChat) -> Self {
        self.chat = chat;
        self
    }

    pub fn thinking_keep(mut self, keep: ThinkingKeep) -> Self {
        self.chat = std::mem::take(&mut self.chat).thinking_keep(keep);
        self
    }

    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.chat = std::mem::take(&mut self.chat).prompt_cache_key(key);
        self
    }
}

impl ExtensionOptions for MoonshotOptions {
    type Ext = MoonshotExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MoonshotChat {
    #[serde(skip_serializing_if = "MoonshotThinking::is_empty")]
    pub thinking: MoonshotThinking,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MoonshotThinking {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<ThinkingKeep>,
}

impl MoonshotThinking {
    fn is_empty(&self) -> bool {
        self.keep.is_none()
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingKeep {
    All,
}

impl MoonshotChat {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn thinking_keep(mut self, keep: ThinkingKeep) -> Self {
        self.thinking.keep = Some(keep);
        self
    }

    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.prompt_cache_key = Some(key.into());
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoonshotExtras {
    pub choice_usage: Option<Value>,
    pub prompt_tokens_details: Option<Value>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

impl ReplyExtras for MoonshotExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() == MESSAGES_API {
            let MessagesStop {
                stop_reason,
                stop_sequence,
            } = MessagesStop::read("Moonshot", api, raw)?;
            return Ok(Self {
                stop_reason,
                stop_sequence,
                ..Self::default()
            });
        }
        if api.as_str() != "openai.chat" {
            return Ok(Self::default());
        }
        Ok(Self {
            choice_usage: reply_field(raw, "/choices/0/usage")?,
            prompt_tokens_details: reply_field(raw, "/usage/prompt_tokens_details")?,
            ..Self::default()
        })
    }
}
