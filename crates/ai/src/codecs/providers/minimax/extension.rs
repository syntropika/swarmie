use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;
use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;
use crate::codecs::providers::anthropic::extension::{MessagesStop, Nested};
use crate::codecs::providers::anthropic::wire::MESSAGES_API;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MiniMaxExt;

impl ProviderExtension for MiniMaxExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = MiniMaxOptions;
    type Extras = MiniMaxExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MiniMaxOptions {
    #[serde(rename = "openai.chat")]
    pub chat: MiniMaxChat,
    #[serde(rename = "anthropic.messages")]
    pub messages: MiniMaxMessages,
}

impl MiniMaxOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chat(mut self, chat: MiniMaxChat) -> Self {
        self.chat = chat;
        self
    }

    pub fn messages(mut self, messages: MiniMaxMessages) -> Self {
        self.messages = messages;
        self
    }

    pub fn reasoning_split(mut self, split: bool) -> Self {
        self.chat = std::mem::take(&mut self.chat).reasoning_split(split);
        self
    }

    pub fn metadata_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.messages = std::mem::take(&mut self.messages).metadata_user_id(user_id);
        self
    }
}

impl ExtensionOptions for MiniMaxOptions {
    type Ext = MiniMaxExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MiniMaxChat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_split: Option<bool>,
}

impl MiniMaxChat {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reasoning_split(mut self, split: bool) -> Self {
        self.reasoning_split = Some(split);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MiniMaxMessages {
    pub metadata_user_id: Option<String>,
}

impl MiniMaxMessages {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn metadata_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.metadata_user_id = Some(user_id.into());
        self
    }
}

impl Serialize for MiniMaxMessages {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self { metadata_user_id } = self;
        let mut map = serializer.serialize_map(None)?;
        if let Some(user_id) = metadata_user_id {
            map.serialize_entry("metadata", &Nested("user_id", user_id))?;
        }
        map.end()
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MiniMaxExtras {
    pub reasoning_details: Option<Vec<Value>>,
    pub base_resp: Option<Value>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

impl ReplyExtras for MiniMaxExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() == MESSAGES_API {
            let MessagesStop {
                stop_reason,
                stop_sequence,
            } = MessagesStop::read("MiniMax", api, raw)?;
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
            reasoning_details: reply_field(raw, "/choices/0/message/reasoning_details")?,
            base_resp: reply_field(raw, "/base_resp")?,
            ..Self::default()
        })
    }
}
