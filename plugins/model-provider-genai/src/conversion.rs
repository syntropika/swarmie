use crate::error::{Reason, invalid};
use genai::chat::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, MessageContent, StopReason, Tool,
    ToolResponse,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
    fn encode(&self, name: &ToolName) -> &str {
        self.outbound
            .get(name)
            .expect("all canonical names are collected")
    }
}

/// Validate portable tool pairing before any network work. Function names are
/// recovered from their preceding calls, including on Gemini result messages.
pub(crate) fn request(input: &ModelRequest) -> tembra::Result<ChatRequest> {
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
        tools.push(
            Tool::new(mapping.encode(&tool.name))
                .with_description(&tool.description)
                .with_schema(tool.input_schema.clone()),
        );
    }
    let mut messages = Vec::new();
    let mut pending = VecDeque::new();
    let mut ids = BTreeSet::new();
    for message in &input.context.messages {
        match message {
            Message::User { content } => {
                if !pending.is_empty() {
                    return Err(invalid(
                        Reason::Request,
                        "user message precedes pending tool results",
                    ));
                }
                messages.push(ChatMessage::user(content.clone()));
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
                    parts.push(ContentPart::Text(assistant.content.clone()));
                }
                for call in &assistant.tool_calls {
                    if !call.arguments.is_object() || !ids.insert(&call.id) {
                        return Err(invalid(
                            Reason::Request,
                            "invalid or repeated historical tool call",
                        ));
                    }
                    pending.push_back((&call.id, &call.name));
                    parts.push(ContentPart::ToolCall(genai::chat::ToolCall {
                        call_id: call.id.as_str().into(),
                        fn_name: mapping.encode(&call.name).into(),
                        fn_arguments: call.arguments.clone(),
                        thought_signatures: None,
                    }));
                }
                messages.push(ChatMessage::assistant(MessageContent::from_parts(parts)));
            }
            Message::Tool { call, result } => {
                let Some((expected, name)) = pending.pop_front() else {
                    return Err(invalid(
                        Reason::Request,
                        "tool result has no preceding call",
                    ));
                };
                if expected != call {
                    return Err(invalid(
                        Reason::Request,
                        "tool result does not match its pending call",
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
                let response = ToolResponse::new(call.as_str(), value.to_string())
                    .with_fn_name(mapping.encode(name));
                messages.push(ChatMessage::tool(MessageContent::from_tool_responses(
                    vec![response],
                )));
            }
        }
    }
    if !pending.is_empty() || messages.is_empty() {
        return Err(invalid(
            Reason::Request,
            "model request has empty or incomplete message history",
        ));
    }
    let mut request = ChatRequest::new(messages);
    if !input.context.instructions.is_empty() {
        request = request.with_system(input.context.instructions.clone());
    }
    if !tools.is_empty() {
        request = request.with_tools(tools);
    }
    Ok(request)
}

/// The initial neutral profile is text plus JSON tool calls. Reject content that
/// would require signed/opaque replay rather than silently discarding it.
pub(crate) fn response(
    output: ChatResponse,
    input: &ModelRequest,
) -> tembra::Result<ModelResponse> {
    let mapping = ToolNames::new(input)?;
    if output
        .reasoning_content
        .as_ref()
        .is_some_and(|v| !v.is_empty())
    {
        return Err(invalid(
            Reason::Response,
            "reasoning continuation is outside the current text/tool profile",
        ));
    }
    if output.stop_reason.as_ref().is_some_and(|r| {
        !matches!(
            r,
            StopReason::Completed(_) | StopReason::ToolCall(_) | StopReason::StopSequence(_)
        )
    }) {
        return Err(invalid(
            Reason::Response,
            "model did not finish a complete answer or tool request",
        ));
    }
    let mut content = String::new();
    let mut calls = Vec::new();
    let mut ids = BTreeSet::new();
    for part in output.content {
        match part {
            ContentPart::Text(text) => content.push_str(&text),
            ContentPart::ToolCall(call) => {
                let id = ToolCallId::new(call.call_id)?;
                let name = mapping
                    .inbound
                    .get(&call.fn_name)
                    .filter(|name| input.tools.iter().any(|tool| &tool.name == *name))
                    .cloned()
                    .ok_or_else(|| {
                        invalid(
                            Reason::Response,
                            "model returned an unadmitted provider tool name",
                        )
                    })?;
                if !ids.insert(id.clone())
                    || !call.fn_arguments.is_object()
                    || !input.tools.iter().any(|t| t.name == name)
                    || call
                        .thought_signatures
                        .as_ref()
                        .is_some_and(|v| !v.is_empty())
                {
                    return Err(invalid(
                        Reason::Response,
                        "model returned invalid, unknown or signed tool calls",
                    ));
                }
                calls.push(ToolCall {
                    id,
                    name,
                    arguments: call.fn_arguments,
                });
            }
            _ => {
                return Err(invalid(
                    Reason::Response,
                    "model returned content outside the text/tool profile",
                ));
            }
        }
    }
    let tokens = |value: Option<i32>| {
        u64::try_from(value.unwrap_or(0))
            .map_err(|_| invalid(Reason::Response, "provider reported negative usage"))
    };
    Ok(ModelResponse {
        message: AssistantMessage {
            content,
            tool_calls: calls,
        },
        usage: Usage {
            input_tokens: tokens(output.usage.prompt_tokens)?,
            output_tokens: tokens(output.usage.completion_tokens)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_providers::{AgentContext, ToolSpec};

    #[test]
    fn empty_descriptions_keep_readable_provider_tool_identity() {
        let input = ModelRequest {
            context: AgentContext {
                instructions: String::new(),
                messages: vec![Message::User {
                    content: "Read the file".into(),
                }],
            },
            tools: vec![ToolSpec {
                name: ToolName::new("mcp.documents.read_file").unwrap(),
                description: String::new(),
                input_schema: json!({"type":"object"}),
            }],
        };
        let mapping = ToolNames::new(&input).unwrap();
        assert_eq!(
            mapping.encode(&input.tools[0].name),
            "mcp__documents__read_file"
        );
        let encoded = serde_json::to_value(request(&input).unwrap()).unwrap();
        assert!(
            encoded["tools"]
                .to_string()
                .contains("mcp__documents__read_file")
        );
    }

    #[test]
    fn aliases_are_reversible_and_cannot_shadow_portable_or_historical_names() {
        let canonical = [
            "mcp.server.read".to_owned(),
            "mcp.server__read".to_owned(),
            "mcp__server__read".to_owned(),
            "mcp.节点.读取".to_owned(),
            "mcp.点节.取读".to_owned(),
            "x".repeat(255),
            format!("{}y", "x".repeat(254)),
        ];
        let old = ToolName::new("mcp.removed.history").unwrap();
        let input = ModelRequest {
            context: AgentContext {
                instructions: String::new(),
                messages: vec![
                    Message::Assistant(AssistantMessage {
                        content: String::new(),
                        tool_calls: vec![ToolCall {
                            id: ToolCallId::new("old").unwrap(),
                            name: old.clone(),
                            arguments: json!({}),
                        }],
                    }),
                    Message::Tool {
                        call: ToolCallId::new("old").unwrap(),
                        result: ToolResult::Success(json!({})),
                    },
                    Message::User {
                        content: "Continue".into(),
                    },
                ],
            },
            tools: canonical
                .iter()
                .map(|name| ToolSpec {
                    name: ToolName::new(name).unwrap(),
                    description: String::new(),
                    input_schema: json!({"type":"object"}),
                })
                .collect(),
        };
        let original = serde_json::to_value(&input).unwrap();
        let mapping = ToolNames::new(&input).unwrap();
        let aliases: BTreeSet<_> = mapping.outbound.values().collect();
        assert_eq!(aliases.len(), canonical.len() + 1);
        for (name, alias) in &mapping.outbound {
            assert!(portable_name(alias));
            assert_eq!(mapping.inbound.get(alias), Some(name));
        }
        assert_eq!(
            mapping.encode(&ToolName::new("mcp__server__read").unwrap()),
            "mcp__server__read"
        );
        assert_ne!(
            mapping.encode(&ToolName::new("mcp.server.read").unwrap()),
            "mcp__server__read"
        );
        assert!(!mapping.inbound.contains_key("mcp__unknown__tool"));
        let repeated = ToolNames::new(&input).unwrap();
        assert_eq!(mapping.outbound, repeated.outbound);
        for name in [&canonical[3], &canonical[4], &canonical[5], &canonical[6]] {
            let alias = mapping.encode(&ToolName::new(name).unwrap());
            assert_eq!(alias.rsplit('_').next().unwrap().len(), 16);
            assert!(
                alias
                    .rsplit('_')
                    .next()
                    .unwrap()
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
            );
        }
        let encoded = request(&input).unwrap();
        let encoded = serde_json::to_value(encoded).unwrap();
        let encoded = encoded.to_string();
        assert!(!encoded.contains(old.as_str()));
        assert!(encoded.contains(mapping.encode(&old)));
        assert_eq!(serde_json::to_value(input).unwrap(), original);
    }
}
