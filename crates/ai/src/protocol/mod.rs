mod native;
mod openai;

use crate::{Error, Failure, WireMode};
use reqwest::Url;
use serde_json::Value;
use swarmie_model_catalog::{ModelIdentity, Snapshot};

/// A protocol profile is explicit and never inferred from endpoint/model text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatProfile {
    OpenAi,
    DeepSeek,
    Kimi,
}

/// Wire protocols share admission, transport and physical request ownership.
/// Profiles identify distinct metadata qualifications, including compatible APIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    Responses,
    ChatCompletions(ChatProfile),
    Anthropic,
    Gemini,
    Ollama,
}

impl Protocol {
    pub const fn adapter(self) -> &'static str {
        match self {
            Self::Responses => "swarmie-ai.responses.api-key.v1",
            Self::ChatCompletions(ChatProfile::OpenAi) => "swarmie-ai.chat.openai.api-key.v1",
            Self::ChatCompletions(ChatProfile::DeepSeek) => "swarmie-ai.chat.deepseek.api-key.v1",
            Self::ChatCompletions(ChatProfile::Kimi) => "swarmie-ai.chat.kimi.api-key.v1",
            Self::Anthropic => "swarmie-ai.anthropic.api-key.v1",
            Self::Gemini => "swarmie-ai.gemini.api-key.v1",
            // The same native wire protocol permits anonymous local or explicit API-key access.
            Self::Ollama => "swarmie-ai.ollama.native.v1",
        }
    }

    pub(crate) fn prepare(
        self,
        payload: &mut Value,
        mode: WireMode,
        snapshot: &Snapshot,
    ) -> Result<u64, Error> {
        if self != Self::Gemini {
            let object = payload.as_object_mut().ok_or_else(request)?;
            let streaming = mode == WireMode::Stream;
            if object
                .get("stream")
                .is_some_and(|v| v.as_bool() != Some(streaming))
                || object.get("model").and_then(Value::as_str)
                    != Some(snapshot.identity().model.as_str())
            {
                return Err(request());
            }
            object.insert("stream".into(), streaming.into());
        }
        match self {
            Self::Responses => openai::responses(payload),
            Self::ChatCompletions(profile) => openai::chat(payload, mode, profile),
            Self::Anthropic => native::anthropic(payload),
            Self::Gemini => native::gemini(payload),
            Self::Ollama => native::ollama(payload, snapshot),
        }
    }

    /// Gemini metadata binds a model-resource URL. Other protocols bind a full
    /// POST endpoint. Derived Gemini methods/query contain no credentials.
    pub(crate) fn destination(
        self,
        endpoint: &Url,
        mode: WireMode,
        identity: &ModelIdentity,
    ) -> Result<Url, Error> {
        if self != Self::Gemini {
            return Ok(endpoint.clone());
        }
        let path = endpoint.path().trim_end_matches('/');
        if !path.contains("/models/") || path.rsplit('/').next() != Some(identity.model.as_str()) {
            return Err(Error::before(Failure::Configuration));
        }
        let method = if mode == WireMode::Stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let mut destination = endpoint.clone();
        destination.set_path(&format!("{path}:{method}"));
        if mode == WireMode::Stream {
            destination.set_query(Some("alt=sse"));
        }
        Ok(destination)
    }

    pub(crate) fn validate_response(
        self,
        response: &Value,
        identity: &ModelIdentity,
    ) -> Result<(), Error> {
        if !response["error"].is_null()
            || response["type"] == "error"
            || response["status"] == "failed"
        {
            return Err(Error::unknown(Failure::Provider));
        }
        let valid = match self {
            Self::Responses => {
                matches!(
                    response["status"].as_str(),
                    Some("completed" | "incomplete")
                ) && nonempty(&response["id"])
                    && response["model"] == identity.model
                    && response["output"].is_array()
                    && usage(response, "usage", "input_tokens", "output_tokens")
            }
            Self::ChatCompletions(_) => {
                nonempty(&response["id"])
                    && response["model"] == identity.model
                    && response["choices"].as_array().is_some_and(|choices| {
                        !choices.is_empty()
                            && choices.iter().all(|choice| {
                                nonempty(&choice["finish_reason"])
                                    && choice["message"]["role"] == "assistant"
                            })
                    })
                    && usage(response, "usage", "prompt_tokens", "completion_tokens")
            }
            Self::Anthropic => {
                response["type"] == "message"
                    && response["role"] == "assistant"
                    && nonempty(&response["id"])
                    && response["model"] == identity.model
                    && response["content"].is_array()
                    && nonempty(&response["stop_reason"])
                    && usage(response, "usage", "input_tokens", "output_tokens")
            }
            Self::Gemini => {
                response["candidates"].as_array().is_some_and(|candidates| {
                    !candidates.is_empty()
                        && candidates.iter().all(|candidate| {
                            nonempty(&candidate["finishReason"])
                                && candidate["finishReason"] != "FINISH_REASON_UNSPECIFIED"
                        })
                }) && usage(
                    response,
                    "usageMetadata",
                    "promptTokenCount",
                    "candidatesTokenCount",
                ) && identity
                    .revision
                    .as_ref()
                    .is_none_or(|revision| response["modelVersion"] == *revision)
            }
            Self::Ollama => {
                response["done"] == true
                    && response["model"] == identity.model
                    && response["message"].is_object()
                    && response["message"]["role"] == "assistant"
                    && response["prompt_eval_count"].as_u64().is_some()
                    && response["eval_count"].as_u64().is_some()
            }
        };
        if valid && response["error"].is_null() {
            Ok(())
        } else {
            Err(wire())
        }
    }
}

pub(super) fn request() -> Error {
    Error::before(Failure::Request)
}

pub(crate) fn wire() -> Error {
    Error::unknown(Failure::Protocol)
}

pub(crate) fn nonempty(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty())
}

pub(crate) fn usage(value: &Value, field: &str, input: &str, output: &str) -> bool {
    value[field][input].as_u64().is_some() && value[field][output].as_u64().is_some()
}

fn positive(value: &Value) -> Result<u64, Error> {
    value.as_u64().filter(|v| *v > 0).ok_or_else(request)
}

fn messages(value: &Value, roles: &[&str]) -> Result<(), Error> {
    let messages = value
        .as_array()
        .filter(|messages| !messages.is_empty())
        .ok_or_else(request)?;
    if messages.iter().any(|message| {
        message["role"]
            .as_str()
            .is_none_or(|role| !roles.contains(&role))
    }) {
        return Err(request());
    }
    Ok(())
}
