use super::{messages, positive, request};
use crate::{Error, Failure};
use serde_json::Value;
use swarmie_model_catalog::Snapshot;

pub(super) fn anthropic(payload: &Value) -> Result<u64, Error> {
    messages(&payload["messages"], &["user", "assistant"])?;
    for message in payload["messages"].as_array().expect("validated messages") {
        if let Some(blocks) = message["content"].as_array() {
            if blocks.is_empty() || blocks.iter().any(|block| !anthropic_block(block)) {
                return Err(request());
            }
        } else if !message["content"].is_string() {
            return Err(request());
        }
    }
    if let Some(system) = payload.get("system")
        && !system.is_string()
        && system.as_array().is_none_or(|blocks| {
            blocks
                .iter()
                .any(|b| b["type"] != "text" || !b["text"].is_string())
        })
    {
        return Err(request());
    }
    if let Some(tools) = payload.get("tools") {
        let tools = tools.as_array().ok_or_else(request)?;
        if tools.iter().any(|tool| {
            !super::nonempty(&tool["name"])
                || !tool["input_schema"].is_object()
                || tool.get("type").is_some_and(|kind| kind != "custom")
        }) {
            return Err(request());
        }
    }
    if payload.get("container").is_some()
        || payload.get("context_management").is_some()
        || payload.get("fallbacks").is_some()
    {
        return Err(request());
    }
    positive(&payload["max_tokens"])
}

pub(super) fn gemini(payload: &Value) -> Result<u64, Error> {
    messages(&payload["contents"], &["user", "model"])?;
    for message in payload["contents"].as_array().expect("validated contents") {
        let parts = message["parts"]
            .as_array()
            .filter(|parts| !parts.is_empty())
            .ok_or_else(request)?;
        for part in parts {
            let kinds = [
                "text",
                "inlineData",
                "fileData",
                "functionCall",
                "functionResponse",
            ]
            .iter()
            .filter(|kind| part.get(**kind).is_some())
            .count();
            let valid = if let Some(text) = part.get("text") {
                text.is_string()
            } else if let Some(call) = part.get("functionCall") {
                super::nonempty(&call["name"]) && call["args"].is_object()
            } else if let Some(result) = part.get("functionResponse") {
                super::nonempty(&result["name"]) && result["response"].is_object()
            } else if let Some(data) = part.get("inlineData") {
                super::nonempty(&data["mimeType"]) && data["data"].is_string()
            } else if let Some(file) = part.get("fileData") {
                super::nonempty(&file["fileUri"])
            } else {
                false
            };
            if kinds != 1 || !valid || part.get("thoughtSignature").is_some_and(|v| !v.is_string())
            {
                return Err(request());
            }
        }
    }
    if payload.get("model").is_some()
        || payload.get("stream").is_some()
        || payload.get("cachedContent").is_some()
    {
        return Err(request());
    }
    if let Some(tools) = payload.get("tools") {
        let tools = tools.as_array().ok_or_else(request)?;
        if tools.iter().any(|tool| {
            tool.as_object().is_none_or(|object| object.len() != 1)
                || tool["functionDeclarations"].as_array().is_none_or(|items| {
                    items.is_empty() || items.iter().any(|item| !super::nonempty(&item["name"]))
                })
        }) {
            return Err(request());
        }
    }
    if payload["generationConfig"]
        .get("candidateCount")
        .is_some_and(|v| v.as_u64() != Some(1))
    {
        return Err(request());
    }
    if payload["generationConfig"]
        .get("responseModalities")
        .is_some_and(|v| {
            v.as_array()
                .is_none_or(|modes| modes.iter().any(|mode| mode != "TEXT"))
        })
    {
        return Err(request());
    }
    positive(&payload["generationConfig"]["maxOutputTokens"])
}

pub(super) fn ollama(payload: &Value, snapshot: &Snapshot) -> Result<u64, Error> {
    messages(
        &payload["messages"],
        &["system", "user", "assistant", "tool"],
    )?;
    for message in payload["messages"].as_array().expect("validated messages") {
        if !message["content"].is_string() {
            return Err(request());
        }
    }
    if let Some(tools) = payload.get("tools") {
        let tools = tools.as_array().ok_or_else(request)?;
        if tools
            .iter()
            .any(|tool| tool["type"] != "function" || !super::nonempty(&tool["function"]["name"]))
        {
            return Err(request());
        }
    }
    if payload
        .get("think")
        .is_some_and(|v| !(v.is_null() || v.is_boolean() || v.is_string()))
    {
        return Err(request());
    }
    if let Some(tokens) = payload["options"].get("num_ctx") {
        let context = snapshot
            .context()
            .known()
            .expect("validated snapshot")
            .tokens
            .get();
        if positive(tokens)? != context {
            return Err(Error::before(Failure::Metadata));
        }
    }
    positive(&payload["options"]["num_predict"])
}

fn anthropic_block(block: &Value) -> bool {
    match block["type"].as_str() {
        Some("text") => block["text"].is_string(),
        Some("image" | "document") => block["source"].is_object(),
        Some("thinking") => block["thinking"].is_string() && super::nonempty(&block["signature"]),
        Some("redacted_thinking") => super::nonempty(&block["data"]),
        Some("tool_use") => {
            super::nonempty(&block["id"])
                && super::nonempty(&block["name"])
                && block["input"].is_object()
        }
        Some("tool_result") => {
            super::nonempty(&block["tool_use_id"])
                && (block["content"].is_string()
                    || block["content"].as_array().is_some_and(|blocks| {
                        blocks.iter().all(|b| {
                            matches!(b["type"].as_str(), Some("text" | "image" | "document"))
                                && anthropic_block(b)
                        })
                    }))
        }
        _ => false,
    }
}
