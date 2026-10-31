use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::codecs::completion::{Cost, FinishReason, Usage};
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils::Lenient;
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::message::{Source, SourceLocation};
use crate::codecs::operation::{Block, CallFragment, Completion, Finish};
use crate::codecs::providers::internal::wire;
use crate::codecs::wire::{Decoder, Flow, Out, WireCitation, WireEvent, WireFrame};

fn is_known_responses_event_type(kind: &str) -> bool {
    matches!(kind, "error" | "keepalive")
        || is_lifecycle_event(kind)
        || matches!(
            kind.strip_prefix("response."),
            Some(
                "output_item.added"
                    | "output_item.done"
                    | "content_part.added"
                    | "content_part.done"
                    | "output_text.delta"
                    | "output_text.done"
                    | "refusal.delta"
                    | "refusal.done"
                    | "function_call_arguments.delta"
                    | "function_call_arguments.done"
                    | "custom_tool_call_input.delta"
                    | "custom_tool_call_input.done"
                    | "reasoning_summary_part.added"
                    | "reasoning_summary_part.done"
                    | "reasoning_summary_text.delta"
                    | "reasoning_summary_text.done"
                    | "reasoning_text.delta"
                    | "reasoning_text.done"
            )
        )
}

pub(crate) fn is_lifecycle_event(kind: &str) -> bool {
    matches!(
        kind.strip_prefix("response."),
        Some("created" | "queued" | "in_progress" | "completed" | "failed" | "incomplete")
    )
}

#[derive(Debug)]
pub enum ResponsesEvent {
    Frame {
        kind: String,
        frame: Value,
        raw: Option<String>,
    },
    Whole(Value),
    Failure(String),
    Sentinel,
}

const WHOLE_BODY_MARKERS: &[&str] = &["object", "output", "status", "id", "error"];

struct Tagged(Value);

impl<'de> serde::Deserialize<'de> for Tagged {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.str("type").is_some() {
            Ok(Self(value))
        } else {
            Err(serde::de::Error::custom("the payload names no `type`"))
        }
    }
}

pub fn classify_responses_payload(data: &str) -> WireEvent<ResponsesEvent> {
    if data.trim() == "[DONE]" {
        return WireEvent::Known(ResponsesEvent::Sentinel);
    }
    wire::classify_or_untagged(
        data,
        "type",
        is_known_responses_event_type,
        |Tagged(frame)| match frame.str("type") {
            Some("error") => ResponsesEvent::Failure(data.to_owned()),
            kind => ResponsesEvent::Frame {
                raw: (kind == Some("response.failed")).then(|| data.to_owned()),
                kind: kind.unwrap_or_default().to_owned(),
                frame,
            },
        },
        |data| {
            wire::classify_marker_keyed_frame::<Value>(data, WHOLE_BODY_MARKERS).map(|body| {
                let error = body.get("error").is_some_and(|error| !error.is_null());
                if error && body.get("output").is_none() && body.get("status").is_none() {
                    ResponsesEvent::Failure(data.to_owned())
                } else {
                    ResponsesEvent::Whole(body)
                }
            })
        },
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Message,
    Reasoning,
    Call,
    Opaque,
}

impl Kind {
    fn of(item: &Value) -> Self {
        match item.str("type") {
            Some("message") => Self::Message,
            Some("reasoning") => Self::Reasoning,
            Some("function_call" | "custom_tool_call") => Self::Call,
            _ => Self::Opaque,
        }
    }
}

struct Slot {
    at: usize,
    kind: Kind,
    id: Option<String>,
    open: bool,
    text: String,
    field: Option<&'static str>,
    part: u64,
    arguments: String,
    custom: bool,
    named: bool,
    titled: bool,
}

const CLIENT_EXECUTED: &[&str] = &[
    "computer_call",
    "local_shell_call",
    "shell_call",
    "apply_patch_call",
    "mcp_approval_request",
];

fn item_id(item: &Value) -> Option<&str> {
    item.str("id").filter(|id| !id.is_empty())
}

fn arguments_of(item: &Value) -> Option<String> {
    if item.str("type") == Some("custom_tool_call") {
        return item
            .str("input")
            .map(|input| json!({ "input": input }).to_string());
    }
    match item.get("arguments")? {
        Value::String(arguments) if arguments.is_empty() => None,
        Value::String(arguments) => Some(arguments.clone()),
        Value::Null => None,
        arguments => Some(arguments.to_string()),
    }
}

fn text_of(item: &Value) -> String {
    let texts = |key: &str| -> Vec<&str> {
        match item.get(key) {
            Some(Value::String(text)) => vec![text.as_str()],
            _ => item
                .arr(key)
                .iter()
                .filter_map(|part| {
                    part.as_str()
                        .or_else(|| part.str("text"))
                        .or_else(|| part.str("refusal"))
                })
                .collect(),
        }
    };
    match Kind::of(item) {
        Kind::Message => texts("content").concat(),
        Kind::Reasoning => Some(texts("summary"))
            .filter(|summary| !summary.is_empty())
            .unwrap_or_else(|| texts("content"))
            .join("\n\n"),
        Kind::Call | Kind::Opaque => String::new(),
    }
}

fn stating(mut item: Value, kind: Kind, text: &str) -> Value {
    let (key, part) = match kind {
        Kind::Message => (
            "content",
            json!({"type": "output_text", "text": text, "annotations": []}),
        ),
        _ => ("summary", json!({"type": "summary_text", "text": text})),
    };
    if let Some(fields) = item.as_object_mut() {
        fields.insert(key.to_owned(), json!([part]));
    }
    item
}

fn provider_message(response: &Value, fallback: &str) -> String {
    let parts: Vec<&str> = ["/error/code", "/error/message"]
        .iter()
        .filter_map(|pointer| response.at(pointer).and_then(Value::as_str))
        .collect();
    if parts.is_empty() {
        fallback.to_owned()
    } else {
        parts.join(": ")
    }
}

pub(crate) fn finish_reason_of(response: &Value) -> (FinishReason, Option<String>) {
    let reason = response
        .at("/incomplete_details/reason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty());
    let other = |reason: &str, error: String| (FinishReason::Other(reason.to_owned()), Some(error));
    match response.str("status") {
        None | Some("completed") => (FinishReason::Stop, None),
        Some("incomplete") => match reason {
            Some("max_output_tokens") => (FinishReason::Length, None),
            Some("content_filter") => (FinishReason::ContentFilter, None),
            Some(reason) => other(
                &format!("incomplete: {reason}"),
                format!("Response incomplete: {reason}"),
            ),
            None => other(
                "incomplete",
                "Response incomplete without a provider reason".to_owned(),
            ),
        },
        Some(status @ ("failed" | "cancelled")) => other(
            status,
            provider_message(response, &format!("Response {status}")),
        ),
        Some(status @ ("queued" | "in_progress")) => {
            other(status, format!("Response ended while {status}"))
        }
        Some(status) => other(
            status,
            format!("Response ended with the unknown status `{status}`"),
        ),
    }
}

pub(crate) fn usage_of(usage: &Value) -> Usage {
    let count = |pointer: &str| usage.at(pointer).and_then(Lenient::as_u64_lenient);
    Usage {
        input_tokens: count("/input_tokens"),
        output_tokens: count("/output_tokens"),
        total_tokens: count("/total_tokens"),
        cached_input_tokens: count("/input_tokens_details/cached_tokens"),
        cache_creation_input_tokens: count("/input_tokens_details/cache_write_tokens"),
        reasoning_tokens: count("/output_tokens_details/reasoning_tokens"),
        ..Usage::default()
    }
    .cost(reported_cost(usage))
}

const TICKS_PER_USD: f64 = 1e10;

fn reported_cost(usage: &Value) -> Option<Cost> {
    let total = match usage.u64("cost_in_usd_ticks") {
        Some(ticks) => ticks as f64 / TICKS_PER_USD,
        None => usage.f64("cost").filter(|cost| cost.is_finite())?,
    };
    Some(Cost::from_total(total))
}

fn citations_of(item: &Value) -> Vec<WireCitation> {
    item.arr("content")
        .iter()
        .flat_map(|part| part.arr("annotations"))
        .filter_map(source_of)
        .map(|source| WireCitation::new(None, vec![source]))
        .collect()
}

fn source_of(annotation: &Value) -> Option<Source> {
    let text = |key: &str| {
        annotation
            .str(key)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let location = match annotation.str("type")? {
        "url_citation" => SourceLocation::Url { url: text("url")? },
        "file_citation" | "file_path" | "container_file_citation" => SourceLocation::File {
            file_id: text("file_id")?,
            filename: text("filename"),
            container_id: text("container_id"),
        },
        _ => return None,
    };
    let source = Source::new(location);
    Some(match text("title") {
        Some(title) => source.title(title),
        None => source,
    })
}

#[derive(Default)]
pub struct ResponsesDecoder {
    slots: Vec<Slot>,
    indexed: HashMap<usize, usize>,
    current: Option<usize>,
    held: Vec<usize>,
}

impl ResponsesDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn addressed(&self, frame: &Value, kind: Kind) -> Result<Option<usize>, ProviderError> {
        let index = output_index(frame)?;
        if let Some(slot) = index.and_then(|index| self.indexed.get(&index)) {
            return Ok(Some(*slot));
        }
        let id = frame
            .at("/item/id")
            .and_then(Value::as_str)
            .or_else(|| frame.str("item_id"));
        if let Some(slot) = id.filter(|id| !id.is_empty()).and_then(|id| {
            self.slots
                .iter()
                .rposition(|slot| slot.id.as_deref() == Some(id))
        }) {
            return Ok(Some(slot));
        }
        Ok(self.current.filter(|current| {
            (index.is_none() || !self.indexed.values().any(|slot| slot == current))
                && self
                    .slots
                    .get(*current)
                    .is_some_and(|slot| slot.open && slot.kind == kind)
        }))
    }

    fn added(
        &mut self,
        index: Option<usize>,
        item: &Value,
        out: &mut Out<'_, Completion>,
    ) -> Result<usize, ProviderError> {
        if let Some(previous) = index.and_then(|index| self.indexed.get(&index).copied()) {
            self.vacate(previous, out)?;
        }
        let at = match index {
            Some(index) if !self.slots.iter().any(|slot| slot.at == index) => index,
            _ => out.fresh_index(),
        };
        let kind = Kind::of(item);
        let custom = item.str("type") == Some("custom_tool_call");
        match kind {
            Kind::Message => out.open(at, Block::Text, Value::Null)?,
            Kind::Reasoning => out.open(at, Block::Reasoning { redacted: false }, Value::Null)?,
            Kind::Call => out.fragment(
                Some(at),
                CallFragment {
                    id: item.str("call_id"),
                    name: item.str("name"),
                    arguments: None,
                },
            )?,
            // An item that names no type is kept and never sent back.
            Kind::Opaque => {
                let replay = item
                    .str("type")
                    .is_some_and(|kind| !CLIENT_EXECUTED.contains(&kind));
                out.open(at, Block::Opaque { replay }, item.clone())?;
            }
        }
        self.slots.push(Slot {
            at,
            kind,
            id: item_id(item).map(str::to_owned),
            open: true,
            text: String::new(),
            field: None,
            part: 0,
            arguments: String::new(),
            custom,
            named: item.str("call_id").is_some_and(|id| !id.is_empty()),
            titled: item.str("name").is_some_and(|name| !name.is_empty()),
        });
        let slot = self.slots.len() - 1;
        if let Some(index) = index {
            self.indexed.insert(index, slot);
        }
        self.current = Some(slot);
        if let Some(call) = self
            .slots
            .get_mut(slot)
            .filter(|slot| slot.kind == Kind::Call)
        {
            let arguments = item.str(if custom { "input" } else { "arguments" });
            send(call, arguments.unwrap_or_default(), out)?;
        }
        Ok(slot)
    }

    fn vacate(&mut self, slot: usize, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let Some(open) = self.slots.get_mut(slot).filter(|slot| slot.open) else {
            return Ok(());
        };
        open.open = false;
        let at = open.at;
        if open.kind == Kind::Call {
            flush(open, out)?;
        }
        out.close(at)?;
        self.release(false, out)
    }

    fn release(
        &mut self,
        complete: bool,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        std::mem::take(&mut self.held)
            .into_iter()
            .try_for_each(|at| {
                if complete {
                    out.finish(at)
                } else {
                    out.close(at)
                }
            })
    }

    fn text(
        &mut self,
        frame: &Value,
        field: &'static str,
        part: &str,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        let delta = frame.str("delta").unwrap_or_default();
        if delta.is_empty() {
            return Ok(());
        }
        let part = frame.u64(part).unwrap_or(0);
        let kind = match field {
            "message" => Kind::Message,
            _ => Kind::Reasoning,
        };
        let known = self.addressed(frame, kind)?.and_then(|at| {
            let slot = self.slots.get(at)?;
            Some((at, slot.open, slot.kind == kind))
        });
        let slot = match known {
            Some((slot, true, true)) => slot,
            // A delta for an item that already closed is left to the item,
            // which states its text.
            Some((_, false, true)) => return Ok(()),
            _ => self.added(
                output_index(frame)?,
                &json!({"type": if kind == Kind::Message { "message" } else { "reasoning" }}),
                out,
            )?,
        };
        let Some(streamed) = self.slots.get_mut(slot).filter(|slot| slot.open) else {
            return Ok(());
        };
        if streamed.field.is_some_and(|known| known != field) {
            return Ok(());
        }
        if streamed.part != part && kind != Kind::Message && !streamed.text.is_empty() {
            streamed.text.push_str("\n\n");
            out.push(streamed.at, "\n\n")?;
        }
        streamed.field = Some(field);
        streamed.part = part;
        streamed.text.push_str(delta);
        out.push(streamed.at, delta)
    }

    fn arguments(
        &mut self,
        frame: &Value,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        let slot = self.addressed(frame, Kind::Call)?;
        if let Some(call) = slot
            .and_then(|slot| self.slots.get_mut(slot))
            .filter(|slot| slot.open && slot.kind == Kind::Call)
        {
            send(call, frame.str("delta").unwrap_or_default(), out)?;
        }
        Ok(())
    }

    fn done(
        &mut self,
        slot: usize,
        item: Value,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        let Some(done) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        done.open = false;
        if done.id.is_none() {
            done.id = item_id(&item).map(str::to_owned);
        }
        let at = done.at;
        let mut item = item;
        let mut nameless = false;
        match done.kind {
            Kind::Call => {
                nameless = !done.titled && item.str("name").is_none_or(str::is_empty);
                let arguments = arguments_of(&item).unwrap_or_else(|| streamed_arguments(done));
                let rest = remainder(done, &arguments, out)?;
                out.fragment(
                    Some(at),
                    CallFragment {
                        id: item.str("call_id").filter(|_| !done.named),
                        name: item.str("name"),
                        arguments: Some(&rest),
                    },
                )?;
            }
            Kind::Message | Kind::Reasoning => {
                let text = text_of(&item);
                if text.is_empty() {
                    // Raw reasoning text the done item leaves out stays out:
                    // written in as a summary, it would replay one the
                    // provider never produced.
                    if !done.text.is_empty() && done.field != Some("reasoning") {
                        item = stating(item, done.kind, &done.text);
                    }
                } else if let Some(rest) = text.strip_prefix(done.text.as_str()) {
                    out.push(at, rest)?;
                } else {
                    // The done item states the whole text.
                    out.restate(at, &text)?;
                }
                let citations = citations_of(&item);
                if done.kind == Kind::Message && !citations.is_empty() {
                    // The item restates every annotation, so its list
                    // replaces any an earlier snapshot gave.
                    out.set_citations(at, citations);
                }
            }
            Kind::Opaque => {}
        }
        let reasoning = done.kind == Kind::Reasoning;
        out.edit(at, |native| *native = item)?;
        if reasoning {
            // Reasoning is sent only with the item it precedes, so it is
            // complete only once that item is.
            self.held.push(at);
            return Ok(());
        }
        out.finish(at)?;
        // The writer drops a call with no name, so the reasoning it held
        // has no item to go with.
        self.release(!nameless, out)
    }

    fn item_event(
        &mut self,
        kind: &str,
        mut frame: Value,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        match kind {
            "response.output_item.added" | "response.output_item.done" => {
                let Some(item) = frame.get("item").filter(|item| item.is_object()) else {
                    return Ok(());
                };
                let index = output_index(&frame)?;
                if kind == "response.output_item.added" {
                    return self.added(index, item, out).map(|_| ());
                }
                let known = self.addressed(&frame, Kind::of(item))?;
                // A named output index identifies the item that
                // reads it, whatever id it states.
                let restates = |slot: &Slot| {
                    slot.kind == Kind::of(item)
                        && (index.is_some()
                            || item_id(item)
                                .zip(slot.id.as_deref())
                                .is_none_or(|(id, known)| id == known))
                };
                let known = known.and_then(|at| {
                    let slot = self.slots.get(at)?;
                    let same = item_id(item).is_some() && slot.id.as_deref() == item_id(item);
                    Some((at, slot.open && restates(slot), same))
                });
                let slot = match known {
                    Some((slot, true, _)) => slot,
                    // The item this one restates is already done.
                    Some((_, false, true)) => return Ok(()),
                    _ => self.added(index, item, out)?,
                };
                let item = frame
                    .as_object_mut()
                    .and_then(|frame| remove_preserving_order(frame, "item"))
                    .unwrap_or_default();
                self.done(slot, item, out)
            }
            // A refusal is the assistant's message for that turn.
            "response.output_text.delta" | "response.refusal.delta" => {
                self.text(&frame, "message", "content_index", out)
            }
            "response.reasoning_summary_text.delta" => {
                self.text(&frame, "summary", "summary_index", out)
            }
            "response.reasoning_text.delta" => self.text(&frame, "reasoning", "content_index", out),
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                self.arguments(&frame, out)
            }
            _ => Ok(()),
        }
    }

    fn restated(
        &self,
        index: usize,
        item: &Value,
        kinds: &[Kind],
        taken: &HashSet<usize>,
    ) -> Option<usize> {
        let id = item_id(item);
        let free = |at: &usize| {
            !taken.contains(at)
                && self
                    .slots
                    .get(*at)
                    .is_some_and(|slot| slot.kind == Kind::of(item))
        };
        let find = |fits: &dyn Fn(usize, &Slot) -> bool| {
            self.slots
                .iter()
                .enumerate()
                .find(|(at, slot)| free(at) && fits(*at, slot))
                .map(|(at, _)| at)
        };
        id.and_then(|id| find(&|_, slot| slot.id.as_deref() == Some(id)))
            .or_else(|| self.indexed.get(&index).copied().filter(free))
            .or_else(|| {
                find(&|at, slot| {
                    !self.indexed.values().any(|indexed| *indexed == at)
                        && (id.is_none() || slot.id.is_none())
                })
            })
            .or_else(|| {
                find(&|at, slot| {
                    (id.is_none() || slot.id.is_none())
                        && self.indexed.iter().any(|(streamed, indexed)| {
                            *indexed == at
                                && kinds.get(*streamed).is_none_or(|other| *other != slot.kind)
                        })
                })
            })
    }

    fn finish(
        &mut self,
        mut response: Value,
        mut out: Out<'_, Completion>,
    ) -> Result<Flow, ProviderError> {
        let output = match response.get_mut("output") {
            Some(Value::Array(output)) => std::mem::take(output),
            _ => Vec::new(),
        };
        if self.slots.is_empty()
            && !output.iter().any(|item| Kind::of(item) == Kind::Reasoning)
            && let Some(reasoning) = response.str("reasoning").filter(|text| !text.is_empty())
        {
            // Index 0 orders it before the output; it closes at once, so the
            // output's first item may open there after it.
            out.whole(
                0,
                Block::Reasoning { redacted: false },
                Value::Null,
                reasoning,
            )?;
        }
        let mut taken = HashSet::new();
        let kinds: Vec<Kind> = output.iter().map(Kind::of).collect();
        for (index, item) in output
            .into_iter()
            .enumerate()
            .filter(|(_, item)| item.is_object())
        {
            let known = self.restated(index, &item, &kinds, &taken).and_then(|at| {
                let slot = self.slots.get(at)?;
                Some((at, slot.open, slot.kind, slot.at))
            });
            let slot = match known {
                Some((slot, false, kind, at)) => {
                    taken.insert(slot);
                    let ciphertext = item
                        .get("encrypted_content")
                        .filter(|cipher| cipher.as_str().is_some_and(|cipher| !cipher.is_empty()));
                    if let (Kind::Reasoning, Some(ciphertext)) = (kind, ciphertext) {
                        out.edit(at, |native| {
                            let stated = native
                                .str("encrypted_content")
                                .is_some_and(|cipher| !cipher.is_empty());
                            if let (false, Some(fields)) = (stated, native.as_object_mut()) {
                                fields.insert("encrypted_content".to_owned(), ciphertext.clone());
                            }
                        })?;
                    }
                    continue;
                }
                Some((slot, ..)) => slot,
                None => self.added(
                    Some(index).filter(|index| !self.indexed.contains_key(index)),
                    &item,
                    &mut out,
                )?,
            };
            taken.insert(slot);
            self.done(slot, item, &mut out)?;
        }
        // Reasoning still waiting is complete when nothing after it is left
        // open.
        let open = self.slots.iter().any(|slot| slot.open);
        self.release(!open, &mut out)?;
        // A call never done shows what streamed, in a turn that fails.
        for slot in self.slots.iter_mut().filter(|slot| slot.open) {
            if slot.kind == Kind::Call {
                flush(slot, &mut out)?;
            }
        }
        let (reason, error) = finish_reason_of(&response);
        let end = Finish {
            usage: response.get("usage").map(usage_of).unwrap_or_default(),
            reason: Some(reason),
            response_id: item_id(&response).map(str::to_owned),
            model: response
                .str("model")
                .filter(|model| !model.is_empty())
                .map(str::to_owned),
            error,
        };
        Ok(out.end(end))
    }
}

fn flush(slot: &Slot, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
    let arguments = streamed_arguments(slot);
    let rest = remainder(slot, &arguments, out)?;
    let fragment = CallFragment {
        arguments: Some(&rest),
        ..CallFragment::default()
    };
    out.fragment(Some(slot.at), fragment)
}

fn send(slot: &mut Slot, delta: &str, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
    if delta.is_empty() {
        return Ok(());
    }
    let fragment = if slot.custom {
        let quoted = serde_json::to_string(delta)?;
        let open = quoted.strip_suffix('"').unwrap_or(&quoted);
        Cow::Owned(match open.strip_prefix('"') {
            Some(escaped) if !slot.arguments.is_empty() => escaped.to_owned(),
            _ => format!("{{\"input\":{open}"),
        })
    } else {
        Cow::Borrowed(delta)
    };
    slot.arguments.push_str(delta);
    let fragment = CallFragment {
        arguments: Some(&fragment),
        ..CallFragment::default()
    };
    out.fragment(Some(slot.at), fragment)
}

fn remainder(
    slot: &Slot,
    arguments: &str,
    out: &mut Out<'_, Completion>,
) -> Result<String, ProviderError> {
    // What `send` wrote: the streamed JSON, a custom call's input still open.
    let streamed = streamed_arguments(slot);
    let sent = if slot.arguments.is_empty() {
        ""
    } else if slot.custom {
        streamed.strip_suffix("\"}").unwrap_or(&streamed)
    } else {
        &streamed
    };
    Ok(match arguments.strip_prefix(sent) {
        Some(rest) => rest.to_owned(),
        None => {
            out.restate(slot.at, arguments)?;
            String::new()
        }
    })
}

fn streamed_arguments(slot: &Slot) -> String {
    if slot.custom {
        json!({ "input": slot.arguments }).to_string()
    } else {
        slot.arguments.clone()
    }
}

fn output_index(frame: &Value) -> Result<Option<usize>, ProviderError> {
    frame
        .u64("output_index")
        .map(|index| {
            usize::try_from(index).map_err(|_| {
                ProviderError::Response(format!("output_index {index} is out of range"))
            })
        })
        .transpose()
}

impl<'id> Decoder<'id, Completion> for ResponsesDecoder {
    type Event = ResponsesEvent;

    fn classify(&self, frame: WireFrame) -> WireEvent<ResponsesEvent> {
        classify_responses_payload(&frame.as_str())
    }

    fn decode(
        &mut self,
        event: ResponsesEvent,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        // A terminal may state items the stream never announced; they take
        // their place by output index.
        out.order_by_index();
        match event {
            ResponsesEvent::Frame {
                kind,
                mut frame,
                raw,
            } => match kind.as_str() {
                // `response.incomplete` is a genuine terminal that keeps the
                // partial output and usage. It is incomplete whatever its
                // `status` identifies the terminal state.
                "response.completed" | "response.incomplete" => {
                    let mut response = frame
                        .as_object_mut()
                        .and_then(|frame| remove_preserving_order(frame, "response"))
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({}));
                    if let (true, Some(fields)) =
                        (kind == "response.incomplete", response.as_object_mut())
                    {
                        fields.insert("status".to_owned(), json!("incomplete"));
                    }
                    self.finish(response, out)
                }
                "response.failed" => {
                    Err(ProviderError::from_provider_body(raw.unwrap_or_default()))
                }
                "keepalive" => Ok(Flow::More),
                kind if is_lifecycle_event(kind) => Ok(Flow::More),
                kind => {
                    self.item_event(kind, frame, &mut out)?;
                    Ok(Flow::More)
                }
            },
            // The unary reply is the terminal response with no stream before it.
            ResponsesEvent::Whole(body) => self.finish(body, out),
            ResponsesEvent::Failure(raw) => Err(ProviderError::from_provider_body(raw)),
            // Nothing to write: the provider's end is `response.completed`.
            ResponsesEvent::Sentinel => Ok(Flow::More),
        }
    }
}

pub(crate) mod document;
