mod event;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::codecs::completion::CompletionResponse;
use crate::codecs::error::ErrorReport;
pub use crate::codecs::json_utils::parse_partial_arguments;

use crate::codecs::wasm_compat::WasmBoxedStream;

pub use event::{Item, Part, PartKind, SequenceError, StreamEvent, StreamRecord};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnknownPayload(serde_json::Value);

impl UnknownPayload {
    pub fn new(value: serde_json::Value) -> Self {
        Self(value)
    }

    pub fn value(&self) -> &serde_json::Value {
        &self.0
    }
}

impl std::fmt::Debug for UnknownPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = serde_json::to_vec(&self.0).map_or(0, |json| json.len());
        write!(f, "UnknownPayload({bytes} bytes redacted)")
    }
}

impl From<serde_json::Value> for UnknownPayload {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Relayed {
    Origin(crate::codecs::message::Origin),
    Item(Item<StreamEvent>),
    Done(Arc<CompletionResponse>),
}

pub type StreamEvents = WasmBoxedStream<'static, Result<Relayed, ErrorReport>>;
