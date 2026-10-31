use serde::Serialize;
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;
use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;
use crate::codecs::providers::anthropic::extension::MessagesStop;
use crate::codecs::providers::anthropic::wire::MESSAGES_API;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XiaomiMimoExt;

impl ProviderExtension for XiaomiMimoExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = XiaomiMimoOptions;
    type Extras = XiaomiMimoExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct XiaomiMimoOptions {}

impl XiaomiMimoOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ExtensionOptions for XiaomiMimoOptions {
    type Ext = XiaomiMimoExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XiaomiMimoExtras {
    pub annotations: Option<Vec<Value>>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

impl ReplyExtras for XiaomiMimoExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() == MESSAGES_API {
            let MessagesStop {
                stop_reason,
                stop_sequence,
            } = MessagesStop::read("Xiaomi MiMo", api, raw)?;
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
            annotations: reply_field(raw, "/choices/0/message/annotations")?,
            ..Self::default()
        })
    }
}
