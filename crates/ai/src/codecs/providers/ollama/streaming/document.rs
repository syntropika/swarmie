use serde_json::{Map, Value};

use super::ChatRecord;
use crate::codecs::wire::document::Reassemble;

#[derive(Debug, Default)]
pub struct ChatResponse {
    document: Map<String, Value>,
}

const APPENDED: &[&str] = &["content", "thinking"];

const COLLECTED: &[&str] = &["tool_calls", "images", "logprobs"];

fn fold<'a>(
    into: &mut Map<String, Value>,
    fields: impl IntoIterator<Item = (&'a String, &'a Value)>,
    appended: &[&str],
) {
    for (key, value) in fields {
        match (into.get_mut(key), value) {
            (Some(Value::String(text)), Value::String(more))
                if appended.contains(&key.as_str()) =>
            {
                text.push_str(more);
            }
            (Some(Value::Array(items)), Value::Array(more))
                if COLLECTED.contains(&key.as_str()) =>
            {
                items.extend(more.iter().cloned());
            }
            (Some(_), Value::Null) => {}
            (_, value) => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

impl crate::codecs::wire::document::Serves<crate::codecs::operation::Completion> for ChatResponse {}

impl Reassemble<ChatRecord> for ChatResponse {
    fn absorb(&mut self, ChatRecord(fields): &ChatRecord) {
        if let Some(Value::Object(message)) = fields.get("message") {
            match self.document.get_mut("message") {
                Some(Value::Object(into)) => fold(into, message, APPENDED),
                _ => {
                    let message = Value::Object(message.clone());
                    self.document.insert("message".to_owned(), message);
                }
            }
        }
        let rest = fields.iter().filter(|(key, _)| *key != "message");
        fold(&mut self.document, rest, &[]);
    }

    fn finish(self) -> Value {
        if self.document.is_empty() {
            Value::Null
        } else {
            Value::Object(self.document)
        }
    }
}
