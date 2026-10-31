use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::codecs::completion::FinishReason;
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils::Lenient;
use crate::codecs::message::{CallId, DocumentRange, Source, SourceLocation, ToolName};
use crate::codecs::observe::ObservedError;
use crate::codecs::operation::{Block, CallFragment, Completion, Finish};
use crate::codecs::providers::internal::set_fields;
use crate::codecs::providers::internal::wire;
use crate::codecs::wire::{
    AdapterVerdict, Decoder, Flow, ObservationSink, Out, WireCitation, WireEvent, WireFrame,
};

const KNOWN_EVENT_TYPES: &[&str] = &[
    "message",
    "message_start",
    "content_block_start",
    "content_block_delta",
    "content_block_stop",
    "message_delta",
    "message_stop",
    "ping",
    "error",
];

#[derive(Debug, Clone, PartialEq)]
pub struct MessagesEvent {
    pub fields: Value,
    raw: Option<String>,
}

impl MessagesEvent {
    fn kind(&self) -> &str {
        self.fields.str("type").unwrap_or_default()
    }

    fn index(&self) -> Result<usize, ProviderError> {
        self.fields
            .u64("index")
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| {
                ProviderError::Response(format!("Anthropic `{}` names no block index", self.kind()))
            })
    }

    fn item(&self, key: &str) -> Result<&Map<String, Value>, ProviderError> {
        match self.fields.get(key) {
            Some(Value::Object(item)) if item.get("type").is_some_and(Value::is_string) => Ok(item),
            _ => Err(ProviderError::Response(format!(
                "Anthropic `{}` carries no `{key}` with a string `type`",
                self.kind()
            ))),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Counts {
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_creation: Option<u64>,
    cache_creation_split: Option<Value>,
    thinking: Option<u64>,
}

impl Counts {
    fn of(usage: Option<&Value>) -> Self {
        let count = |pointer: &str| usage?.at(pointer)?.as_u64_lenient();
        Self {
            input: count("/input_tokens"),
            output: count("/output_tokens"),
            cache_read: count("/cache_read_input_tokens"),
            cache_creation: count("/cache_creation_input_tokens"),
            cache_creation_split: usage
                .and_then(|usage| usage.get("cache_creation"))
                .filter(|split| !split.is_null())
                .cloned(),
            thinking: count("/output_tokens_details/thinking_tokens"),
        }
    }

    fn usage(&self) -> crate::codecs::completion::Usage {
        let input = self.input.map(|uncached| {
            uncached + self.cache_read.unwrap_or(0) + self.cache_creation.unwrap_or(0)
        });
        crate::codecs::completion::Usage {
            input_tokens: input,
            output_tokens: self.output,
            cached_input_tokens: self.cache_read,
            cache_creation_input_tokens: self.cache_creation,
            reasoning_tokens: self.thinking,
            total_tokens: input.zip(self.output).map(|(input, output)| input + output),
            tool_use_prompt_tokens: None,
            cost: None,
        }
    }

    fn write_cost(&self, pricing: &crate::codecs::catalog::Pricing) -> Option<f64> {
        let written = self.cache_creation.unwrap_or(0);
        if written == 0 {
            return Some(0.0);
        }
        let split = self.cache_creation_split.as_ref()?;
        let short = split.get("ephemeral_5m_input_tokens")?.as_u64()?;
        let long = split.get("ephemeral_1h_input_tokens")?.as_u64()?;
        if short.checked_add(long)? != written {
            return None;
        }
        let short_cost = if short == 0 {
            0.0
        } else {
            short as f64 * pricing.cache_write?
        };
        Some((short_cost + long as f64 * pricing.input * 2.0) / 1_000_000.0)
    }
}

fn finish_of(reason: &str, details: Option<&Value>) -> (Option<FinishReason>, Option<String>) {
    let reason = match reason {
        // `pause_turn` is a server-tool loop that stopped at its limit: the
        // turn is replayed as it is to resume it.
        "end_turn" | "stop_sequence" | "pause_turn" => FinishReason::Stop,
        "max_tokens" | "model_context_window_exceeded" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        // A refusal fails the turn with its explanation.
        "refusal" => {
            let explanation = details
                .and_then(|details| details.str("explanation"))
                .filter(|explanation| !explanation.is_empty())
                .unwrap_or("The model refused to complete the request");
            return (
                Some(FinishReason::ContentFilter),
                Some(explanation.to_owned()),
            );
        }
        other => FinishReason::Other(other.to_owned()),
    };
    (Some(reason), None)
}

fn citation_of(citation: &Value) -> Option<WireCitation> {
    let number = |key: &str| citation.u64(key).and_then(|n| u32::try_from(n).ok());
    let range = |start: &str, end: &str| Some(number(start)?..number(end)?);
    let location = match citation.str("type")? {
        "char_location" => SourceLocation::Document {
            index: number("document_index"),
            id: citation.str("file_id").map(str::to_owned),
            within: citation
                .u64("start_char_index")
                .zip(citation.u64("end_char_index"))
                .map(|(start, end)| DocumentRange::Chars(start..end)),
        },
        "page_location" => SourceLocation::Document {
            index: number("document_index"),
            id: citation.str("file_id").map(str::to_owned),
            within: range("start_page_number", "end_page_number").map(DocumentRange::Pages),
        },
        "content_block_location" => SourceLocation::Document {
            index: number("document_index"),
            id: citation.str("file_id").map(str::to_owned),
            within: range("start_block_index", "end_block_index").map(DocumentRange::Blocks),
        },
        "search_result_location" => SourceLocation::SearchResult {
            index: number("search_result_index")?,
            source: citation.str("source")?.to_owned(),
            blocks: range("start_block_index", "end_block_index"),
        },
        "web_search_result_location" => SourceLocation::Url {
            url: citation.str("url")?.to_owned(),
        },
        _ => return None,
    };
    let mut source = Source::new(location);
    if let Some(title) = citation
        .str("document_title")
        .or_else(|| citation.str("title"))
    {
        source = source.title(title);
    }
    if let Some(cited) = citation.str("cited_text") {
        source = source.cited_text(cited);
    }
    Some(WireCitation::new(None, vec![source]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Thinking,
    Redacted,
    Call,
    Opaque,
}

fn replayed_start(
    previous: &[Map<String, Value>],
    block: &Map<String, Value>,
    reconnecting: bool,
) -> bool {
    reconnecting && previous.contains(block)
}

#[derive(Debug)]
pub struct MessagesDecoder {
    open: BTreeMap<usize, (Kind, String)>,
    started: BTreeMap<usize, Vec<Map<String, Value>>>,
    reconnecting: bool,
    replayed: BTreeSet<usize>,
    message_started: bool,
    opened: bool,
    unsigned_thinking: bool,
    cache_ttl_pricing: bool,
    start: Counts,
    message_id: Option<String>,
    response_model: Option<String>,
    container: Option<Value>,
}

impl Default for MessagesDecoder {
    fn default() -> Self {
        Self::new(super::wire::Quirks::anthropic())
    }
}

impl MessagesDecoder {
    pub fn new(quirks: super::wire::Quirks) -> Self {
        Self {
            open: BTreeMap::new(),
            started: BTreeMap::new(),
            reconnecting: false,
            replayed: BTreeSet::new(),
            message_started: false,
            opened: false,
            unsigned_thinking: quirks.unsigned_thinking,
            cache_ttl_pricing: quirks.cache_ttl_pricing,
            start: Counts::default(),
            message_id: None,
            response_model: None,
            container: None,
        }
    }

    fn start(
        &mut self,
        index: usize,
        block: Map<String, Value>,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        if self.open.contains_key(&index) {
            return Ok(());
        }
        if self
            .started
            .get(&index)
            .is_some_and(|previous| replayed_start(previous, &block, self.reconnecting))
        {
            self.replayed.insert(index);
            return Ok(());
        }
        self.replayed.remove(&index);
        self.reconnecting = false;
        self.started.entry(index).or_default().push(block.clone());
        let block = Value::Object(block);
        let kind = block.str("type").unwrap_or_default();
        // A leading `fallback` names the model that took over; one after
        // output began is a fallback that cannot be represented.
        if kind == "fallback" {
            if self.opened {
                return Err(ProviderError::Response(
                    "Anthropic performed an unsupported mid-output model fallback".to_owned(),
                ));
            }
            self.open.insert(index, (Kind::Opaque, String::new()));
            return out.open(index, Block::Opaque { replay: false }, block);
        }
        self.opened = true;
        let (opened, kind, text) = match kind {
            "text" => (Block::Text, Kind::Text, block.str("text")),
            "thinking" => (
                Block::Reasoning { redacted: false },
                Kind::Thinking,
                block.str("thinking"),
            ),
            "redacted_thinking" => (Block::Reasoning { redacted: true }, Kind::Redacted, None),
            "tool_use" => {
                self.open.insert(index, (Kind::Call, String::new()));
                let id = block.str("id").unwrap_or_default().to_owned();
                let input = block.get("input").cloned().unwrap_or_default();
                match ToolName::new(block.str("name").unwrap_or_default()) {
                    Ok(name) => {
                        let id = CallId::from_wire(&id);
                        out.open(index, Block::Call { id, name }, block)?;
                    }
                    // A nameless call: the writer drops it with a warning.
                    Err(_) => out.fragment(
                        Some(index),
                        CallFragment {
                            id: Some(&id),
                            ..CallFragment::default()
                        },
                    )?,
                }
                // A whole reply states the input on the block; a stream
                // streams it, and the fragments win.
                return out.announce(index, input);
            }
            _ => (Block::Opaque { replay: true }, Kind::Opaque, None),
        };
        let text = text.unwrap_or_default().to_owned();
        let citations: Vec<WireCitation> = match kind {
            Kind::Text => block
                .arr("citations")
                .iter()
                .filter_map(citation_of)
                .collect(),
            _ => Vec::new(),
        };
        self.open.insert(index, (kind, String::new()));
        out.open(index, opened, block)?;
        for citation in citations {
            out.cite(index, citation);
        }
        out.push(index, &text)
    }

    fn delta(
        &mut self,
        index: usize,
        delta: &Map<String, Value>,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        if self.replayed.contains(&index) {
            return Ok(());
        }
        let kind = delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // The fragment a block is built from must be text.
        let fragment = |key: &str| {
            delta.get(key).and_then(Value::as_str).ok_or_else(|| {
                ProviderError::Response(format!("Anthropic `{kind}` carries no string `{key}`"))
            })
        };
        // A gateway that skips `content_block_start` still streams the
        // block's text, so the first delta opens it.
        if !self.started.contains_key(&index) {
            let opened = match kind {
                "text_delta" => Some(json!({"type": "text", "text": ""})),
                "thinking_delta" => Some(json!({"type": "thinking", "thinking": ""})),
                _ => None,
            };
            if let Some(Value::Object(block)) = opened {
                self.start(index, block, out)?;
            }
        }
        match kind {
            "text_delta" => out.push(index, fragment("text")?)?,
            "thinking_delta" => out.push(index, fragment("thinking")?)?,
            "input_json_delta" => {
                let fragment = fragment("partial_json")?;
                let Some((kind @ (Kind::Call | Kind::Opaque), json)) = self.open.get_mut(&index)
                else {
                    return Err(ProviderError::Response(format!(
                        "Anthropic streamed input to content block {index}, which takes none"
                    )));
                };
                json.push_str(fragment);
                if *kind == Kind::Call {
                    out.push(index, fragment)?;
                }
                return Ok(());
            }
            "citations_delta" => {
                let citation = delta.get("citation").cloned().unwrap_or_default();
                let cited = citation_of(&citation);
                out.edit(index, |item| {
                    if let Some(item) = item.as_object_mut() {
                        match item.get_mut("citations") {
                            Some(Value::Array(citations)) => citations.push(citation),
                            _ => {
                                item.insert("citations".to_owned(), Value::Array(vec![citation]));
                            }
                        }
                    }
                })?;
                if let Some(cited) = cited {
                    out.cite(index, cited);
                }
                return Ok(());
            }
            _ => {}
        }
        // Text and signatures concatenate; `compaction_delta` and kinds
        // previously unseen fields also land in the item.
        out.edit(index, |item| {
            crate::codecs::operation::completion::merge(item, delta)
        })
    }

    fn stop(&mut self, index: usize, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        if self.replayed.remove(&index) {
            return Ok(());
        }
        let Some((kind, json)) = self.open.remove(&index) else {
            return out.finish(index);
        };
        let streamed =
            (!json.is_empty()).then(|| crate::codecs::json_utils::parse_tool_arguments(&json));
        let unsigned = self.unsigned_thinking;
        let mut complete = true;
        out.edit(index, |item| {
            let input = match streamed {
                Some(Ok(parsed)) => Ok(Some(parsed)),
                Some(Err(_)) => Err(()),
                None => Ok(item.get("input").cloned()),
            };
            let kept = match (kind, input) {
                (Kind::Text, _) => !item.str("text").unwrap_or_default().trim().is_empty(),
                (Kind::Thinking, _) => {
                    unsigned || item.str("signature").is_some_and(|s| !s.is_empty())
                }
                (Kind::Redacted, _) => item.str("data").is_some_and(|data| !data.is_empty()),
                (Kind::Opaque, Ok(None)) => true,
                (Kind::Call | Kind::Opaque, Ok(Some(input @ Value::Object(_)))) => {
                    crate::codecs::operation::completion::merge(
                        item,
                        &set_fields([("input", Some(input))]),
                    );
                    true
                }
                (Kind::Call, Ok(None | Some(Value::Null))) => {
                    crate::codecs::operation::completion::merge(
                        item,
                        &set_fields([("input", Some(json!({})))]),
                    );
                    true
                }
                (Kind::Call, _) => false,
                (Kind::Opaque, _) => {
                    complete = false;
                    true
                }
            };
            if !kept {
                *item = Value::Null;
            }
        })?;
        if complete {
            out.finish(index)
        } else {
            out.close(index)
        }
    }

    fn metadata(&mut self, message: &Value) {
        self.start = Counts::of(message.get("usage"));
        self.message_id = message.str("id").map(str::to_owned);
        self.response_model = message.str("model").map(str::to_owned);
        self.note_container(message.get("container"));
    }

    fn note_container(&mut self, container: Option<&Value>) {
        self.container = container
            .filter(|c| !c.is_null())
            .or(self.container.as_ref())
            .cloned();
    }

    fn end(
        &mut self,
        usage: &Counts,
        stop_reason: Option<&str>,
        details: Option<&Value>,
        mut out: Out<'_, Completion>,
    ) -> Result<Flow, ProviderError> {
        let open: Vec<usize> = self
            .open
            .iter()
            .filter(|(_, (kind, json))| {
                *kind != Kind::Call
                    || json.is_empty()
                    || crate::codecs::json_utils::parse_tool_arguments(json)
                        .is_ok_and(|v| v.is_object())
            })
            .map(|(index, _)| *index)
            .collect();
        for index in open {
            self.stop(index, &mut out)?;
        }
        if let Some(container) = &self.container {
            let index = out.fresh_index();
            let item = json!({ "type": "container", "container": container });
            out.whole(index, Block::Opaque { replay: true }, item, "")?;
        }
        let (reason, error) = stop_reason.map_or((None, None), |reason| finish_of(reason, details));
        let mut counted = usage.usage();
        if self.cache_ttl_pricing {
            counted.cost = out
                .catalog_pricing(self.response_model.as_deref())
                .and_then(|pricing| {
                    let write = usage.write_cost(&pricing);
                    pricing.cost_with_cache_write(&counted, write)
                });
        }
        Ok(out.end(Finish {
            usage: counted,
            reason,
            response_id: self.message_id.clone(),
            model: self.response_model.clone(),
            error,
        }))
    }

    fn whole(
        &mut self,
        mut message: Value,
        mut out: Out<'_, Completion>,
    ) -> Result<Flow, ProviderError> {
        self.metadata(&message);
        let content = match message.get_mut("content").map(Value::take) {
            Some(Value::Array(content)) => content,
            _ => Vec::new(),
        };
        for (index, block) in content.into_iter().enumerate() {
            let block = match block {
                Value::Object(block) if block.get("type").is_some_and(Value::is_string) => block,
                _ => {
                    return Err(ProviderError::Response(format!(
                        "Anthropic content block {index} has no string `type`"
                    )));
                }
            };
            self.start(index, block, &mut out)?;
            self.stop(index, &mut out)?;
        }
        let usage = self.start.clone();
        self.end(
            &usage,
            message.str("stop_reason"),
            message.get("stop_details"),
            out,
        )
    }

    fn terminal(&self, usage: Option<&Value>) -> Counts {
        let (terminal, start) = (Counts::of(usage), self.start.clone());
        Counts {
            // Zero-as-missing is a gateway heuristic for the input count
            // only, not a rule for cache counts.
            input: terminal.input.filter(|tokens| *tokens > 0).or(start.input),
            cache_read: terminal.cache_read.or(start.cache_read),
            cache_creation: terminal.cache_creation.or(start.cache_creation),
            cache_creation_split: terminal.cache_creation_split.or(start.cache_creation_split),
            ..terminal
        }
    }
}

impl<'id> Decoder<'id, Completion> for MessagesDecoder {
    type Event = MessagesEvent;

    fn classify(&self, frame: WireFrame) -> WireEvent<MessagesEvent> {
        let data = frame.as_str();
        wire::classify_tagged_frame::<Value>(&data, "type", |tag| KNOWN_EVENT_TYPES.contains(&tag))
            .map(|fields| {
                // The one event whose payload leaves this crate as bytes rather
                // than as decoded fields, so it is captured where the frame is
                // still in hand.
                let raw = (fields.str("type") == Some("error")).then(|| data.to_string());
                MessagesEvent { fields, raw }
            })
    }

    fn decode(
        &mut self,
        event: MessagesEvent,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        match event.kind() {
            "message" => return self.whole(event.fields, out),
            // A `message_start` without a message body (a Bedrock-compatible
            // gateway sends one) is a no-op.
            "message_start" => {
                if self.message_started && event.fields.get("message").is_some_and(Value::is_object)
                {
                    self.reconnecting = true;
                }
                if !self.message_started
                    && let Some(message) = event.fields.get("message").filter(|m| m.is_object())
                {
                    self.message_started = true;
                    self.metadata(message);
                }
            }
            "content_block_start" => {
                let block = event.item("content_block")?.clone();
                self.start(event.index()?, block, &mut out)?;
            }
            "content_block_delta" => {
                let delta = event.item("delta")?;
                self.delta(event.index()?, delta, &mut out)?;
            }
            "content_block_stop" => self.stop(event.index()?, &mut out)?,
            "message_delta" => {
                let delta = event.fields.get("delta");
                self.note_container(delta.and_then(|delta| delta.get("container")));
                // Only a `message_delta` carrying a stop reason is the
                // provider's end; without one its counters update the
                // running ones, as they do the reassembled reply.
                let Some(reason) = delta.and_then(|delta| delta.str("stop_reason")) else {
                    if let Some(usage) = event.fields.get("usage") {
                        self.start = self.terminal(Some(usage));
                    }
                    return Ok(Flow::More);
                };
                let usage = self.terminal(event.fields.get("usage"));
                let details = delta.and_then(|delta| delta.get("stop_details"));
                return self.end(&usage, Some(reason), details, out);
            }
            // Preserve the complete error envelope rather than re-encode
            // modeled fields.
            "error" => {
                return Err(ProviderError::from_provider_body(
                    event.raw.unwrap_or_default(),
                ));
            }
            // `message_stop`, `ping`, and nothing else: the classifier
            // passes only the listed tags.
            _ => {}
        }
        Ok(Flow::More)
    }
}

impl MessagesDecoder {
    pub(crate) fn project(payload: &[u8], sink: &mut ObservationSink<'_>) {
        let Ok(payload) = serde_json::from_slice::<Value>(payload) else {
            return;
        };
        let fields = payload
            .get("message")
            .filter(|m| m.is_object())
            .unwrap_or(&payload);
        let stop_reason = fields
            .str("stop_reason")
            .or_else(|| payload.at("/delta/stop_reason").and_then(Value::as_str));
        let verdict = AdapterVerdict {
            finish_reason: stop_reason.map(|reason| sink.scrub(reason)),
            block_reason: None,
            detail: None,
            model: fields.str("model").map(|model| sink.scrub(model)),
        };
        let response_id = fields.str("id").map(|id| sink.scrub(id));
        sink.provider(verdict, response_id);
        if let Some(error) = ObservedError::of(&payload) {
            error.emit(sink);
        }
    }
}

pub(crate) mod document;

pub use document::Message as MessagesDocument;
