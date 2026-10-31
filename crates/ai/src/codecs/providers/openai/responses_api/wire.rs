use crate::codecs::catalog::ModelFacts;
use crate::codecs::completion::{self, ProviderCapabilities};
use crate::codecs::error::EncodeError;
use crate::codecs::json_utils::Lenient;
use crate::codecs::observe::ObservedError;
use crate::codecs::operation::Completion;
use crate::codecs::providers::openai::wire::OpenAIConfig;
pub(crate) use crate::codecs::providers::openai::wire::ResponsesContract;
use crate::codecs::wire::{
    AdapterVerdict, Capabilities, Descriptor, Encoded, Framing, Mode, ObservationSink, Wire,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::streaming::ResponsesDecoder;
use super::{ResponsesToolDefinition, SystemInstructionsPlacement};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Responses {
    pub provider: OpenAIConfig,
    pub model: String,
    pub tools: Vec<ResponsesToolDefinition>,
    pub strict_tools: bool,
    pub system_instructions: SystemInstructionsPlacement,
    #[serde(skip)]
    pub facts: ModelFacts,
}

impl Responses {
    pub fn new(provider: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            system_instructions: provider.system_instructions_placement(),
            strict_tools: provider.dialect.quirks.responses.strict_tools_by_default,
            provider,
            model: model.into(),
            tools: Vec::new(),
            facts: ModelFacts::default(),
        }
    }

    pub fn with_facts(mut self, facts: ModelFacts) -> Self {
        self.facts = facts;
        self
    }

    pub fn with_strict_tools(mut self) -> Self {
        self.strict_tools = true;
        self
    }

    pub fn with_tool(mut self, tool: impl Into<ResponsesToolDefinition>) -> Self {
        self.tools.push(tool.into());
        self
    }

    pub fn with_tools<I, Tool>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = Tool>,
        Tool: Into<ResponsesToolDefinition>,
    {
        self.tools.extend(tools.into_iter().map(Into::into));
        self
    }

    pub fn with_system_instructions_placement(
        mut self,
        placement: SystemInstructionsPlacement,
    ) -> Self {
        self.system_instructions = placement;
        self
    }

    pub fn with_system_instructions_as_messages(self) -> Self {
        self.with_system_instructions_placement(SystemInstructionsPlacement::InputSystemMessages)
    }
}

impl Wire for Responses {
    type Op = Completion;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = super::streaming::ResponsesEvent;
    type Decoder<'id> = ResponsesDecoder;
    type Reassembler = super::streaming::document::Response;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
            .model(self.model.as_str())
            .facts(&self.facts)
            .capabilities(Capabilities::completion(
                ProviderCapabilities::default().with_native_output_tool_composition(
                    self.provider.dialect.quirks.responses.contract != ResponsesContract::Xai,
                ),
            ))
            .replay(self)
    }

    fn encode(
        &self,
        request: completion::CompletionRequest,
        mode: Mode,
    ) -> Result<Encoded, EncodeError> {
        let quirks = &self.provider.dialect.quirks.responses;
        // The codex gateway only ever answers with an event stream, and
        // names no content type on it. It is asked for one whatever the
        // caller wanted: the reply is framed the same way either way, and
        // the driver folds it.
        let codex = quirks.contract == ResponsesContract::Codex;
        let streaming = matches!(mode, Mode::Streaming) || codex;
        let builder = self.provider.completion_headers(
            &request,
            http::Request::post(self.provider.uri(quirks.path, None)),
        );
        let mode = if streaming {
            Mode::Streaming
        } else {
            Mode::Unary
        };
        let body = self.responses_request(&request, super::Delivery::Http(mode))?;
        crate::codecs::providers::internal::trace_json(
            crate::codecs::providers::internal::LogTarget::Completions,
            "Responses completion request",
            &body,
        );
        let request = builder
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(body.into_body())?;

        let framing = if streaming {
            Framing::Sse
        } else {
            Framing::Whole
        };
        let encoded = Encoded::new(request, framing)
            .with_request_id_header(self.provider.dialect.request_id_header)
            .with_route(Some(self.provider.dialect.quirks.responses.path))
            .with_projection(project_payload);
        Ok(if codex {
            encoded.with_relaxed_content_type()
        } else {
            encoded
        })
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        ResponsesDecoder::new()
    }
}

impl crate::codecs::completion::ReplayTarget for Responses {
    fn catalog_priced(&self) -> bool {
        self.provider.dialect.quirks.catalog_priced
    }

    fn map_options(
        &self,
        request: &crate::codecs::completion::CompletionRequest,
        fields: crate::codecs::completion::options::OptionFields<'_>,
    ) -> crate::codecs::completion::options::OptionMap {
        crate::codecs::providers::openai::options::responses_options(self, request, fields)
    }

    fn facts(&self) -> Option<&ModelFacts> {
        Some(&self.facts)
    }

    fn api(&self) -> crate::codecs::message::Api {
        crate::codecs::message::Api::from_static("openai.responses")
    }

    fn provider(&self) -> &str {
        self.provider.dialect.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn declares_tools(&self, request: &completion::CompletionRequest) -> bool {
        !self.tools.is_empty() || crate::codecs::completion::history::declares_tools(request)
    }

    fn call_id_slot(&self) -> Option<&'static str> {
        Some("/call_id")
    }

    fn blob_slots(&self) -> &'static [&'static str] {
        &["/encrypted_content"]
    }

    fn identity(&self, item: &Value) -> Map<String, Value> {
        let keys: &[&str] = match item.str("type") {
            Some("message") => &["type", "id", "phase"],
            Some("reasoning") => &["type", "id", "encrypted_content"],
            Some("function_call" | "custom_tool_call") => &["type", "id"],
            _ => &[],
        };
        keys.iter()
            .filter_map(|key| Some(((*key).to_owned(), item.get(*key)?.clone())))
            .collect()
    }

    fn needs_next(&self, item: &Value) -> bool {
        item.str("type") == Some("reasoning")
    }

    fn accepts(&self, model: &str) -> crate::codecs::completion::Accepts {
        let images = reads_images(
            &self.facts,
            self.provider.dialect.quirks.responses.contract,
            model,
        );
        let model = model.to_ascii_lowercase();
        crate::codecs::completion::Accepts {
            user_images: images,
            assistant_images: false,
            tool_result_images: images,
            tools: !(model.starts_with("o1-mini") || model.starts_with("o1-preview")),
            // A reasoning item replays only as the provider made it.
            foreign_reasoning: false,
        }
    }

    fn encodes(&self, _model: &str, media: crate::codecs::completion::Media<'_>) -> bool {
        use crate::codecs::completion::{Media, Place};
        use crate::codecs::message::{DocumentData, DocumentSourceKind as Source};
        let quirks = &self.provider.dialect.quirks;
        let file_ids = quirks.accepts_file_ids;
        match media {
            Media::Image(_, Place::Assistant) | Media::Audio(_) | Media::Video(_) => false,
            Media::Image(image, _) => {
                super::image_part(image).is_some()
                    && (!matches!(image.data, Source::FileId(_))
                        || file_ids && quirks.responses.contract != ResponsesContract::Xai)
            }
            Media::Document(document) => {
                super::document_part(document).is_some()
                    && (file_ids || !matches!(document.data, DocumentData::File(Source::FileId(_))))
            }
        }
    }

    fn continues_stored(&self, request: &completion::CompletionRequest) -> bool {
        ["previous_response_id", "conversation", "input"]
            .iter()
            .any(|key| {
                crate::codecs::completion::options::param(self, request, key)
                    .is_some_and(|value| !value.is_null())
            })
    }

    fn normalize_tool_call_id(
        &self,
        id: &str,
        _model: &str,
        _source: Option<&crate::codecs::message::Origin>,
    ) -> String {
        use crate::codecs::providers::internal::wire_ids::{legal_call_id, short_hash};
        let legal = legal_call_id(id, 64);
        match legal.trim_end_matches('_') {
            // An id with nothing legal in it still names its call.
            "" => short_hash(id),
            trimmed => trimmed.to_owned(),
        }
    }
}

fn reads_images(facts: &ModelFacts, contract: ResponsesContract, model: &str) -> bool {
    let model = model.rsplit('/').next().unwrap_or_default();
    match contract {
        ResponsesContract::Xai => facts.reads_images_or(
            crate::codecs::providers::xai::DIALECT.name,
            model,
            crate::codecs::providers::xai::reads_images,
        ),
        ResponsesContract::OpenAi | ResponsesContract::Codex => facts.reads_images_or(
            crate::codecs::providers::openai::wire::OPENAI.name,
            model,
            crate::codecs::providers::openai::reads_images,
        ),
    }
}

pub(crate) fn project_payload(payload: &[u8], sink: &mut ObservationSink<'_>) {
    let Ok(payload) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    // An error event carries its envelope and nothing else.
    if payload.str("type") != Some("error") {
        project_verdict(&payload, sink);
    }
    if let Some(error) = ObservedError::of(&payload) {
        error.emit(sink);
    }
}

fn project_verdict(payload: &Value, sink: &mut ObservationSink<'_>) {
    let object = payload
        .get("response")
        .filter(|response| response.is_object())
        .unwrap_or(payload);
    // `status` is the provider's verdict; `in_progress` on a stream's
    // opening event is not one yet, so it is left out of the projection.
    let verdict = AdapterVerdict {
        finish_reason: object
            .str("status")
            .filter(|status| *status != "in_progress" && *status != "queued")
            .map(|value| sink.scrub(value)),
        block_reason: None,
        detail: object
            .at("/incomplete_details/reason")
            .and_then(Value::as_str)
            .map(|value| sink.scrub(value)),
        model: object.str("model").map(|value| sink.scrub(value)),
    };
    let response_id = object.str("id").map(|value| sink.scrub(value));
    sink.provider(verdict, response_id);
}
