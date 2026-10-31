use serde_json::{Map, Value};

use super::completion::blocked_prompt_error;
use super::completion::{map_google_finish_reason, usage_of};
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils::Lenient;
use crate::codecs::message::{CallId, DocumentSourceKind, Image, MediaType, MimeType, ToolName};
use crate::codecs::operation::{Block, Completion, Finish};
use crate::codecs::providers::internal::wire;
use crate::codecs::wire::{
    AdapterVerdict, Decoder, Flow, ObservationSink, Out, WireCitation, WireEvent, WireFrame,
};

const RECOGNIZABLE_CHUNK_KEYS: &[&str] =
    &["candidates", "usageMetadata", "promptFeedback", "error"];

#[derive(Debug, Default, serde::Deserialize)]
#[serde(transparent)]
pub struct GenerateContentChunk(pub Map<String, Value>);

#[derive(Debug, Default)]
pub struct GenerateContentDecoder {
    finish: Option<String>,
    usage: Option<Value>,
    model_version: Option<String>,
    response_id: Option<String>,
    open: Option<(usize, bool)>,
    signed: bool,
    last: Option<usize>,
    signature: Option<String>,
    answer: grounding::AnswerText,
    placed: Vec<Option<(usize, usize)>>,
    placement: Option<(usize, usize)>,
    grounding: Vec<(usize, WireCitation)>,
    chunks: usize,
    recitations: Vec<(usize, WireCitation)>,
}

impl<'id> Decoder<'id, Completion> for GenerateContentDecoder {
    type Event = GenerateContentChunk;

    fn classify(&self, frame: WireFrame) -> WireEvent<GenerateContentChunk> {
        match wire::classify_marker_keyed_frame(&frame.as_str(), RECOGNIZABLE_CHUNK_KEYS) {
            chunk @ WireEvent::Known(_) => chunk,
            // ID-only metadata must not create an unknown-content truncation tail.
            _ if Self::is_analysis_only(&frame) => {
                wire::classify_untyped_line(frame.as_str().as_bytes())
            }
            event => event,
        }
    }

    fn decode(
        &mut self,
        GenerateContentChunk(data): GenerateContentChunk,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        let mut data = Value::Object(data);
        // The parts move into their blocks; the rest of the chunk is read
        // in place.
        let parts = data
            .pointer_mut("/candidates/0/content/parts")
            .map(Value::take);
        // Gemini repeats both on every chunk. Record a value when it changes:
        // every span record counts against the exporter's attribute limit.
        let span = tracing::Span::current();
        if let Some(id) = data.str("responseId").filter(|id| !id.is_empty())
            && self.response_id.as_deref() != Some(id)
        {
            span.record("gen_ai.response.id", id);
            self.response_id = Some(id.to_owned());
        }
        if let Some(model) = data.str("modelVersion").filter(|model| !model.is_empty())
            && self.model_version.as_deref() != Some(model)
        {
            span.record("gen_ai.response.model", model);
            self.model_version = Some(model.to_owned());
        }
        if let Some(usage) = data.get_mut("usageMetadata") {
            self.usage = Some(usage.take());
        }
        if let Some(error) = data.at("/error") {
            // An in-band failure is a provider error, not a truncation.
            let body = serde_json::json!({ "error": error }).to_string();
            return Err(ProviderError::from_provider_body(body));
        }
        if let Some(blocked) = data.get("promptFeedback").and_then(blocked_prompt_error) {
            return Err(blocked);
        }
        // The candidates, their content and its parts hold every block and
        // the finish, so a wrongly typed one fails the reply.
        let candidate = match data
            .get("candidates")
            .map(|candidates| (candidates, candidates.get(0)))
        {
            None | Some((Value::Null, _) | (Value::Array(_), None)) => return Ok(Flow::More),
            Some((Value::Array(_), Some(candidate @ Value::Object(_)))) => candidate,
            Some((Value::Array(_), Some(_))) => {
                return Err(malformed("a candidate that is not an object"));
            }
            Some(_) => return Err(malformed("candidates that are not a list")),
        };
        // Last one wins: an intermediate `finishReason` is superseded by the
        // reason the turn actually ended on. Proto3 JSON spells an enum
        // value its schema does not know as its number.
        match candidate.get("finishReason") {
            Some(Value::String(name)) => self.finish = Some(name.clone()),
            Some(Value::Number(number)) => self.finish = Some(format!("FINISH_REASON_{number}")),
            _ => {}
        }
        let parts = match candidate.get("content") {
            None | Some(Value::Null) => None,
            Some(Value::Object(_)) => parts,
            Some(_) => return Err(malformed("candidate content that is not an object")),
        };
        match parts {
            None | Some(Value::Null) => {}
            Some(Value::Array(parts)) => {
                self.chunks += 1;
                self.placed.clear();
                for part in parts {
                    self.part(part, &mut out)?;
                    self.placed.push(self.placement.take());
                }
            }
            Some(_) => return Err(malformed("candidate parts that are not a list")),
        }
        // Each chunk restates the grounding whole; recitation sources add up.
        if let Some(metadata) = candidate.get("groundingMetadata") {
            self.grounding =
                grounding::grounding(metadata, &self.placed, &self.answer, self.chunks > 1);
        }
        if let Some(metadata) = candidate.get("citationMetadata") {
            let recitations = grounding::recitations(metadata, &self.answer);
            self.recitations.extend(recitations);
        }
        // A failure is final: nothing after it is read.
        use crate::codecs::completion::FinishReason::{Length, Stop};
        let reason = self.finish.as_deref().map(map_google_finish_reason);
        match (reason, candidate.get("finishReason")) {
            (None | Some(Stop | Length), _) | (_, None) => Ok(Flow::More),
            _ => self.end(out),
        }
    }

    fn eof(&mut self, out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        self.end(out)
    }
}

impl<'id> Decoder<'id, Completion, GenerateContentChunk> for GenerateContentDecoder {
    type Event = GenerateContentChunk;

    fn classify(&self, chunk: GenerateContentChunk) -> WireEvent<GenerateContentChunk> {
        WireEvent::Known(chunk)
    }

    fn decode(
        &mut self,
        chunk: GenerateContentChunk,
        out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        Decoder::<'id, Completion>::decode(self, chunk, out)
    }

    fn eof(&mut self, out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        self.end(out)
    }
}

impl GenerateContentDecoder {
    fn end(&mut self, mut out: Out<'_, Completion>) -> Result<Flow, ProviderError> {
        let Some(reason) = self.finish.take() else {
            return Err(ProviderError::Truncated);
        };
        self.close(&mut out)?;
        let citations = std::mem::take(&mut self.grounding)
            .into_iter()
            .chain(std::mem::take(&mut self.recitations));
        for (index, citation) in citations {
            out.cite(index, citation);
        }
        let usage = self.usage.as_ref().map(usage_of).unwrap_or_default();
        let model = self.model_version.take();
        let response_id = self.response_id.take();
        Ok(out.end(Finish {
            usage,
            reason: Some(map_google_finish_reason(&reason)),
            response_id,
            model,
            ..Finish::default()
        }))
    }

    fn close(&mut self, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        self.open
            .take()
            .map_or(Ok(()), |(index, _)| out.finish(index))
    }

    fn open(
        &mut self,
        block: Block,
        item: Value,
        text: &str,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        self.close(out)?;
        let index = out.fresh_index();
        let mut item = item;
        if let (Some(signature), Some(fields)) = (self.signature.take(), item.as_object_mut()) {
            fields
                .entry("thoughtSignature")
                .or_insert(Value::String(signature));
        }
        self.last = Some(index);
        self.signed = item
            .get("thoughtSignature")
            .and_then(Value::as_str)
            .is_some_and(|signature| !signature.is_empty());
        if matches!(block, Block::Text) {
            self.answer.open(index, text);
            self.placement = Some((index, 0));
        }
        let run = match &block {
            Block::Text => Some(false),
            Block::Reasoning { .. } => Some(true),
            _ => None,
        };
        out.open(index, block, item)?;
        out.push(index, text)?;
        self.open = run.map(|thought| (index, thought));
        self.open.map_or_else(|| out.finish(index), |_| Ok(()))
    }

    fn part(&mut self, part: Value, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let Value::Object(fields) = &part else {
            return self.open(Block::Opaque { replay: false }, part, "", out);
        };
        let thought = part.bool("thought") == Some(true);
        let signature = part
            .str("thoughtSignature")
            .filter(|signature| !signature.is_empty());
        if let Some(text) = part.str("text") {
            // An empty part without a signature carries nothing.
            if text.is_empty() && signature.is_none() {
                return Ok(());
            }
            // A second signed part starts a block of its own, so each
            // signature stays with the part that carried it.
            return match self.open {
                Some((index, kind)) if kind == thought && !(self.signed && signature.is_some()) => {
                    self.signed |= signature.is_some();
                    if !thought {
                        self.placement = self.answer.push(index, text).map(|at| (index, at));
                    }
                    out.push(index, text)?;
                    out.edit(index, |item| merge_part(item, &part))
                }
                _ => {
                    let block = match thought {
                        true => Block::Reasoning { redacted: false },
                        false => Block::Text,
                    };
                    let text = text.to_owned();
                    self.open(block, part, &text, out)
                }
            };
        }
        if let Some(call) = part.obj("functionCall") {
            let Ok(name) =
                ToolName::new(call.get("name").and_then(Value::as_str).unwrap_or_default())
            else {
                tracing::warn!("Gemini sent a function call without a name; nothing can answer it");
                return Ok(());
            };
            // Proto3 JSON leaves out an empty `args` Struct.
            let args = call
                .get("args")
                .map_or_else(|| "{}".to_owned(), Value::to_string);
            let id = CallId::from_wire(call.get("id").and_then(Value::as_str).unwrap_or_default());
            return self.open(Block::Call { id, name }, part, &args, out);
        }
        if !thought
            && let (Some(mime_type), Some(data)) =
                (part.at("/inlineData/mimeType"), part.at("/inlineData/data"))
            && let (Some(mime_type), Some(data)) = (mime_type.as_str(), data.as_str())
            && let Some(MediaType::Image(media_type)) = MediaType::from_mime_type(mime_type)
        {
            let image = Image {
                data: DocumentSourceKind::Base64(data.to_owned()),
                media_type: Some(media_type),
                detail: None,
                native: None,
            };
            return self.open(Block::Image(image), part, "", out);
        }
        let bare = ["thought", "thoughtSignature", "partMetadata"];
        let data = fields.keys().any(|key| !bare.contains(&key.as_str()));
        if !data && let Some(signature) = signature {
            let Some(index) = self.open.map(|(index, _)| index).or(self.last) else {
                self.signature = Some(signature.to_owned());
                return Ok(());
            };
            return out.edit(index, |item| {
                if let Some(item) = item.as_object_mut() {
                    item.insert("thoughtSignature".to_owned(), Value::from(signature));
                }
            });
        }
        self.open(Block::Opaque { replay: data }, part, "", out)
    }

    pub(crate) fn is_analysis_only(frame: &WireFrame) -> bool {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ResponseIdOnly {
            #[serde(rename = "responseId")]
            _id: String,
        }
        matches!(
            wire::classify_untyped_line::<ResponseIdOnly>(frame.as_str().as_bytes()),
            WireEvent::Known(_)
        )
    }

    pub(crate) fn project(payload: &[u8], sink: &mut ObservationSink<'_>) {
        let Ok(reply) = serde_json::from_slice::<Value>(payload) else {
            return;
        };
        let candidate = reply.arr("candidates").first().unwrap_or(&Value::Null);
        let scrub = |value: Option<&str>| value.map(|value| sink.scrub(value));
        let block = reply
            .at("/promptFeedback/blockReason")
            .and_then(Value::as_str);
        let verdict = AdapterVerdict {
            finish_reason: scrub(candidate.str("finishReason")),
            block_reason: scrub(block),
            detail: scrub(candidate.str("finishMessage")),
            model: scrub(reply.str("modelVersion")),
        };
        let response_id = scrub(reply.str("responseId"));
        sink.provider(verdict, response_id);
        if let Some(error) = crate::codecs::observe::ObservedError::of(&reply) {
            error.emit(sink);
        }
    }
}

fn malformed(what: &str) -> ProviderError {
    ProviderError::Response(format!("Gemini sent {what}"))
}

fn merge_part(item: &mut Value, part: &Value) {
    let (Value::Object(held), Value::Object(part)) = (&mut *item, part) else {
        *item = part.clone();
        return;
    };
    for (key, value) in part {
        if key == "text"
            && let (Some(Value::String(text)), Value::String(more)) = (held.get_mut("text"), value)
        {
            text.push_str(more);
        } else if key != "thoughtSignature" || value.as_str() != Some("") {
            held.insert(key.clone(), value.clone());
        }
    }
}

pub mod document;
mod grounding;
