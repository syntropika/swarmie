//! Provider-native continuation blocks are deliberately private to this adapter. They keep
//! their wire order and opaque values; the neutral contract sees versioned JSON extensions.
use super::{Checked, bytes, invalid, namespace, parse_error, unsupported};
use crate::{Connection, error::failure};
use genai::{adapter::AdapterKind, chat as g};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use swarmie_model_contracts::{Extension, ProviderErrorKind, Submission};

fn loss() -> swarmie_model_contracts::ProviderError {
    failure(ProviderErrorKind::DataLoss, Submission::ResponseReceived)
}

pub(super) fn replay_extension(c: &Connection, kind: &str, value: Value) -> Extension {
    Extension {
        namespace: namespace(c),
        version: 2,
        value: json!({"kind":kind,"value":value}),
    }
}

pub(super) fn replay_text(kind: AdapterKind, ext: &Extension) -> Checked<Option<String>> {
    match ext.value["kind"].as_str() {
        Some("reasoning_replay") => Ok(Some(
            ext.value["value"].as_str().ok_or_else(invalid)?.into(),
        )),
        Some("provider_content") => Ok(provider_reasoning(kind, &ext.value["value"])),
        _ => Err(unsupported()),
    }
}

/// Only provider-documented continuation block shapes can pass through. Unknown content
/// cannot silently become conversational text or disappear on the next turn.
pub(super) fn validate_provider_content(kind: AdapterKind, value: &Value) -> Checked<()> {
    let valid = match kind {
        AdapterKind::Anthropic => match value["type"].as_str() {
            Some("thinking") => value["thinking"].is_string() && value["signature"].is_string(),
            Some("redacted_thinking") => value["data"].is_string(),
            _ => false,
        },
        AdapterKind::OpenAIResp => {
            value["type"] == "reasoning"
                && value["id"].is_string()
                && value["summary"].as_array().is_some_and(|items| {
                    items
                        .iter()
                        .all(|p| p["type"] == "summary_text" && p["text"].is_string())
                })
                && value
                    .get("encrypted_content")
                    .is_none_or(|v| v.is_string() || v.is_null())
        }
        AdapterKind::Gemini => {
            value["thought"] == true
                && value["text"].is_string()
                && value.get("thoughtSignature").is_none_or(Value::is_string)
                && value.as_object().is_some_and(|o| {
                    o.keys()
                        .all(|k| matches!(k.as_str(), "thought" | "text" | "thoughtSignature"))
                })
        }
        _ => false,
    };
    if valid { Ok(()) } else { Err(loss()) }
}

pub(super) fn provider_reasoning(kind: AdapterKind, value: &Value) -> Option<String> {
    match kind {
        AdapterKind::Anthropic if value["type"] == "thinking" => {
            value["thinking"].as_str().map(str::to_owned)
        }
        AdapterKind::OpenAIResp if value["type"] == "reasoning" => value["summary"]
            .as_array()
            .map(|items| items.iter().filter_map(|p| p["text"].as_str()).collect()),
        AdapterKind::Gemini if value["thought"] == true => {
            value["text"].as_str().map(str::to_owned)
        }
        _ => None,
    }
}

pub(super) fn prepare_replay(
    kind: AdapterKind,
    model: &str,
    parts: Vec<g::ContentPart>,
) -> Checked<Vec<g::ContentPart>> {
    if kind == AdapterKind::Anthropic {
        let mut result = Vec::new();
        let mut parts = parts.into_iter().peekable();
        while let Some(part) = parts.next() {
            match part {
                g::ContentPart::ThoughtSignature(signature) => {
                    let Some(g::ContentPart::ReasoningContent(thinking)) = parts.next() else {
                        return Err(invalid());
                    };
                    result.push(g::ContentPart::from_custom(
                        json!({"type":"thinking","thinking":thinking,"signature":signature}),
                        None,
                    ));
                }
                g::ContentPart::ReasoningContent(thinking) => {
                    let Some(g::ContentPart::ThoughtSignature(signature)) = parts.next() else {
                        return Err(invalid());
                    };
                    result.push(g::ContentPart::from_custom(
                        json!({"type":"thinking","thinking":thinking,"signature":signature}),
                        None,
                    ));
                }
                g::ContentPart::ToolCall(mut call) => {
                    // Old genai mirrored the complete turn's signatures on the first call.
                    // Positional native blocks are authoritative; inconsistent mirrors fail.
                    if let Some(signatures) = call.thought_signatures.take() {
                        let expected: Vec<_> = result
                            .iter()
                            .filter_map(|p| match p {
                                g::ContentPart::Custom(custom) => {
                                    custom.data["signature"].as_str().map(str::to_owned)
                                }
                                _ => None,
                            })
                            .collect();
                        if signatures != expected {
                            return Err(invalid());
                        }
                    }
                    result.push(g::ContentPart::ToolCall(call));
                }
                g::ContentPart::Custom(custom) => {
                    validate_provider_content(kind, &custom.data).map_err(|_| unsupported())?;
                    result.push(g::ContentPart::Custom(custom));
                }
                other => result.push(other),
            }
        }
        return Ok(result);
    }
    if !matches!(kind, AdapterKind::OpenAIResp | AdapterKind::Gemini) {
        let mut non_reasoning = false;
        let mut reasoning_count = 0;
        let mut tool_seen = false;
        for part in &parts {
            if matches!(part, g::ContentPart::Text(_)) && tool_seen {
                return Err(unsupported());
            }
            tool_seen |= matches!(part, g::ContentPart::ToolCall(_));
            if matches!(part, g::ContentPart::ReasoningContent(_)) {
                reasoning_count += 1;
                if non_reasoning || reasoning_count > 1 {
                    return Err(unsupported());
                }
            } else {
                non_reasoning = true;
            }
        }
    }
    if kind == AdapterKind::Gemini
        && model.contains("gemini-3")
        && parts
            .iter()
            .find_map(|p| match p {
                g::ContentPart::ToolCall(call) => Some(call),
                _ => None,
            })
            .is_some_and(|call| call.thought_signatures.is_none())
    {
        return Err(unsupported());
    }
    for part in &parts {
        match part {
            g::ContentPart::ThoughtSignature(_) => return Err(unsupported()),
            g::ContentPart::ToolCall(call)
                if kind == AdapterKind::Gemini && synthetic_gemini_id(&call.call_id) =>
            {
                return Err(unsupported());
            }
            // Responses needs the complete reasoning item, including its ID and summary.
            // Gemini needs the signed native thought block, not a flattened string.
            g::ContentPart::ReasoningContent(_)
                if matches!(kind, AdapterKind::OpenAIResp | AdapterKind::Gemini) =>
            {
                return Err(unsupported());
            }
            g::ContentPart::Custom(custom) => {
                validate_provider_content(kind, &custom.data).map_err(|_| unsupported())?
            }
            g::ContentPart::ToolCall(call)
                if kind != AdapterKind::Gemini && call.thought_signatures.is_some() =>
            {
                return Err(unsupported());
            }
            g::ContentPart::ToolCall(call)
                if kind == AdapterKind::Gemini
                    && call
                        .thought_signatures
                        .as_ref()
                        .is_some_and(|s| s.len() != 1) =>
            {
                return Err(unsupported());
            }
            _ => {}
        }
    }
    Ok(parts)
}

fn synthetic_gemini_id(id: &str) -> bool {
    id.strip_prefix("call#")
        .and_then(|s| s.rsplit_once('#'))
        .is_some_and(|(name, index)| !name.is_empty() && index.parse::<usize>().is_ok())
}

fn string(value: &Value, key: &str) -> Checked<String> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(parse_error)
}

fn native_parts(kind: AdapterKind, blocks: &[Value]) -> Checked<g::MessageContent> {
    let mut parts = Vec::new();
    for block in blocks {
        match kind {
            AdapterKind::Anthropic => match block["type"].as_str() {
                Some("text") => parts.push(g::ContentPart::Text(string(block, "text")?)),
                Some("thinking" | "redacted_thinking") => {
                    validate_provider_content(kind, block)?;
                    parts.push(g::ContentPart::from_custom(block.clone(), None));
                }
                Some("tool_use") => parts.push(g::ContentPart::ToolCall(g::ToolCall {
                    call_id: string(block, "id")?,
                    fn_name: string(block, "name")?,
                    fn_arguments: block["input"].clone(),
                    thought_signatures: None,
                })),
                _ => return Err(loss()),
            },
            AdapterKind::OpenAIResp => match block["type"].as_str() {
                Some("message") => {
                    for part in block["content"].as_array().ok_or_else(parse_error)? {
                        if part["type"] != "output_text" {
                            return Err(loss());
                        }
                        parts.push(g::ContentPart::Text(string(part, "text")?));
                    }
                }
                Some("reasoning") => {
                    validate_provider_content(kind, block)?;
                    parts.push(g::ContentPart::from_custom(block.clone(), None));
                }
                Some("function_call") => parts.push(g::ContentPart::ToolCall(g::ToolCall {
                    call_id: string(block, "call_id")?,
                    fn_name: string(block, "name")?,
                    fn_arguments: serde_json::from_str(&string(block, "arguments")?)
                        .map_err(|_| parse_error())?,
                    thought_signatures: None,
                })),
                _ => return Err(loss()),
            },
            AdapterKind::Gemini => {
                if block["thought"] == true {
                    validate_provider_content(kind, block)?;
                    parts.push(g::ContentPart::from_custom(block.clone(), None));
                } else if let Some(text) = block.get("text") {
                    if block.get("thoughtSignature").is_some() {
                        return Err(loss());
                    }
                    parts.push(g::ContentPart::Text(
                        text.as_str().ok_or_else(parse_error)?.into(),
                    ));
                } else if let Some(call) = block.get("functionCall") {
                    parts.push(g::ContentPart::ToolCall(g::ToolCall {
                        // genai synthesizes IDs for older Gemini responses, which cannot be
                        // distinguished from original IDs later. Require the actual wire ID.
                        call_id: {
                            let id = string(call, "id")?;
                            if synthetic_gemini_id(&id) {
                                return Err(loss());
                            }
                            id
                        },
                        fn_name: string(call, "name")?,
                        fn_arguments: call["args"].clone(),
                        thought_signatures: block
                            .get("thoughtSignature")
                            .map(|s| {
                                s.as_str()
                                    .map(|s| vec![s.to_owned()])
                                    .ok_or_else(parse_error)
                            })
                            .transpose()?,
                    }));
                } else {
                    return Err(loss());
                }
            }
            _ => return Err(loss()),
        }
    }
    Ok(g::MessageContent::from_parts(parts))
}

pub(super) fn provider_content(
    kind: AdapterKind,
    raw: &Value,
) -> Checked<Option<g::MessageContent>> {
    let blocks = match kind {
        AdapterKind::Anthropic => raw.get("content"),
        AdapterKind::OpenAIResp => raw.get("output"),
        AdapterKind::Gemini => raw.pointer("/candidates/0/content/parts"),
        _ => return Ok(None),
    }
    .and_then(Value::as_array)
    .ok_or_else(parse_error)?;
    native_parts(kind, blocks).map(Some)
}

/// Rebuilds only provider content from already parsed genai frames. This is bounded by the
/// configured response budget and does not fix genai's pre-parse HTTP/SSE buffering.
pub(crate) struct StreamCapture {
    kind: AdapterKind,
    limit: usize,
    observed: usize,
    blocks: BTreeMap<usize, Value>,
    arguments: BTreeMap<usize, String>,
    failed: Option<swarmie_model_contracts::ProviderError>,
    seen: bool,
    terminal: bool,
    active_anthropic: Option<usize>,
    native_reasoning: bool,
}

impl StreamCapture {
    pub(crate) fn new(kind: AdapterKind, limit: usize) -> Self {
        Self {
            kind,
            limit,
            observed: 0,
            blocks: BTreeMap::new(),
            arguments: BTreeMap::new(),
            failed: None,
            seen: false,
            terminal: false,
            active_anthropic: None,
            native_reasoning: false,
        }
    }

    pub(crate) fn observe(&mut self, frame: &Value) {
        if self.failed.is_some() {
            return;
        }
        self.observed = self.observed.saturating_add(bytes(frame));
        if self.observed > self.limit {
            self.failed = Some(failure(
                ProviderErrorKind::PayloadLimit,
                Submission::ResponseReceived,
            ));
            return;
        }
        if let Err(error) = self.capture(frame) {
            self.failed = Some(error);
        }
    }

    fn capture(&mut self, frame: &Value) -> Checked<()> {
        if let Some(delta) = frame.pointer("/choices/0/delta") {
            let channels = ["content", "reasoning_content", "reasoning"]
                .iter()
                .filter(|key| {
                    delta
                        .get(**key)
                        .is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                })
                .count()
                + usize::from(
                    delta["tool_calls"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty()),
                );
            if channels > 1 || delta["content"].as_array().is_some_and(|a| !a.is_empty()) {
                return Err(loss());
            }
        }
        match self.kind {
            AdapterKind::Anthropic => self.capture_anthropic(frame)?,
            AdapterKind::OpenAIResp => self.capture_responses(frame)?,
            AdapterKind::Gemini => {
                if let Some(parts) = frame
                    .pointer("/candidates/0/content/parts")
                    .and_then(Value::as_array)
                {
                    // genai groups reasoning/text/tools within one frame. Refuse frames whose
                    // transport event order would differ, or whose multiple thought blocks
                    // overwrite each other. The final response never excuses lost events.
                    let mut last = 0;
                    let mut thoughts = 0;
                    for (index, part) in parts.iter().enumerate() {
                        if index > 0 && part.get("thoughtSignature").is_some() {
                            return Err(loss());
                        }
                        let rank = if part["thought"] == true {
                            thoughts += 1;
                            0
                        } else if part.get("text").is_some() {
                            1
                        } else {
                            2
                        };
                        if rank < last || thoughts > 1 {
                            return Err(loss());
                        }
                        last = rank;
                    }
                    self.seen = true;
                    for part in parts {
                        self.blocks.insert(self.blocks.len(), part.clone());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn capture_anthropic(&mut self, frame: &Value) -> Checked<()> {
        if self.terminal {
            return Err(loss());
        }
        match frame["type"].as_str().ok_or_else(parse_error)? {
            "content_block_start" => {
                let index = frame["index"].as_u64().ok_or_else(parse_error)? as usize;
                if self.active_anthropic.is_some() || index != self.blocks.len() {
                    return Err(loss());
                }
                let block = &frame["content_block"];
                match block["type"].as_str() {
                    Some("text") if block["text"].is_string() => {}
                    Some("thinking") if block["thinking"].is_string() => {}
                    Some("redacted_thinking") if block["data"].is_string() => {}
                    Some("tool_use")
                        if block["id"].is_string()
                            && block["name"].is_string()
                            && block["input"].is_object() => {}
                    _ => return Err(loss()),
                }
                self.seen = true;
                self.active_anthropic = Some(index);
                self.blocks.insert(index, block.clone());
            }
            "content_block_delta" => {
                let index = frame["index"].as_u64().ok_or_else(parse_error)? as usize;
                if self.active_anthropic != Some(index) {
                    return Err(loss());
                }
                let block = self.blocks.get_mut(&index).ok_or_else(loss)?;
                let delta = &frame["delta"];
                let key = match (block["type"].as_str(), delta["type"].as_str()) {
                    (Some("text"), Some("text_delta")) => "text",
                    (Some("thinking"), Some("thinking_delta")) => "thinking",
                    (Some("thinking"), Some("signature_delta")) => "signature",
                    (Some("tool_use"), Some("input_json_delta")) => {
                        self.arguments
                            .entry(index)
                            .or_default()
                            .push_str(delta["partial_json"].as_str().ok_or_else(parse_error)?);
                        return Ok(());
                    }
                    _ => return Err(loss()),
                };
                let existing = block[key].as_str().unwrap_or_default();
                block[key] =
                    format!("{existing}{}", delta[key].as_str().ok_or_else(parse_error)?).into();
            }
            "content_block_stop" => {
                let index = frame["index"].as_u64().ok_or_else(parse_error)? as usize;
                if self.active_anthropic != Some(index) {
                    return Err(loss());
                }
                if let Some(arguments) = self.arguments.remove(&index) {
                    self.blocks.get_mut(&index).ok_or_else(loss)?["input"] =
                        serde_json::from_str(&arguments).map_err(|_| parse_error())?;
                }
                self.active_anthropic = None;
            }
            "message_stop" => {
                if self.active_anthropic.is_some() || !self.arguments.is_empty() {
                    return Err(parse_error());
                }
                self.terminal = true;
            }
            "message_start" if frame["message"].is_object() => {}
            "message_delta" if frame["delta"].is_object() => {}
            "ping" | "error" => {}
            _ => return Err(loss()),
        }
        Ok(())
    }

    fn capture_responses(&mut self, frame: &Value) -> Checked<()> {
        if self.terminal {
            return Err(loss());
        }
        // genai skips JSON events that fail its internal enum decoding. Validate known
        // content-bearing event shapes here so a later terminal frame cannot hide a loss.
        let kind = frame["type"].as_str().ok_or_else(parse_error)?;
        for key in ["output_index", "content_index", "summary_index"] {
            if frame.get(key).is_some_and(|value| value.as_u64().is_none()) {
                return Err(parse_error());
            }
        }
        match kind {
            "response.output_text.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.function_call_arguments.delta" => {
                if !frame["delta"].is_string() {
                    return Err(parse_error());
                }
                if matches!(
                    kind,
                    "response.reasoning_text.delta" | "response.reasoning_summary_text.delta"
                ) {
                    self.native_reasoning = true;
                }
            }
            "response.output_text.done"
            | "response.reasoning_text.done"
            | "response.reasoning_summary_text.done" => {
                if !frame["text"].is_string() {
                    return Err(parse_error());
                }
            }
            "response.function_call_arguments.done" => {
                if !frame["arguments"].is_string() {
                    return Err(parse_error());
                }
            }
            "response.output_item.added" | "response.output_item.done" => {
                let index = frame["output_index"].as_u64().ok_or_else(parse_error)? as usize;
                let item = &frame["item"];
                if !matches!(
                    item["type"].as_str(),
                    Some("message" | "reasoning" | "function_call")
                ) {
                    return Err(loss());
                }
                if kind.ends_with(".done") {
                    self.seen = true;
                    if self.blocks.insert(index, item.clone()).is_some() {
                        return Err(loss());
                    }
                }
            }
            "response.content_part.added" | "response.content_part.done" => {
                if frame["part"]["type"] != "output_text" || !frame["part"]["text"].is_string() {
                    return Err(loss());
                }
            }
            "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done" => {
                if frame["part"]["type"] != "summary_text" || !frame["part"]["text"].is_string() {
                    return Err(loss());
                }
            }
            "response.completed" | "response.incomplete" => {
                let response = &frame["response"];
                if !response["id"].is_string()
                    || !response["model"].is_string()
                    || response["status"] != kind.strip_prefix("response.").unwrap()
                    || response
                        .get("output")
                        .is_some_and(|output| !output.is_array())
                {
                    return Err(parse_error());
                }
                if let Some(blocks) = response["output"]
                    .as_array()
                    .filter(|blocks| !blocks.is_empty())
                {
                    if self
                        .blocks
                        .iter()
                        .any(|(index, item)| blocks.get(*index) != Some(item))
                    {
                        return Err(loss());
                    }
                    self.seen = true;
                    self.blocks = blocks.iter().cloned().enumerate().collect();
                }
                self.terminal = true;
            }
            "response.created" | "response.in_progress" | "response.queued" | "response.failed" => {
                if !frame["response"].is_object() {
                    return Err(parse_error());
                }
            }
            "error" | "ping" => {}
            unknown
                if unknown.ends_with(".delta")
                    || unknown.split('.').any(|part| {
                        matches!(
                            part,
                            "output_text"
                                | "content_part"
                                | "reasoning"
                                | "reasoning_text"
                                | "reasoning_summary_text"
                                | "reasoning_summary_part"
                                | "audio"
                                | "refusal"
                                | "function_call_arguments"
                                | "custom_tool_call_input"
                        )
                    })
                    || [
                        "delta",
                        "item",
                        "part",
                        "content",
                        "text",
                        "arguments",
                        "encrypted_content",
                    ]
                    .iter()
                    .any(|key| frame.get(*key).is_some()) =>
            {
                return Err(loss());
            }
            // Unknown metadata does not enter model content. New content-bearing event
            // families must be qualified explicitly instead of disappearing through genai.
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn check(&self) -> Checked<()> {
        match &self.failed {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub(crate) fn finish(&self) -> Checked<Option<g::MessageContent>> {
        self.check()?;
        if matches!(self.kind, AdapterKind::Anthropic | AdapterKind::OpenAIResp) && !self.terminal {
            return Err(parse_error());
        }
        if self.native_reasoning
            && !self
                .blocks
                .values()
                .any(|block| block["type"] == "reasoning")
        {
            return Err(loss());
        }
        if !self.seen {
            return Ok(None);
        }
        if !self.arguments.is_empty() {
            return Err(parse_error());
        }
        native_parts(
            self.kind,
            &self.blocks.values().cloned().collect::<Vec<_>>(),
        )
        .map(Some)
    }
}
