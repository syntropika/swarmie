use serde::Serialize;
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;
use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;
use crate::codecs::providers::anthropic::extension::MessagesStop;
use crate::codecs::providers::anthropic::wire::MESSAGES_API;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZaiExt;

impl ProviderExtension for ZaiExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = ZaiOptions;
    type Extras = ZaiExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ZaiOptions {
    #[serde(rename = "openai.chat")]
    pub chat: ZaiChat,
}

impl ZaiOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chat(mut self, chat: ZaiChat) -> Self {
        self.chat = chat;
        self
    }

    fn with_chat(mut self, set: impl FnOnce(ZaiChat) -> ZaiChat) -> Self {
        self.chat = set(std::mem::take(&mut self.chat));
        self
    }

    pub fn do_sample(self, sample: bool) -> Self {
        self.with_chat(|chat| chat.do_sample(sample))
    }

    pub fn request_id(self, id: impl Into<String>) -> Self {
        self.with_chat(|chat| chat.request_id(id))
    }

    pub fn user_id(self, id: impl Into<String>) -> Self {
        self.with_chat(|chat| chat.user_id(id))
    }

    pub fn clear_thinking(self, clear: bool) -> Self {
        self.with_chat(|chat| chat.clear_thinking(clear))
    }
}

impl ExtensionOptions for ZaiOptions {
    type Ext = ZaiExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ZaiChat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub do_sample: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "ZaiThinking::is_empty")]
    pub thinking: ZaiThinking,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ZaiThinking {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_thinking: Option<bool>,
}

impl ZaiThinking {
    fn is_empty(&self) -> bool {
        self.clear_thinking.is_none()
    }
}

impl ZaiChat {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn do_sample(mut self, sample: bool) -> Self {
        self.do_sample = Some(sample);
        self
    }

    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    pub fn user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    pub fn clear_thinking(mut self, clear: bool) -> Self {
        self.thinking.clear_thinking = Some(clear);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZaiExtras {
    pub request_id: Option<String>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

impl ReplyExtras for ZaiExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() == MESSAGES_API {
            let MessagesStop {
                stop_reason,
                stop_sequence,
            } = MessagesStop::read("Z.AI", api, raw)?;
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
            request_id: reply_field(raw, "/request_id")?,
            ..Self::default()
        })
    }
}
