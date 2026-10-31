use crate::codecs::wire::Wire;

use serde::{Deserialize, Serialize};

use crate::codecs::catalog::ModelFacts;
use crate::codecs::completion::CompletionRequest;
use crate::codecs::error::{EncodeError, ProviderError};
use crate::codecs::operation::Completion;
use crate::codecs::providers::openai::responses_api::streaming::{
    ResponsesDecoder, ResponsesEvent,
};
use crate::codecs::providers::openai::responses_api::wire::Responses;
use crate::codecs::providers::openai::responses_api::{
    ResponsesToolDefinition, SystemInstructionsPlacement,
};
use crate::codecs::wire::document::Reassemble;
use crate::codecs::wire::{Decoder, Descriptor, Encoded, Flow, Mode, Out, WireEvent, WireFrame};

use super::OpenAIConfig;
use super::chat::{Chat, ChatDecoder, ChatEvent};

macro_rules! on_route {
    ($chosen:expr, $wire:ident => $ask:expr) => {
        match $chosen {
            Self::Chat($wire) => $ask,
            Self::Responses($wire) => $ask,
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    Chat,
    Responses,
}

fn catalog_route(spec: &crate::codecs::catalog::ModelSpec) -> Option<Route> {
    let provider = spec.details.provider.as_ref()?;
    match (provider.npm.as_deref(), provider.shape.as_deref()) {
        (_, Some("responses")) | (Some("@ai-sdk/openai"), None) => Some(Route::Responses),
        (_, Some("completions")) | (Some("@ai-sdk/openai-compatible"), None) => Some(Route::Chat),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OpenAiWire {
    Chat(Chat),
    Responses(Responses),
}

impl OpenAiWire {
    pub fn new(provider: OpenAIConfig, model: impl Into<String>) -> Self {
        Self::routed(provider, model, ModelFacts::builtin())
    }

    pub(crate) fn routed(
        provider: OpenAIConfig,
        model: impl Into<String>,
        facts: &ModelFacts,
    ) -> Self {
        let model = model.into();
        let hook = provider
            .dialect
            .quirks
            .hooks
            .and_then(|hooks| hooks.model_route);
        let route = provider
            .route
            .or_else(|| hook.map(|route| route(&model)))
            .or_else(|| match provider.dialect.quirks.routes_by_catalog {
                true => catalog_route(facts.for_model(provider.dialect.name, &model)?),
                false => None,
            })
            .unwrap_or_else(|| provider.completion_route());
        match route {
            Route::Chat => Self::Chat(Chat::new(provider, model)),
            Route::Responses => Self::Responses(Responses::new(provider, model)),
        }
    }

    pub fn provider(&self) -> &OpenAIConfig {
        on_route!(self, wire => &wire.provider)
    }

    pub fn with_strict_tools(self) -> Self {
        self.on_chat(Chat::with_strict_tools)
            .on_responses(Responses::with_strict_tools)
    }

    pub fn with_tool_result_array_content(self) -> Self {
        self.on_chat(Chat::with_tool_result_array_content)
    }

    pub fn with_tool(self, tool: impl Into<ResponsesToolDefinition>) -> Self {
        self.on_responses(|wire| wire.with_tool(tool))
    }

    pub fn with_tools<I, Tool>(self, tools: I) -> Self
    where
        I: IntoIterator<Item = Tool>,
        Tool: Into<ResponsesToolDefinition>,
    {
        self.on_responses(|wire| wire.with_tools(tools))
    }

    pub fn with_system_instructions_placement(
        self,
        placement: SystemInstructionsPlacement,
    ) -> Self {
        self.on_responses(|wire| wire.with_system_instructions_placement(placement))
    }

    pub fn with_system_instructions_as_messages(self) -> Self {
        self.on_responses(Responses::with_system_instructions_as_messages)
    }

    pub fn with_facts(self, facts: ModelFacts) -> Self {
        match self {
            Self::Chat(wire) => Self::Chat(wire.with_facts(facts)),
            Self::Responses(wire) => Self::Responses(wire.with_facts(facts)),
        }
    }

    fn on_chat(self, option: impl FnOnce(Chat) -> Chat) -> Self {
        match self {
            Self::Chat(wire) => Self::Chat(option(wire)),
            responses => responses,
        }
    }

    fn on_responses(self, option: impl FnOnce(Responses) -> Responses) -> Self {
        match self {
            Self::Responses(wire) => Self::Responses(option(wire)),
            chat => chat,
        }
    }
}

impl From<Chat> for OpenAiWire {
    fn from(wire: Chat) -> Self {
        Self::Chat(wire)
    }
}

impl From<Responses> for OpenAiWire {
    fn from(wire: Responses) -> Self {
        Self::Responses(wire)
    }
}

impl Wire for OpenAiWire {
    type Op = Completion;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = OpenAiEvent;
    type Decoder<'id> = OpenAiDecoder;
    type Reassembler = OpenAiReassembler;

    fn describe(&self) -> Descriptor<'_> {
        on_route!(self, wire => wire.describe())
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        on_route!(self, wire => wire.encode(request, mode))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        match self {
            Self::Chat(wire) => OpenAiDecoder::Chat(Box::new(wire.decoder())),
            Self::Responses(wire) => OpenAiDecoder::Responses(wire.decoder()),
        }
    }

    fn reassembler(&self) -> Self::Reassembler {
        match self {
            Self::Chat(wire) => OpenAiReassembler::Chat(wire.reassembler()),
            Self::Responses(wire) => OpenAiReassembler::Responses(wire.reassembler()),
        }
    }
}

pub enum OpenAiReassembler {
    Chat(<Chat as Wire>::Reassembler),
    Responses(<Responses as Wire>::Reassembler),
}

impl Default for OpenAiReassembler {
    fn default() -> Self {
        Self::Chat(Default::default())
    }
}

impl crate::codecs::wire::document::Serves<crate::codecs::operation::Completion>
    for OpenAiReassembler
{
}

impl Reassemble<OpenAiEvent> for OpenAiReassembler {
    fn absorb(&mut self, event: &OpenAiEvent) {
        match (self, event) {
            (Self::Chat(document), OpenAiEvent::Chat(event)) => document.absorb(event),
            (Self::Responses(document), OpenAiEvent::Responses(event)) => document.absorb(event),
            // The route's decoder classifies only its own events.
            _ => {}
        }
    }

    fn finish(self) -> serde_json::Value {
        on_route!(self, document => document.finish())
    }
}

pub enum OpenAiEvent {
    Chat(ChatEvent),
    Responses(ResponsesEvent),
}

pub enum OpenAiDecoder {
    Chat(Box<ChatDecoder>),
    Responses(ResponsesDecoder),
}

impl<'id> Decoder<'id, Completion> for OpenAiDecoder {
    type Event = OpenAiEvent;

    fn classify(&self, frame: WireFrame) -> WireEvent<OpenAiEvent> {
        match self {
            Self::Chat(decoder) => decoder.classify(frame).map(OpenAiEvent::Chat),
            Self::Responses(decoder) => decoder.classify(frame).map(OpenAiEvent::Responses),
        }
    }

    fn decode(
        &mut self,
        event: OpenAiEvent,
        out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        match (self, event) {
            (Self::Chat(decoder), OpenAiEvent::Chat(event)) => decoder.decode(event, out),
            (Self::Responses(decoder), OpenAiEvent::Responses(event)) => decoder.decode(event, out),
            // The driver feeds each decoder only events from its own
            // classifier.
            (Self::Chat(_), OpenAiEvent::Responses(_))
            | (Self::Responses(_), OpenAiEvent::Chat(_)) => Ok(Flow::More),
        }
    }

    fn eof(&mut self, out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        on_route!(self, decoder => decoder.eof(out))
    }
}
