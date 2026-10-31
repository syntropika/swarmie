//! Rebuild one Messages response. The decoder enforces block lifetimes first.
use super::{keep, slot};
use crate::{Error, protocol::wire};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Message {
    message: Map<String, Value>,
    blocks: BTreeMap<u64, Value>,
    inputs: BTreeMap<u64, String>,
}

impl Message {
    pub(super) fn absorb(&mut self, event: &Value) -> Result<(), Error> {
        match event["type"].as_str().ok_or_else(wire)? {
            "message_start" => {
                self.message = event["message"].as_object().ok_or_else(wire)?.clone();
                // A start is a skeleton, not a second source of content blocks.
                if self
                    .message
                    .get("content")
                    .is_some_and(|v| v.as_array().is_none_or(|v| !v.is_empty()))
                {
                    return Err(wire());
                }
            }
            "content_block_start" => {
                self.blocks.insert(
                    event["index"].as_u64().ok_or_else(wire)?,
                    event["content_block"].clone(),
                );
            }
            "content_block_delta" => self.delta(
                event["index"].as_u64().ok_or_else(wire)?,
                event["delta"].as_object().ok_or_else(wire)?,
            )?,
            "content_block_stop" => {
                let index = event["index"].as_u64().ok_or_else(wire)?;
                if let Some(json) = self.inputs.remove(&index) {
                    // Never replace malformed or truncated arguments with an empty object.
                    let input: Value = serde_json::from_str(&json).map_err(|_| wire())?;
                    if !input.is_object() {
                        return Err(wire());
                    }
                    self.blocks
                        .get_mut(&index)
                        .and_then(Value::as_object_mut)
                        .ok_or_else(wire)?
                        .insert("input".into(), input);
                }
            }
            "message_delta" => {
                for (key, value) in event["delta"].as_object().ok_or_else(wire)? {
                    keep(&mut self.message, key, value);
                }
                let usage = slot(&mut self.message, "usage", Value::Object)
                    .as_object_mut()
                    .ok_or_else(wire)?;
                for (key, value) in event["usage"].as_object().ok_or_else(wire)? {
                    keep(usage, key, value);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn delta(&mut self, index: u64, delta: &Map<String, Value>) -> Result<(), Error> {
        let block = self
            .blocks
            .get_mut(&index)
            .and_then(Value::as_object_mut)
            .ok_or_else(wire)?;
        let kind = delta.get("type").and_then(Value::as_str).ok_or_else(wire)?;
        if kind == "input_json_delta" {
            self.inputs.entry(index).or_default().push_str(
                delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .ok_or_else(wire)?,
            );
        } else if kind == "citations_delta" {
            let citation = delta.get("citation").ok_or_else(wire)?.clone();
            slot(block, "citations", Value::Array)
                .as_array_mut()
                .ok_or_else(wire)?
                .push(citation);
        } else {
            // Preserve the block's type. Strings and arrays in deltas append.
            for (key, value) in delta {
                if key == "type" {
                    continue;
                }
                match (block.get_mut(key), value) {
                    (Some(Value::String(text)), Value::String(more)) => text.push_str(more),
                    (Some(Value::Array(items)), Value::Array(more)) => {
                        items.extend(more.iter().cloned())
                    }
                    _ => {
                        block.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Value {
        // Compact storage is bounded by received blocks, even for sparse wire indices.
        self.message.insert(
            "content".into(),
            Value::Array(self.blocks.into_values().collect()),
        );
        Value::Object(self.message)
    }
}
