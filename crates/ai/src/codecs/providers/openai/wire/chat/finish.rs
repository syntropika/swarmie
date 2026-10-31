use crate::codecs::completion::FinishReason;
use crate::codecs::error::ProviderError;
use crate::codecs::providers::openai::wire::Quirks;

pub(crate) fn provider_error_envelope(
    value: &serde_json::Value,
    data: &str,
) -> Option<ProviderError> {
    let error = value
        .get("error")
        .filter(|error| error.is_object() || error.as_str().is_some_and(|s| !s.is_empty()))?;
    if value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|choices| !choices.is_empty())
    {
        return None;
    }
    if let Some(message) = error.get("message").and_then(serde_json::Value::as_str) {
        tracing::warn!(message, "provider returned a streaming error event");
    }
    Some(ProviderError::from_provider_body(data))
}

pub(crate) fn finish_reason(reason: &str, quirks: &Quirks) -> FinishReason {
    if let Some((_, finish)) = quirks.finishes.iter().find(|(name, _)| *name == reason) {
        return finish.clone();
    }
    match reason {
        "stop" | "end" | "eos" | "end_turn" | "stop_sequence" => FinishReason::Stop,
        "length" | "max_tokens" | "model_length" => FinishReason::Length,
        "tool_calls" | "function_call" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Other(reason.to_owned()),
    }
}

pub(crate) fn native_finish_reason(reason: &str) -> FinishReason {
    let reason = reason.to_ascii_lowercase();
    match reason.as_str() {
        "stop" | "end_turn" | "stop_sequence" | "complete" | "completed" => FinishReason::Stop,
        "length" | "max_tokens" | "max_output_tokens" | "model_length" => FinishReason::Length,
        "tool_calls" | "function_call" | "tool_use" => FinishReason::ToolCalls,
        "content_filter" | "safety" | "blocklist" | "prohibited_content" | "spii" => {
            FinishReason::ContentFilter
        }
        _ => FinishReason::Other(reason),
    }
}
