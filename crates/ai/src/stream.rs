use crate::catalog::ModelIdentity;
use crate::{
    Error, Outcome, Protocol,
    framing::{Frame, Framer},
    protocol::{nonempty, usage, wire},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::watch;

/// Protocol terminal recognition and raw event retention. Reassembly runs only
/// after completion; previews and partial arguments never authorize tool execution.
pub(crate) struct Decoder {
    protocol: Protocol,
    identity: ModelIdentity,
    framer: Framer,
    events: Vec<Value>,
    text: String,
    id: Option<String>,
    choices: BTreeMap<u64, bool>,
    usage: bool,
    started: bool,
    open: BTreeSet<u64>,
    seen: BTreeSet<u64>,
    stop_reason: bool,
    output_tokens: Option<u64>,
}

impl Decoder {
    pub fn new(protocol: Protocol, identity: ModelIdentity) -> Self {
        Self {
            protocol,
            framer: Framer::new(protocol),
            identity,
            events: Vec::new(),
            text: String::new(),
            id: None,
            choices: BTreeMap::new(),
            usage: false,
            started: false,
            open: BTreeSet::new(),
            seen: BTreeSet::new(),
            stop_reason: false,
            output_tokens: None,
        }
    }

    pub fn push(
        &mut self,
        bytes: &[u8],
        previews: &watch::Sender<Option<String>>,
    ) -> Result<Option<Outcome>, Error> {
        let frames = self.framer.push(bytes)?;
        self.consume(frames, previews)
    }

    pub fn finish(mut self, previews: &watch::Sender<Option<String>>) -> Result<Outcome, Error> {
        let frames = self.framer.finish()?;
        if let Some(outcome) = self.consume(frames, previews)? {
            return Ok(outcome);
        }
        if self.protocol == Protocol::Gemini && self.finished_choices() && self.usage {
            return self.outcome(None);
        }
        Err(wire())
    }

    fn consume(
        &mut self,
        frames: Vec<Frame>,
        previews: &watch::Sender<Option<String>>,
    ) -> Result<Option<Outcome>, Error> {
        for frame in frames {
            if matches!(frame, Frame::Invalid) {
                return Err(wire());
            }
            let Frame::Json { name, value } = frame else {
                if matches!(self.protocol, Protocol::ChatCompletions(_))
                    && self.finished_choices()
                    && self.usage
                {
                    return self.outcome(None).map(Some);
                }
                return Err(wire());
            };
            if !value["error"].is_null() || value["type"] == "error" {
                return Err(Error::unknown(crate::Failure::Provider));
            }
            if matches!(self.protocol, Protocol::Responses | Protocol::Anthropic)
                && name
                    .as_deref()
                    .is_some_and(|name| value["type"].as_str() != Some(name))
            {
                return Err(wire());
            }
            let mut complete = false;
            let mut final_response = None;
            match self.protocol {
                Protocol::Responses => match value["type"].as_str().ok_or_else(wire)? {
                    "response.completed" | "response.incomplete" => {
                        let status = if value["type"] == "response.completed" {
                            "completed"
                        } else {
                            "incomplete"
                        };
                        if value["response"]["status"] != status {
                            return Err(wire());
                        }
                        self.protocol
                            .validate_response(&value["response"], &self.identity)?;
                        final_response = Some(value["response"].clone());
                        complete = true;
                    }
                    "response.failed" => return Err(Error::unknown(crate::Failure::Provider)),
                    "response.output_text.delta" => {
                        self.preview(value["delta"].as_str().ok_or_else(wire)?, previews)
                    }
                    _ => {}
                },
                Protocol::ChatCompletions(_) => {
                    self.bind_id(&value["id"])?;
                    if value["model"] != self.identity.model
                        || value["object"] != "chat.completion.chunk"
                    {
                        return Err(wire());
                    }
                    let choices = value["choices"].as_array().ok_or_else(wire)?;
                    for choice in choices {
                        let index = choice["index"]
                            .as_u64()
                            .filter(|v| *v == 0)
                            .ok_or_else(wire)?;
                        if *self.choices.get(&index).unwrap_or(&false) {
                            return Err(wire());
                        }
                        if !choice["delta"].is_object() {
                            return Err(wire());
                        }
                        self.choices
                            .insert(index, nonempty(&choice["finish_reason"]));
                        if let Some(delta) = choice["delta"]["content"].as_str() {
                            self.preview(delta, previews);
                        }
                    }
                    if !value["usage"].is_null() {
                        if !usage(&value, "usage", "prompt_tokens", "completion_tokens")
                            || !self.finished_choices()
                        {
                            return Err(wire());
                        }
                        self.usage = true;
                    }
                }
                Protocol::Anthropic => complete = self.anthropic(&value, previews)?,
                Protocol::Gemini => {
                    if self
                        .identity
                        .revision
                        .as_ref()
                        .is_some_and(|revision| value["modelVersion"] != *revision)
                    {
                        return Err(wire());
                    }
                    if let Some(id) = value.get("responseId") {
                        self.bind_id(id)?;
                    }
                    let candidates = match value.get("candidates") {
                        Some(candidates) => candidates.as_array().ok_or_else(wire)?.as_slice(),
                        None if value.get("usageMetadata").is_some() => &[],
                        None => return Err(wire()),
                    };
                    for candidate in candidates {
                        let index = candidate
                            .get("index")
                            .map_or(Some(0), Value::as_u64)
                            .filter(|v| *v == 0)
                            .ok_or_else(wire)?;
                        if *self.choices.get(&index).unwrap_or(&false) {
                            return Err(wire());
                        }
                        let finished = nonempty(&candidate["finishReason"])
                            && candidate["finishReason"] != "FINISH_REASON_UNSPECIFIED";
                        self.choices.insert(index, finished);
                        if let Some(parts) = candidate["content"]["parts"].as_array() {
                            for part in parts {
                                if part["thought"] != true
                                    && let Some(text) = part["text"].as_str()
                                {
                                    self.preview(text, previews);
                                }
                            }
                        }
                    }
                    if value.get("usageMetadata").is_some() {
                        for field in [
                            "promptTokenCount",
                            "candidatesTokenCount",
                            "thoughtsTokenCount",
                        ] {
                            if value["usageMetadata"]
                                .get(field)
                                .is_some_and(|v| v.as_u64().is_none())
                            {
                                return Err(wire());
                            }
                        }
                        if self.finished_choices() {
                            if !usage(
                                &value,
                                "usageMetadata",
                                "promptTokenCount",
                                "candidatesTokenCount",
                            ) {
                                return Err(wire());
                            }
                            self.usage = true;
                        }
                    }
                }
                Protocol::Ollama => {
                    if value["model"] != self.identity.model || value["done"].as_bool().is_none() {
                        return Err(wire());
                    }
                    if let Some(text) = value["message"]["content"].as_str() {
                        self.preview(text, previews);
                    }
                    if value["done"] == true {
                        self.protocol.validate_response(&value, &self.identity)?;
                        complete = true;
                    }
                }
            }
            self.events.push(value);
            if complete {
                return self.outcome(final_response).map(Some);
            }
        }
        Ok(None)
    }

    fn anthropic(
        &mut self,
        value: &Value,
        previews: &watch::Sender<Option<String>>,
    ) -> Result<bool, Error> {
        match value["type"].as_str().ok_or_else(wire)? {
            "ping" => {}
            "message_start" => {
                if self.started
                    || value["message"]["model"] != self.identity.model
                    || value["message"]["usage"]["input_tokens"].as_u64().is_none()
                {
                    return Err(wire());
                }
                self.bind_id(&value["message"]["id"])?;
                self.started = true;
            }
            "content_block_start" => {
                let index = value["index"].as_u64().ok_or_else(wire)?;
                if !self.started || !self.seen.insert(index) || !value["content_block"].is_object()
                {
                    return Err(wire());
                }
                self.open.insert(index);
                if value["content_block"]["type"] == "text"
                    && let Some(text) = value["content_block"]["text"].as_str()
                {
                    self.preview(text, previews);
                }
            }
            "content_block_delta" => {
                let index = value["index"].as_u64().ok_or_else(wire)?;
                if !self.open.contains(&index) || !value["delta"].is_object() {
                    return Err(wire());
                }
                if value["delta"]["type"] == "text_delta" {
                    self.preview(value["delta"]["text"].as_str().ok_or_else(wire)?, previews);
                }
            }
            "content_block_stop" => {
                if !self.open.remove(&value["index"].as_u64().ok_or_else(wire)?) {
                    return Err(wire());
                }
            }
            "message_delta" => {
                if !self.started || !self.open.is_empty() {
                    return Err(wire());
                }
                self.stop_reason |= nonempty(&value["delta"]["stop_reason"]);
                let tokens = value["usage"]["output_tokens"].as_u64().ok_or_else(wire)?;
                if self.output_tokens.is_some_and(|previous| tokens < previous) {
                    return Err(wire());
                }
                self.output_tokens = Some(tokens);
            }
            "message_stop" => {
                if !self.started
                    || !self.open.is_empty()
                    || !self.stop_reason
                    || self.output_tokens.is_none()
                {
                    return Err(wire());
                }
                return Ok(true);
            }
            _ => {} // Unknown event JSON stays in the retained protocol transcript.
        }
        Ok(false)
    }

    fn finished_choices(&self) -> bool {
        !self.choices.is_empty() && self.choices.values().all(|v| *v)
    }

    fn bind_id(&mut self, value: &Value) -> Result<(), Error> {
        let id = value
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(wire)?;
        if self.id.as_ref().is_some_and(|previous| previous != id) {
            return Err(wire());
        }
        self.id = Some(id.into());
        Ok(())
    }

    fn preview(&mut self, text: &str, previews: &watch::Sender<Option<String>>) {
        self.text.push_str(text);
        previews.send_replace(Some(self.text.clone()));
    }

    fn outcome(&mut self, response: Option<Value>) -> Result<Outcome, Error> {
        let response = match response {
            Some(response) => response,
            None => crate::document::assemble(self.protocol, &self.events)?,
        };
        self.protocol.validate_response(&response, &self.identity)?;
        Ok(Outcome::streamed(
            self.protocol,
            Some(response),
            std::mem::take(&mut self.events),
        ))
    }
}
