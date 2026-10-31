use std::sync::Arc;

use crate::catalog::ModelIdentity;
use reqwest::{Url, header::HeaderMap};
use serde_json::{Value, json};

use crate::codecs::{
    completion::{CompletionRequest, CompletionResponse, OnUnsupported},
    operation::Completion,
    providers::{
        anthropic::AnthropicConfig,
        gemini::GeminiConfig,
        ollama::OllamaConfig,
        openai::{
            OpenAIConfig,
            wire::{DEEPSEEK, MOONSHOT, OPENAI},
        },
    },
    wire::{Body, Encoded, Framing, Mode, Operation, Wire, WireFrame},
};
use crate::{ChatProfile, Error, Failure, Protocol, WireMode};

pub(crate) type ResponseDecoder =
    Arc<dyn Fn(&Value) -> Result<CompletionResponse, Failure> + Send + Sync>;

pub(crate) type StartSpan = Arc<dyn Fn() -> tracing::Span + Send + Sync>;

pub(crate) struct Call {
    pub body: Vec<u8>,
    pub headers: HeaderMap,
    pub decode: ResponseDecoder,
    pub request_id_header: Option<&'static str>,
    pub output_reserve: u64,
    pub start_span: StartSpan,
}

fn refused() -> Error {
    Error::before(Failure::Request)
}

fn base<'a>(endpoint: &'a str, suffix: &str) -> Result<&'a str, Error> {
    endpoint
        .strip_suffix(suffix)
        .filter(|base| !base.is_empty())
        .ok_or_else(refused)
}

pub(crate) fn encode(
    protocol: Protocol,
    identity: &ModelIdentity,
    mut request: CompletionRequest,
    mode: WireMode,
) -> Result<Call, Error> {
    if request
        .model
        .as_deref()
        .is_some_and(|model| model != identity.model)
        || request.max_tokens.is_none_or(|cap| cap == 0)
        || !request.documents.is_empty()
        || request.temperature.is_some_and(|value| !value.is_finite())
        || request
            .options
            .top_p
            .is_some_and(|value| !value.is_finite())
    {
        return Err(refused());
    }
    request.model = Some(identity.model.clone());
    request.options.on_unsupported = Some(OnUnsupported::Error);
    request.validate_message_content().map_err(|_| refused())?;
    crate::codecs::conversation::pairing::validate(&request.chat_history).map_err(|_| refused())?;
    if !matches!(protocol, Protocol::Gemini | Protocol::Ollama) {
        let params = request.additional_params.get_or_insert_with(|| json!({}));
        let params = params.as_object_mut().ok_or_else(refused)?;
        let streaming = json!(mode == WireMode::Stream);
        if params
            .get("stream")
            .is_some_and(|actual| actual != &streaming)
        {
            return Err(refused());
        }
        params.insert("stream".into(), streaming);
    }
    if protocol == Protocol::Responses {
        let params = request.additional_params.get_or_insert_with(|| json!({}));
        let params = params.as_object_mut().ok_or_else(refused)?;
        for (key, value) in [("store", json!(false)), ("truncation", json!("disabled"))] {
            if params.get(key).is_some_and(|actual| actual != &value) {
                return Err(refused());
            }
            params.insert(key.into(), value);
        }
    }
    let model = &identity.model;
    match protocol {
        Protocol::Responses => build(
            OpenAIConfig::new("")
                .with_base_url(base(&identity.endpoint, "/responses")?)
                .responses(model),
            request,
            mode,
            identity,
            protocol,
        ),
        Protocol::ChatCompletions(profile) => {
            let dialect = match profile {
                ChatProfile::OpenAi => &OPENAI,
                ChatProfile::DeepSeek => &DEEPSEEK,
                ChatProfile::Kimi => &MOONSHOT,
            };
            build(
                OpenAIConfig::with_key(dialect, "")
                    .with_base_url(base(&identity.endpoint, dialect.quirks.completion_path)?)
                    .chat(model),
                request,
                mode,
                identity,
                protocol,
            )
        }
        Protocol::Anthropic => build(
            AnthropicConfig::new("")
                .with_base_url(base(&identity.endpoint, "/v1/messages")?)
                .completion(model),
            request,
            mode,
            identity,
            protocol,
        ),
        Protocol::Gemini => build(
            GeminiConfig::new("")
                .with_base_url(base(
                    &identity.endpoint,
                    &format!("/v1beta/models/{model}"),
                )?)
                .completion(model),
            request,
            mode,
            identity,
            protocol,
        ),
        Protocol::Ollama => build(
            OllamaConfig::new()
                .with_base_url(base(&identity.endpoint, "/api/chat")?)
                .native_completion(model),
            request,
            mode,
            identity,
            protocol,
        ),
    }
}

fn grouped_users(
    history: &[Arc<crate::codecs::message::Message>],
) -> Vec<Arc<crate::codecs::message::Message>> {
    use crate::codecs::message::Message;
    let mut grouped: Vec<Arc<Message>> = Vec::with_capacity(history.len());
    for message in history {
        if let (Message::User { content }, Some(previous)) = (message.as_ref(), grouped.last_mut())
            && let Message::User { content: parts } = Arc::make_mut(previous)
        {
            parts.extend(content.iter().cloned());
            continue;
        }
        grouped.push(message.clone());
    }
    grouped
}

fn build<W>(
    wire: W,
    request: CompletionRequest,
    mode: WireMode,
    identity: &ModelIdentity,
    protocol: Protocol,
) -> Result<Call, Error>
where
    W: Wire<Op = Completion, Payload = Encoded, Frame = WireFrame> + Send + Sync + 'static,
{
    let history = request.chat_history.clone();
    let mut request = Completion::prepare(request, &wire.describe()).map_err(|_| refused())?;
    // A codec may group adjacent User messages while validating history. Check
    // that this is its only change, then encode the original message boundaries.
    // Other repairs, role changes and lost native state remain unavailable.
    if request.chat_history != history {
        if request.chat_history != grouped_users(&history) {
            return Err(refused());
        }
        request.chat_history = history;
    }
    let output_reserve = request.max_tokens.ok_or_else(refused)?;
    let schema = request.output_schema.is_some();
    let mode = match mode {
        WireMode::Complete => Mode::Unary,
        WireMode::Stream => Mode::Streaming,
    };
    let encoded = wire.encode(request.clone(), mode).map_err(|_| refused())?;
    let framing = match (mode, protocol) {
        (Mode::Unary, _) => Framing::Whole,
        (Mode::Streaming, Protocol::Ollama) => Framing::Ndjson,
        _ => Framing::Sse,
    };
    if encoded.framing != framing
        || encoded.relaxed_content_type
        || encoded.request.method() != http::Method::POST
    {
        return Err(refused());
    }
    let mut uri = Url::parse(&encoded.request.uri().to_string()).map_err(|_| refused())?;
    if protocol == Protocol::Gemini {
        // The pure encoder receives an empty key. Physical authentication uses the
        // captured header, never a query credential or an encoder-owned secret.
        let pairs = uri
            .query_pairs()
            .filter(|(key, value)| !(key == "key" && value.is_empty()))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        uri.set_query(None);
        if !pairs.is_empty() {
            uri.query_pairs_mut().extend_pairs(pairs);
        }
    }
    let endpoint = Url::parse(&identity.endpoint).map_err(|_| refused())?;
    let expected = protocol.destination(
        &endpoint,
        match mode {
            Mode::Unary => WireMode::Complete,
            Mode::Streaming => WireMode::Stream,
        },
        identity,
    )?;
    if uri != expected {
        return Err(refused());
    }
    let request_id_header = encoded.request_id_header;
    let (parts, body) = encoded.request.into_parts();
    let Body::Bytes(body) = body else {
        return Err(refused());
    };
    if schema {
        let body: Value = serde_json::from_slice(&body).map_err(|_| refused())?;
        let paths: &[&str] = match protocol {
            Protocol::Responses => &["/text/format"],
            Protocol::ChatCompletions(_) => &["/response_format"],
            Protocol::Anthropic => &["/output_config/format", "/output_format"],
            Protocol::Gemini => &[
                "/generationConfig/responseJsonSchema",
                "/generationConfig/responseSchema",
            ],
            Protocol::Ollama => &["/format"],
        };
        if !paths
            .iter()
            .any(|path| body.pointer(path).is_some_and(|value| !value.is_null()))
        {
            return Err(refused());
        }
    }
    let mut headers = parts.headers;
    for key in ["authorization", "x-api-key", "x-goog-api-key", "api-key"] {
        headers.remove(key);
    }
    let provider = wire.describe().name.to_owned();
    let request = Arc::new(request);
    let observed = request.clone();
    let start_span = Arc::new(move || {
        let span = crate::completion_parent_span!(
            target: "swarmie_ai::completions",
            name: "provider.completion",
            operation: if protocol == Protocol::Gemini { "generate_content" } else { "chat" },
            system_instructions: crate::codecs::telemetry::Empty
        );
        span.record("gen_ai.provider.name", provider.as_str());
        if let Some(model) = observed.model.as_deref() {
            span.record("gen_ai.request.model", model);
        }
        span.record("gen_ai.request.stream", mode == Mode::Streaming);
        crate::codecs::telemetry::record_model_input(
            &span,
            &observed.chat_history,
            observed.record_telemetry_content,
        );
        crate::codecs::telemetry::record_tool_definitions(
            &span,
            &observed,
            observed.record_telemetry_content,
        );
        span
    });
    let decode = Arc::new(move |raw: &Value| {
        let bytes = serde_json::to_vec(raw).map_err(|_| Failure::Protocol)?;
        let response =
            crate::codecs::wire::decode_reply(&wire, &request, mode, [WireFrame::Bytes(bytes)])
                .map_err(|_| Failure::Protocol)?;
        crate::codecs::telemetry::record_model_output(
            &tracing::Span::current(),
            &response.choice,
            request.record_telemetry_content,
        );
        Ok(response)
    });
    Ok(Call {
        body,
        headers,
        decode,
        request_id_header,
        output_reserve,
        start_span,
    })
}
