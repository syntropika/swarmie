use serde::Deserialize;
use serde_json::{Map, Value};

use crate::codecs::completion::{FinishReason, Usage};
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils::Lenient;
use crate::codecs::message::{CallId, ToolName};
use crate::codecs::observe::ObservedError;
use crate::codecs::operation::{Block, Completion, Finish};
use crate::codecs::providers::internal::wire;
use crate::codecs::wire::{
    AdapterVerdict, Decoder, Flow, ObservationSink, Out, WireEvent, WireFrame,
};

const RECORD_KEYS: &[&str] = &["message", "done", "error"];

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

#[derive(Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct ChatRecord(pub Map<String, Value>);

#[derive(Debug, Default)]
pub struct ChatDecoder {
    split: Split,
    called: bool,
    model: Option<String>,
}

#[derive(Debug)]
enum Split {
    Opening(String),
    Inside { held: String, started: bool },
    Text { trim: bool },
}

impl Default for Split {
    fn default() -> Self {
        Self::Opening(String::new())
    }
}

fn partial_suffix(text: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&len| text.ends_with(&tag[..len]))
        .unwrap_or(0)
}

impl<'id> Decoder<'id, Completion> for ChatDecoder {
    type Event = ChatRecord;

    fn classify(&self, frame: WireFrame) -> WireEvent<ChatRecord> {
        wire::classify_marker_keyed_frame(&frame.as_str(), RECORD_KEYS)
    }

    fn decode(
        &mut self,
        ChatRecord(fields): ChatRecord,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        let record = Value::Object(fields);
        if let Some(error) = record.get("error").filter(|error| !error.is_null()) {
            let body = serde_json::json!({ "error": error }).to_string();
            return Err(ProviderError::from_provider_body(body));
        }
        if let Some(model) = record.str("model").filter(|model| !model.is_empty())
            && self.model.as_deref() != Some(model)
        {
            tracing::Span::current().record("gen_ai.response.model", model);
            self.model = Some(model.to_owned());
        }
        let message = record.get("message").unwrap_or(&Value::Null);
        if let Some(thinking) = message
            .str("thinking")
            .filter(|thinking| !thinking.is_empty())
        {
            // A reply with native thinking splits nothing out of its content.
            self.release(&mut out)?;
            reason(thinking, &mut out)?;
        }
        if let Some(content) = message.str("content").filter(|content| !content.is_empty()) {
            self.content(content, &mut out)?;
        }
        for call in message.arr("tool_calls") {
            self.call(call, &mut out)?;
        }
        if record.bool("done") != Some(true) {
            return Ok(Flow::More);
        }
        self.release(&mut out)?;
        out.end_run()?;
        let reason = record.str("done_reason").map(|reason| match reason {
            "stop" if self.called => FinishReason::ToolCalls,
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            other => FinishReason::Other(other.to_owned()),
        });
        let (input, output) = (record.u64("prompt_eval_count"), record.u64("eval_count"));
        let usage = Usage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input.zip(output).map(|(input, output)| input + output),
            cached_input_tokens: record.u64("prompt_eval_cached_count"),
            ..Usage::default()
        };
        Ok(out.end(Finish {
            usage,
            reason,
            model: self.model.take(),
            ..Finish::default()
        }))
    }
}

impl ChatDecoder {
    fn content(
        &mut self,
        content: &str,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        match std::mem::replace(&mut self.split, Split::Text { trim: false }) {
            Split::Text { trim } => {
                let text = if trim { content.trim_start() } else { content };
                self.split = Split::Text {
                    trim: trim && text.is_empty(),
                };
                if !text.is_empty() {
                    out.run(Block::Text, text)?;
                }
                Ok(())
            }
            Split::Opening(mut held) => {
                held.push_str(content);
                let trimmed = held.trim_start();
                if let Some(rest) = trimmed.strip_prefix(THINK_OPEN) {
                    let rest = rest.to_owned();
                    self.split = Split::Inside {
                        held: String::new(),
                        started: false,
                    };
                    self.content(&rest, out)
                } else if THINK_OPEN.starts_with(trimmed) {
                    self.split = Split::Opening(held);
                    Ok(())
                } else {
                    out.run(Block::Text, &held)?;
                    Ok(())
                }
            }
            Split::Inside {
                mut held,
                mut started,
            } => {
                held.push_str(content);
                if let Some((reasoning, rest)) = held.split_once(THINK_CLOSE) {
                    write_reasoning(reasoning.trim_end(), started, out)?;
                    self.split = Split::Text { trim: true };
                    return self.content(rest, out);
                }
                let cut = held.len() - partial_suffix(&held, THINK_CLOSE);
                let cut = held[..cut].trim_end().len();
                started |= write_reasoning(&held[..cut], started, out)?;
                self.split = Split::Inside {
                    held: held.split_off(cut),
                    started,
                };
                Ok(())
            }
        }
    }

    fn release(&mut self, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        match std::mem::replace(&mut self.split, Split::Text { trim: false }) {
            Split::Opening(held) if !held.is_empty() => {
                out.run(Block::Text, &held)?;
            }
            Split::Inside { held, started } => {
                write_reasoning(held.trim_end(), started, out)?;
            }
            Split::Opening(_) | Split::Text { .. } => {}
        }
        Ok(())
    }

    fn call(&mut self, call: &Value, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let Ok(name) = ToolName::new(
            call.at("/function/name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ) else {
            tracing::warn!("Ollama sent a tool call without a name; nothing can answer it");
            return Ok(());
        };
        let arguments = match call.at("/function/arguments") {
            None | Some(Value::Null) => "{}".to_owned(),
            Some(Value::String(arguments)) => arguments.clone(),
            Some(arguments) => arguments.to_string(),
        };
        self.release(out)?;
        out.end_run()?;
        self.called = true;
        let id = CallId::from_wire(call.str("id").unwrap_or_default());
        let index = out.fresh_index();
        out.open(index, Block::Call { id, name }, call.clone())?;
        out.push(index, &arguments)?;
        out.finish(index)
    }

    pub(crate) fn project(payload: &[u8], sink: &mut ObservationSink<'_>) {
        let Ok(record) = serde_json::from_slice::<Value>(payload) else {
            return;
        };
        let verdict = match record.str("done_reason") {
            Some(reason) => AdapterVerdict {
                finish_reason: Some(sink.scrub(reason)),
                block_reason: None,
                detail: None,
                model: record.str("model").map(|model| sink.scrub(model)),
            },
            None => AdapterVerdict::default(),
        };
        sink.provider(verdict, None);
        if let Some(error) = ObservedError::of(&record) {
            error.emit(sink);
        }
    }
}

fn write_reasoning(
    text: &str,
    started: bool,
    out: &mut Out<'_, Completion>,
) -> Result<bool, ProviderError> {
    let text = if started { text } else { text.trim_start() };
    if text.is_empty() {
        return Ok(false);
    }
    reason(text, out)?;
    Ok(true)
}

fn reason(text: &str, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
    let index = out.run(Block::Reasoning { redacted: false }, text)?;
    out.edit(index, |item| match item {
        Value::Object(fields) => {
            if let Some(Value::String(thinking)) = fields.get_mut("thinking") {
                thinking.push_str(text);
            }
        }
        _ => *item = serde_json::json!({ "thinking": text }),
    })
}

pub(crate) mod document;
