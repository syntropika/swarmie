use serde_json::{Map, Value};

use super::{GenerateContentChunk, merge_part};
use crate::codecs::json_utils::Lenient;
use crate::codecs::wire::document::{Reassemble, keep, slot};

#[derive(Debug, Default)]
pub struct GenerateContentResponse {
    document: Map<String, Value>,
    indices: Vec<u64>,
    signatures: Vec<(u64, String)>,
}

impl GenerateContentResponse {
    #[doc(hidden)]
    pub fn chunk(&mut self, chunk: &Map<String, Value>) {
        for (key, value) in chunk {
            match (key.as_str(), value) {
                ("candidates", Value::Array(candidates)) => {
                    for (position, candidate) in candidates.iter().enumerate() {
                        self.candidate(position, candidate);
                    }
                }
                // Candidates of another type fail the decoder; the document
                // keeps the ones it holds.
                ("candidates", _) => {}
                ("promptFeedback", value) => {
                    if self.document.get(key).is_none_or(Value::is_null) {
                        self.document.insert(key.clone(), value.clone());
                    }
                }
                (_, value) => keep(&mut self.document, key, value),
            }
        }
    }

    #[doc(hidden)]
    pub fn document(self) -> Value {
        if self.document.is_empty() {
            Value::Null
        } else {
            Value::Object(self.document)
        }
    }

    fn candidate(&mut self, position: usize, candidate: &Value) {
        let Value::Object(candidate) = candidate else {
            return;
        };
        let index = candidate
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or(position as u64);
        let candidates = slot(&mut self.document, "candidates", Value::Array);
        let Value::Array(candidates) = candidates else {
            return;
        };
        let at = match self.indices.iter().position(|held| *held == index) {
            Some(at) => at,
            None => {
                self.indices.push(index);
                candidates.push(Value::Object(Map::new()));
                candidates.len() - 1
            }
        };
        let Some(Value::Object(held)) = candidates.get_mut(at) else {
            return;
        };
        for (key, value) in candidate {
            match (key.as_str(), value) {
                ("content", Value::Object(content)) => {
                    if let Value::Object(held) = slot(held, "content", Value::Object) {
                        content_into(held, content, index, &mut self.signatures);
                    }
                }
                ("citationMetadata", Value::Object(citations)) => {
                    if let Value::Object(held) = slot(held, "citationMetadata", Value::Object) {
                        for (key, value) in citations {
                            match (key.as_str(), held.get_mut(key), value) {
                                (
                                    "citationSources",
                                    Some(Value::Array(sources)),
                                    Value::Array(more),
                                ) => {
                                    sources.extend(more.iter().cloned());
                                }
                                (_, _, value) => keep(held, key, value),
                            }
                        }
                    }
                }
                (_, value) => keep(held, key, value),
            }
        }
    }
}

fn content_into(
    held: &mut Map<String, Value>,
    content: &Map<String, Value>,
    index: u64,
    signatures: &mut Vec<(u64, String)>,
) {
    for (key, value) in content {
        let Value::Array(more) = value else {
            keep(held, key, value);
            continue;
        };
        if key != "parts" {
            keep(held, key, value);
            continue;
        }
        let Value::Array(parts) = slot(held, "parts", Value::Array) else {
            return;
        };
        for part in more {
            part_into(parts, part, index, signatures);
        }
    }
}

fn part_into(
    parts: &mut Vec<Value>,
    part: &Value,
    index: u64,
    signatures: &mut Vec<(u64, String)>,
) {
    let signature = part
        .str("thoughtSignature")
        .filter(|signature| !signature.is_empty())
        .map(str::to_owned);
    if let Some(text) = part.str("text") {
        if text.is_empty() && signature.is_none() {
            return;
        }
        let thought = part.bool("thought") == Some(true);
        if let Some(last) = parts.last_mut()
            && last.str("text").is_some()
            && (last.bool("thought") == Some(true)) == thought
            && !(signed(last) && signature.is_some())
        {
            merge_part(last, part);
            return;
        }
    } else if let Value::Object(fields) = part {
        let bare = ["thought", "thoughtSignature", "partMetadata"];
        let data = fields.keys().any(|key| !bare.contains(&key.as_str()));
        if !data && let Some(signature) = signature {
            match parts.last_mut().and_then(Value::as_object_mut) {
                Some(last) => {
                    last.insert("thoughtSignature".to_owned(), Value::String(signature));
                }
                None => signatures.push((index, signature)),
            }
            return;
        }
    }
    let mut part = part.clone();
    if let Some(at) = signatures.iter().position(|(held, _)| *held == index)
        && let Some(fields) = part.as_object_mut()
    {
        let (_, signature) = signatures.remove(at);
        fields
            .entry("thoughtSignature")
            .or_insert(Value::String(signature));
    }
    parts.push(part);
}

fn signed(part: &Value) -> bool {
    part.str("thoughtSignature")
        .is_some_and(|signature| !signature.is_empty())
}

impl crate::codecs::wire::document::Serves<crate::codecs::operation::Completion>
    for GenerateContentResponse
{
}

impl Reassemble<GenerateContentChunk> for GenerateContentResponse {
    fn absorb(&mut self, GenerateContentChunk(chunk): &GenerateContentChunk) {
        self.chunk(chunk);
    }

    fn finish(self) -> Value {
        self.document()
    }
}
