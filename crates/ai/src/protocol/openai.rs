use super::{ChatProfile, positive, request};
use crate::{Error, WireMode};
use async_openai::types::{chat::CreateChatCompletionRequest, responses::CreateResponse};
use serde_json::{Value, json};

pub(super) fn responses(payload: &mut Value) -> Result<u64, Error> {
    let object = payload.as_object_mut().ok_or_else(request)?;
    if object.get("background").is_some_and(|v| v != false)
        || object.get("store").is_some_and(|v| v != false)
        || object.get("truncation").is_some_and(|v| v != "disabled")
        || ["conversation", "previous_response_id", "prompt"]
            .iter()
            .any(|key| object.contains_key(*key))
        || object
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|tool| tool["type"] != "function"))
        || object
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item["type"] == "item_reference"))
    {
        return Err(request());
    }
    object.insert("store".into(), false.into());
    object.insert("truncation".into(), "disabled".into());
    // Validate known fields but serialize the original protocol object exactly once.
    let typed: CreateResponse = serde_json::from_value(payload.clone()).map_err(|_| request())?;
    typed
        .max_output_tokens
        .filter(|v| *v > 0)
        .map(u64::from)
        .ok_or_else(request)
}

pub(super) fn chat(
    payload: &mut Value,
    mode: WireMode,
    profile: ChatProfile,
) -> Result<u64, Error> {
    super::messages(
        &payload["messages"],
        &["system", "developer", "user", "assistant", "tool"],
    )?;
    if payload.get("modalities").is_some_and(|v| {
        v.as_array()
            .is_none_or(|modes| modes.iter().any(|mode| mode != "text"))
    }) {
        return Err(request());
    }
    if payload["n"].as_u64().is_some_and(|n| n != 1) {
        return Err(request());
    }
    let object = payload.as_object_mut().ok_or_else(request)?;
    if mode == WireMode::Stream {
        let options = object.entry("stream_options").or_insert_with(|| json!({}));
        let options = options.as_object_mut().ok_or_else(request)?;
        if options.get("include_usage").is_some_and(|v| v != true) {
            return Err(request());
        }
        options.insert("include_usage".into(), true.into());
    } else if object.contains_key("stream_options") {
        return Err(request());
    }
    if object
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.iter().any(|tool| tool["type"] != "function"))
        || object.get("store").is_some_and(|v| v != false)
        || object.contains_key("functions")
        || object.contains_key("function_call")
    {
        return Err(request());
    }
    let cap = match profile {
        ChatProfile::OpenAi => {
            if object.contains_key("max_tokens") == object.contains_key("max_completion_tokens") {
                return Err(request());
            }
            if object.contains_key("max_completion_tokens") {
                "max_completion_tokens"
            } else {
                "max_tokens"
            }
        }
        ChatProfile::DeepSeek | ChatProfile::Kimi => {
            if object.contains_key("max_completion_tokens") {
                return Err(request());
            }
            "max_tokens"
        }
    };
    let reserve = positive(object.get(cap).unwrap_or(&Value::Null))?;
    let mut checked = payload.clone();
    if profile != ChatProfile::OpenAi {
        // Compatible providers have their own reasoning-effort values. Validate
        // the common SDK shape without converting or deleting the actual field.
        if let Some(effort) = checked
            .as_object_mut()
            .expect("protocol object")
            .remove("reasoning_effort")
            && !effort.is_string()
        {
            return Err(request());
        }
    }
    let _: CreateChatCompletionRequest = serde_json::from_value(checked).map_err(|_| request())?;
    Ok(reserve)
}
