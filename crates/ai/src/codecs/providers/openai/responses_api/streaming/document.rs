use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::ResponsesEvent;
use crate::codecs::json_utils::Lenient;
use crate::codecs::wire::document::{Reassemble, keep};

#[derive(Debug, Default)]
pub struct Response {
    response: Option<Value>,
    terminal: bool,
    items: BTreeMap<usize, Value>,
    beside: Map<String, Value>,
}

impl Response {
    fn item(&mut self, frame: &Value, done: bool) {
        let Some(item) = frame.get("item").filter(|item| item.is_object()) else {
            return;
        };
        let index = frame
            .u64("output_index")
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or_else(|| {
                self.items
                    .last_key_value()
                    .map_or(0, |(last, _)| last.saturating_add(1))
            });
        if done {
            self.items.insert(index, item.clone());
        } else {
            self.items.entry(index).or_insert_with(|| item.clone());
        }
    }

    fn lifecycle(&mut self, kind: &str, frame: &Value) {
        if self.terminal {
            return;
        }
        for (key, value) in frame.as_object().into_iter().flatten() {
            let event_field = matches!(key.as_str(), "type" | "sequence_number" | "response");
            if !event_field {
                keep(&mut self.beside, key, value);
            }
        }
        let status = match kind {
            "response.completed" => Some("completed"),
            "response.incomplete" => Some("incomplete"),
            "response.failed" => Some("failed"),
            _ => None,
        };
        let mut response = frame
            .get("response")
            .filter(|response| response.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(status) = status {
            self.terminal = true;
            if let Some(fields) = response.as_object_mut() {
                fields.insert("status".to_owned(), json!(status));
            }
        }
        self.response = Some(response);
    }
}

impl crate::codecs::wire::document::Serves<crate::codecs::operation::Completion> for Response {}

impl Reassemble<ResponsesEvent> for Response {
    fn absorb(&mut self, event: &ResponsesEvent) {
        match event {
            ResponsesEvent::Frame { kind, frame, .. } => match kind.as_str() {
                "response.output_item.added" => self.item(frame, false),
                "response.output_item.done" => self.item(frame, true),
                kind if super::is_lifecycle_event(kind) => self.lifecycle(kind, frame),
                _ => {}
            },
            ResponsesEvent::Whole(body) if !self.terminal => {
                self.terminal = true;
                self.response = Some(body.clone());
            }
            // An `error` event ends the reply with the response so far.
            _ => {}
        }
    }

    fn finish(self) -> Value {
        let Some(mut response) = self.response.or_else(|| {
            (!self.items.is_empty()).then(|| json!({ "object": "response", "output": [] }))
        }) else {
            return Value::Null;
        };
        if let Some(fields) = response.as_object_mut() {
            for (key, value) in self.beside {
                fields.entry(key).or_insert(value);
            }
        }
        if let Some(fields) = response.as_object_mut()
            && fields
                .get("output")
                .is_none_or(|output| output.as_array().is_none_or(Vec::is_empty))
            && !self.items.is_empty()
        {
            fields.insert(
                "output".to_owned(),
                Value::Array(self.items.into_values().collect()),
            );
        }
        response
    }
}
