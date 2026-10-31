use crate::error::{Reason, invalid};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use swarmie_ai::codecs::{completion::ToolDefinition, message as ai};
use swarmie_ai::{CompletionRequest, CompletionResponse};
use swarmie_providers::{
    AssistantMessage, Message, ModelRequest, ModelResponse, ToolCall, ToolCallId, ToolName,
    ToolResult, Usage,
};

fn portable_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn readable_alias(name: &str, collision: usize) -> String {
    let mut stem = String::new();
    let mut lossy = false;
    for character in name.chars() {
        match character {
            '.' => stem.push_str("__"),
            c if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') => stem.push(c),
            _ => {
                stem.push('_');
                lossy = true;
            }
        }
    }
    if stem.len() > 64 || lossy || collision != 0 {
        // This digest identifies names; it is not a security boundary. The
        // occupied-name set also checks digest and sanitization collisions.
        let digest = name.bytes().fold(0xcbf29ce484222325_u64, |digest, byte| {
            (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        let suffix = if collision == 0 {
            format!("_{digest:016x}")
        } else {
            format!("_{digest:016x}_{collision}")
        };
        stem.truncate(64 - suffix.len());
        stem.push_str(&suffix);
    }
    stem
}

/// Request-local names are encoded only at the provider boundary. Include old
/// calls so that a removed tool still has a coherent historical representation.
struct ToolNames {
    outbound: BTreeMap<ToolName, String>,
    inbound: BTreeMap<String, ToolName>,
}
impl ToolNames {
    fn new(input: &ModelRequest) -> tembra::Result<Self> {
        let canonical: BTreeSet<_> = input
            .tools
            .iter()
            .map(|tool| &tool.name)
            .chain(
                input
                    .context
                    .messages
                    .iter()
                    .filter_map(|message| match message {
                        Message::Assistant(message) => Some(&message.tool_calls),
                        _ => None,
                    })
                    .flatten()
                    .map(|call| &call.name),
            )
            .collect();
        let mut occupied: BTreeSet<String> = canonical
            .iter()
            .filter(|name| portable_name(name.as_str()))
            .map(|name| name.as_str().to_owned())
            .collect();
        let mut outbound = BTreeMap::new();
        let mut inbound = BTreeMap::new();
        for name in canonical {
            let encoded = if portable_name(name.as_str()) {
                name.as_str().to_owned()
            } else {
                let mut collision = 0usize;
                loop {
                    let candidate = readable_alias(name.as_str(), collision);
                    if occupied.insert(candidate.clone()) {
                        break candidate;
                    }
                    collision = collision
                        .checked_add(1)
                        .ok_or_else(|| invalid(Reason::Request, "too many colliding tool names"))?;
                }
            };
            outbound.insert(name.clone(), encoded.clone());
            if inbound.insert(encoded, name.clone()).is_some() {
                return Err(invalid(Reason::Request, "provider tool name collision"));
            }
        }
        Ok(Self { outbound, inbound })
    }
    fn encode(&self, name: &ToolName) -> tembra::Result<&str> {
        self.outbound
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| invalid(Reason::Request, "tool name was not captured"))
    }
}

/// Convert once into AI's canonical typed messages. Native assistant records stay
/// attached to canonical history, including signatures, reasoning and raw usage.
pub(crate) fn request(input: &ModelRequest) -> tembra::Result<CompletionRequest> {
    let mapping = ToolNames::new(input)?;
    let mut names = BTreeSet::new();
    let mut tools = Vec::new();
    for tool in &input.tools {
        if !names.insert(&tool.name)
            || tool.input_schema.get("type").and_then(|v| v.as_str()) != Some("object")
        {
            return Err(invalid(
                Reason::Request,
                "tools require distinct names and object parameter schemas",
            ));
        }
        tools.push(ToolDefinition {
            name: ai::ToolName::new(mapping.encode(&tool.name)?)
                .map_err(|_| invalid(Reason::Request, "invalid tool name"))?,
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
            annotations: Default::default(),
        });
    }
    let mut messages = Vec::new();
    if !input.context.instructions.is_empty() {
        messages.push(ai::Message::system(&input.context.instructions));
    }
    let mut pending = VecDeque::new();
    let mut ids = BTreeSet::new();
    let model_messages = input.context.model_messages()?;
    for message in model_messages.iter() {
        match message {
            Message::User { content } => {
                if !pending.is_empty() {
                    return Err(invalid(
                        Reason::Request,
                        "user message precedes pending tool results",
                    ));
                }
                messages.push(ai::Message::user(content));
            }
            Message::Assistant(assistant) => {
                if !pending.is_empty() {
                    return Err(invalid(
                        Reason::Request,
                        "assistant precedes pending tool results",
                    ));
                }
                let mut parts = Vec::new();
                if !assistant.content.is_empty() {
                    parts.push(ai::AssistantContent::text(&assistant.content));
                }
                for call in &assistant.tool_calls {
                    if !call.arguments.is_object() || !ids.insert(&call.id) {
                        return Err(invalid(
                            Reason::Request,
                            "invalid or repeated historical tool call",
                        ));
                    }
                    parts.push(ai::AssistantContent::tool_call(
                        call.id.as_str(),
                        ai::ToolName::new(mapping.encode(&call.name)?)
                            .map_err(|_| invalid(Reason::Request, "invalid tool name"))?,
                        call.arguments.clone(),
                    ));
                }
                let turn = if let Some(record) = &assistant.provider {
                    if record.get("unavailable").is_some() {
                        return Err(invalid(
                            Reason::Request,
                            "retained provider continuation is unavailable",
                        ));
                    }
                    let response: CompletionResponse = serde_json::from_value(record.clone())
                        .map_err(|_| {
                            invalid(Reason::Request, "invalid retained provider record")
                        })?;
                    let turn = response.head().with_content(response.choice.clone());
                    let actual = turn.tool_calls().collect::<Vec<_>>();
                    if turn.text() != assistant.content
                        || actual.len() != assistant.tool_calls.len()
                    {
                        return Err(invalid(
                            Reason::Request,
                            "provider record differs from canonical history",
                        ));
                    }
                    for (native, call) in actual.into_iter().zip(&assistant.tool_calls) {
                        if native.id.wire() != call.id.as_str()
                            || native.function.name.as_str() != mapping.encode(&call.name)?
                            || Some(&native.function.arguments) != call.arguments.as_object()
                        {
                            return Err(invalid(
                                Reason::Request,
                                "provider call differs from canonical history",
                            ));
                        }
                    }
                    turn
                } else {
                    ai::AssistantMessage::new(parts)
                };
                for (native, call) in turn.tool_calls().zip(&assistant.tool_calls) {
                    pending.push_back((&call.id, &call.name, native.id.clone()));
                }
                messages.push(ai::Message::Assistant(turn));
            }
            Message::Tool { call, result } => {
                let Some((expected, name, native_id)) = pending.pop_front() else {
                    return Err(invalid(
                        Reason::Request,
                        "tool result has no preceding call",
                    ));
                };
                if expected != call {
                    return Err(invalid(
                        Reason::Request,
                        "tool result does not match pending call",
                    ));
                }
                let value = match result {
                    ToolResult::Success(value) => value.clone(),
                    ToolResult::Failed(message) => {
                        json!({"is_error":true,"blocked":false,"message":message})
                    }
                    ToolResult::Blocked(message) => {
                        json!({"is_error":true,"blocked":true,"message":message})
                    }
                };
                messages.push(ai::Message::tool_result(
                    native_id,
                    ai::ToolName::new(mapping.encode(name)?)
                        .map_err(|_| invalid(Reason::Request, "invalid tool name"))?,
                    value.to_string(),
                ));
            }
        }
    }
    if !pending.is_empty() || input.context.messages.is_empty() {
        return Err(invalid(
            Reason::Request,
            "model request has empty or incomplete message history",
        ));
    }
    Ok(CompletionRequest::from(messages).tools(tools))
}

pub(crate) fn response(
    output: &CompletionResponse,
    input: &ModelRequest,
) -> tembra::Result<ModelResponse> {
    let mapping = ToolNames::new(input)?;
    if !matches!(
        output.stop(),
        ai::StopReason::Stop | ai::StopReason::ToolUse
    ) {
        return Err(invalid(
            Reason::Response,
            "model did not finish a complete answer or tool request",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut calls = Vec::new();
    for call in output.tool_calls() {
        let id = ToolCallId::new(call.id.wire().into_owned())?;
        let name = mapping
            .inbound
            .get(call.function.name.as_str())
            .filter(|name| input.tools.iter().any(|tool| &tool.name == *name))
            .cloned()
            .ok_or_else(|| {
                invalid(
                    Reason::Response,
                    "model returned an unadmitted provider tool name",
                )
            })?;
        if !ids.insert(id.clone()) || call.function.invalid_arguments.is_some() {
            return Err(invalid(
                Reason::Response,
                "model returned invalid tool calls",
            ));
        }
        calls.push(ToolCall {
            id,
            name,
            arguments: serde_json::Value::Object(call.function.arguments.clone()),
        });
    }
    let usage = &output.usage;
    Ok(ModelResponse {
        protection: swarmie_providers::SubmissionProtection::Unprotected,
        message: AssistantMessage {
            content: output.text(),
            tool_calls: calls,
            provider: Some(
                serde_json::to_value(output)
                    .map_err(|_| invalid(Reason::Response, "provider record cannot be retained"))?,
            ),
        },
        usage: Usage {
            input_tokens: usage
                .input_tokens
                .ok_or_else(|| invalid(Reason::Response, "provider input usage is unknown"))?,
            output_tokens: usage
                .output_tokens
                .ok_or_else(|| invalid(Reason::Response, "provider output usage is unknown"))?,
        },
    })
}
