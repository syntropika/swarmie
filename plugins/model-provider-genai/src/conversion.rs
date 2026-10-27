use crate::error::{Reason, invalid};
use genai::chat::{
    ChatMessage, ChatRequest, ChatResponse, ContentPart, MessageContent, StopReason, Tool,
    ToolResponse,
};
use serde_json::json;
use std::collections::{BTreeSet, VecDeque};
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

/// Validate portable tool pairing before any network work. Function names are
/// recovered from their preceding calls, including on Gemini result messages.
pub(crate) fn request(input: &ModelRequest) -> tembra::Result<ChatRequest> {
    let mut names = BTreeSet::new();
    let mut tools = Vec::new();
    for tool in &input.tools {
        if !portable_name(tool.name.as_str())
            || !names.insert(&tool.name)
            || tool.input_schema.get("type").and_then(|v| v.as_str()) != Some("object")
        {
            return Err(invalid(
                Reason::Request,
                "tools require distinct portable names and object parameter schemas",
            ));
        }
        tools.push(
            Tool::new(tool.name.as_str())
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
                    if !portable_name(call.name.as_str())
                        || !call.arguments.is_object()
                        || !ids.insert(&call.id)
                    {
                        return Err(invalid(
                            Reason::Request,
                            "invalid or repeated historical tool call",
                        ));
                    }
                    pending.push_back((&call.id, &call.name));
                    parts.push(ContentPart::ToolCall(genai::chat::ToolCall {
                        call_id: call.id.as_str().into(),
                        fn_name: call.name.as_str().into(),
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
                let response =
                    ToolResponse::new(call.as_str(), value.to_string()).with_fn_name(name.as_str());
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
                let name = ToolName::new(call.fn_name)?;
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
