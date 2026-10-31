use serde_json::{Map, Value};

use super::{CallKind, ChatEvent, REASONING_DETAILS, merge_details};
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::wire::document::{Reassemble, keep};

const OBJECT: &str = "chat.completion";
const STREAM_PADDING: [&str; 2] = ["obfuscation", "p"];
const APPENDED_TEXT: [&str; 4] = ["content", "refusal", "reasoning", "reasoning_content"];
const APPENDED_LISTS: [&str; 2] = ["annotations", "images"];

#[derive(Default)]
pub struct ChatCompletion {
    envelope: Map<String, Value>,
    choices: Vec<Choice>,
    bare: bool,
}

struct Choice {
    index: u64,
    fields: Map<String, Value>,
    whole: bool,
}

impl ChatCompletion {
    fn frame(&mut self, frame: &Value) {
        for (key, value) in frame.as_object().into_iter().flatten() {
            match key.as_str() {
                "choices" => {
                    self.envelope
                        .entry(key.clone())
                        .or_insert(Value::Array(Vec::new()));
                    for choice in value.as_array().into_iter().flatten() {
                        self.choice(choice);
                    }
                }
                // Per-chunk padding against size side channels (OpenAI's
                // `obfuscation`, Mistral's `p`); a unary body has none.
                key if STREAM_PADDING.contains(&key) => {}
                "object" if value.is_string() => {
                    self.envelope.insert(key.clone(), Value::from(OBJECT));
                }
                _ => keep(&mut self.envelope, key, value),
            }
        }
    }

    fn choice(&mut self, chunk: &Value) {
        let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0);
        let position = self.choices.iter().position(|choice| choice.index == index);
        let choice = match position.and_then(|position| self.choices.get_mut(position)) {
            Some(choice) => choice,
            None => {
                self.choices.push(Choice {
                    index,
                    fields: Map::new(),
                    whole: false,
                });
                match self.choices.last_mut() {
                    Some(choice) => choice,
                    None => return,
                }
            }
        };
        // A choice that carries its whole message (a whole reply, or a
        // dialect that streams the message so far beside each delta) is
        // kept as sent.
        choice.whole = chunk.get("message").is_some();
        let fields = &mut choice.fields;
        for (key, value) in chunk.as_object().into_iter().flatten() {
            match (key.as_str(), value) {
                ("message", _) => {
                    fields.insert(key.clone(), value.clone());
                }
                ("delta", Value::Object(delta)) if !choice.whole => {
                    if let Value::Object(message) = fields
                        .entry("message")
                        .or_insert_with(|| Value::Object(Map::new()))
                    {
                        merge_delta(message, delta);
                    }
                }
                ("delta", _) if !choice.whole => {}
                ("logprobs", _) if !choice.whole => merge_logprobs(fields, value),
                _ => keep(fields, key, value),
            }
        }
    }
}

impl crate::codecs::wire::document::Serves<crate::codecs::operation::Completion>
    for ChatCompletion
{
}

impl Reassemble<ChatEvent> for ChatCompletion {
    fn absorb(&mut self, event: &ChatEvent) {
        match event {
            ChatEvent::Chunk(frame) | ChatEvent::Whole(frame) => self.frame(frame),
            ChatEvent::BareText(_) => self.bare = true,
            // The sentinel and the error envelope are not part of the
            // document.
            ChatEvent::Done | ChatEvent::Failure(_) => {}
        }
    }

    fn finish(self) -> Value {
        let Self {
            mut envelope,
            mut choices,
            bare,
            ..
        } = self;
        if bare || envelope.is_empty() {
            return Value::Null;
        }
        if envelope.contains_key("choices") {
            // A unary body lists its choices in `index` order; a stream
            // interleaves them.
            choices.sort_by_key(|choice| choice.index);
            let choices = choices
                .into_iter()
                .map(|mut choice| {
                    if let (false, Some(Value::Object(message))) =
                        (choice.whole, choice.fields.get_mut("message"))
                    {
                        finish_message(message);
                    }
                    Value::Object(choice.fields)
                })
                .collect();
            envelope.insert("choices".to_owned(), Value::Array(choices));
        }
        Value::Object(envelope)
    }
}

fn append_text(map: &mut Map<String, Value>, key: &str, fragment: &str) {
    match map.get_mut(key) {
        Some(Value::String(text)) => text.push_str(fragment),
        _ => {
            map.insert(key.to_owned(), Value::from(fragment));
        }
    }
}

fn merge_appending(map: &mut Map<String, Value>, part: &Map<String, Value>, appended: &[&str]) {
    for (key, value) in part {
        match value {
            Value::String(fragment) if appended.contains(&key.as_str()) => {
                append_text(map, key, fragment);
            }
            _ => keep(map, key, value),
        }
    }
}

fn merge_delta(message: &mut Map<String, Value>, delta: &Map<String, Value>) {
    for (key, value) in delta {
        match (key.as_str(), value) {
            (text, Value::String(fragment)) if APPENDED_TEXT.contains(&text) => {
                append_text(message, key, fragment);
            }
            ("tool_calls", Value::Array(calls)) => {
                for call in calls {
                    merge_call(message, call);
                }
            }
            (REASONING_DETAILS, Value::Array(details)) => {
                merge_details(message, details.clone());
            }
            (list, Value::Array(entries)) if APPENDED_LISTS.contains(&list) => {
                match message.get_mut(key) {
                    Some(Value::Array(merged)) => merged.extend(entries.iter().cloned()),
                    _ => {
                        message.insert(key.clone(), value.clone());
                    }
                }
            }
            ("audio", Value::Object(audio)) => {
                if let Value::Object(merged) = message
                    .entry(key.clone())
                    .or_insert_with(|| Value::Object(Map::new()))
                {
                    merge_appending(merged, audio, &["data", "transcript"]);
                }
            }
            _ => keep(message, key, value),
        }
    }
}

fn merge_call(message: &mut Map<String, Value>, fragment: &Value) {
    let Value::Object(fragment) = fragment else {
        return;
    };
    if !matches!(message.get("tool_calls"), Some(Value::Array(_))) {
        message.insert("tool_calls".to_owned(), Value::Array(Vec::new()));
    }
    let Some(Value::Array(calls)) = message.get_mut("tool_calls") else {
        return;
    };
    let index = fragment.get("index").and_then(Value::as_u64);
    let id = stated_id(fragment);
    let named = |part: &str| {
        fragment
            .get(part)
            .and_then(|part| part.get("name"))
            .and_then(Value::as_str)
            .is_some_and(|name| !name.is_empty())
    };
    let names = named("function") || named("custom");
    let position = match index {
        Some(index) => calls
            .iter()
            .rposition(|call| call.get("index").and_then(Value::as_u64) == Some(index))
            .filter(|&position| {
                calls.get(position).is_none_or(|held| {
                    let replaced = id.as_ref().is_some_and(|id| {
                        held.as_object()
                            .and_then(stated_id)
                            .is_some_and(|held| held != *id)
                    });
                    !(replaced && (arguments_complete(held) || names))
                })
            }),
        None => match &id {
            Some(id) => calls
                .iter()
                .rposition(|call| call.as_object().and_then(stated_id).as_ref() == Some(id)),
            None => {
                let starts = names
                    || fragment
                        .get("function")
                        .and_then(|function| function.get("arguments"))
                        .map(crate::codecs::json_utils::value_to_json_string)
                        .is_some_and(|arguments| !arguments.is_empty());
                calls.len().checked_sub(1).filter(|&last| {
                    calls.get(last).is_some_and(|latest| {
                        CallKind::of(latest) == CallKind::Unknown
                            || !arguments_complete(latest)
                            || !starts
                    })
                })
            }
        },
    };
    let position = position.unwrap_or_else(|| {
        let mut call = Map::new();
        if let Some(index) = index {
            call.insert("index".to_owned(), Value::from(index));
        }
        calls.push(Value::Object(call));
        calls.len() - 1
    });
    let Some(Value::Object(call)) = calls.get_mut(position) else {
        return;
    };
    for (key, value) in fragment {
        match (key.as_str(), value) {
            ("index", _) => {}
            ("function" | "custom", Value::Object(function)) => {
                if let Value::Object(merged) = call
                    .entry(key.clone())
                    .or_insert_with(|| Value::Object(Map::new()))
                {
                    merge_appending(merged, function, &["arguments", "input"]);
                }
            }
            _ => keep(call, key, value),
        }
    }
}

fn stated_id(call: &Map<String, Value>) -> Option<String> {
    match call.get("id") {
        Some(Value::String(id)) if !id.is_empty() && id != "null" => Some(id.clone()),
        Some(id @ Value::Number(_)) => Some(id.to_string()),
        _ => None,
    }
}

fn arguments_complete(call: &Value) -> bool {
    call.pointer("/function/arguments")
        .and_then(Value::as_str)
        .is_some_and(crate::codecs::json_utils::is_whole_object)
}

fn merge_logprobs(choice: &mut Map<String, Value>, logprobs: &Value) {
    let (Some(Value::Object(merged)), Value::Object(logprobs)) =
        (choice.get_mut("logprobs"), logprobs)
    else {
        keep(choice, "logprobs", logprobs);
        return;
    };
    for (key, value) in logprobs {
        match (merged.get_mut(key), value) {
            (Some(Value::Array(tokens)), Value::Array(more)) => tokens.extend(more.iter().cloned()),
            _ => keep(merged, key, value),
        }
    }
}

fn finish_message(message: &mut Map<String, Value>) {
    let mut called = false;
    if let Some(Value::Array(calls)) = message.get_mut("tool_calls") {
        for call in calls.iter_mut().filter_map(Value::as_object_mut) {
            remove_preserving_order(call, "index");
        }
        called = !calls.is_empty();
    }
    let refused = message
        .get("refusal")
        .and_then(Value::as_str)
        .is_some_and(|refusal| !refusal.is_empty());
    if (called || refused) && message.get("content").and_then(Value::as_str) == Some("") {
        message.insert("content".to_owned(), Value::Null);
    }
}
