//! Rebuild provider documents only after the protocol confirms completion.
//! Original event JSON remains available independently of the reconstructed view.
mod anthropic;
mod chat;
mod gemini;
mod ollama;

use crate::{Error, Protocol, protocol::wire};
use serde_json::Value;

pub(crate) fn assemble(protocol: Protocol, events: &[Value]) -> Result<Value, Error> {
    match protocol {
        Protocol::ChatCompletions(_) => {
            let mut document = chat::ChatCompletion::default();
            for event in events {
                document.absorb(event);
            }
            Ok(document.finish())
        }
        Protocol::Anthropic => {
            let mut document = anthropic::Message::default();
            for event in events {
                document.absorb(event)?;
            }
            Ok(document.finish())
        }
        Protocol::Gemini => {
            let mut document = gemini::GenerateContentResponse::default();
            for event in events {
                document.chunk(event.as_object().ok_or_else(wire)?);
            }
            Ok(document.document())
        }
        Protocol::Ollama => {
            let mut document = ollama::ChatResponse::default();
            for event in events {
                document.absorb(event.as_object().ok_or_else(wire)?);
            }
            Ok(document.finish())
        }
        Protocol::Responses => Err(wire()),
    }
}

pub(crate) fn keep(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: &serde_json::Value,
) {
    match map.get_mut(key) {
        Some(held) if !value.is_null() && held != value => *held = value.clone(),
        Some(_) => {}
        None => {
            map.insert(key.to_owned(), value.clone());
        }
    }
}

/// The value at `key`, made an empty `kind` (a list or an object) when it is
/// absent or of another type.
pub(crate) fn slot<'a, T: Default>(
    map: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    kind: fn(T) -> serde_json::Value,
) -> &'a mut serde_json::Value {
    let slot = map.entry(key).or_insert(serde_json::Value::Null);
    if std::mem::discriminant(slot) != std::mem::discriminant(&kind(T::default())) {
        *slot = kind(T::default());
    }
    slot
}

// The reassemblers use checked JSON access, without coercing provider values.
trait Fields {
    fn str(&self, key: &str) -> Option<&str>;
    fn bool(&self, key: &str) -> Option<bool>;
}

impl Fields for Value {
    fn str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }
    fn bool(&self, key: &str) -> Option<bool> {
        self.get(key)?.as_bool()
    }
}

fn value_to_json_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn is_whole_object(arguments: &str) -> bool {
    let end = arguments.trim_end();
    end.is_empty()
        || (end.ends_with('}')
            && matches!(
                serde_json::from_str::<Value>(arguments),
                Ok(Value::Object(_))
            ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChatProfile;
    use serde_json::json;

    #[test]
    fn chat_fragments_keep_reasoning_details_unknown_fields_and_tool_boundaries() {
        let frames = [
            json!({"id":"r","model":"m","object":"chat.completion.chunk","opaque":{"keep":true},"choices":[{"index":0,"delta":{"role":"assistant","content":"Hel","reasoning":"rea","reasoning_details":[{"type":"reasoning.text","index":0,"text":"a","signature":"sig"}],"tool_calls":[{"index":0,"id":"a","type":"function","function":{"name":"lookup","arguments":"{\"x\":"}}]}}]}),
            json!({"id":"r","model":"m","object":"chat.completion.chunk","opaque":null,"choices":[{"index":0,"delta":{"content":"lo","reasoning":"son","reasoning_details":[{"type":"reasoning.text","index":0,"text":"b"},{"type":"reasoning.encrypted","data":"opaque"}],"tool_calls":[{"index":0,"function":{"arguments":"1}"}}],"annotations":[{"opaque":"one"}]}}]}),
            json!({"id":"r","model":"m","object":"chat.completion.chunk","choices":[{"index":0,"finish_reason":"tool_calls","delta":{"tool_calls":[{"index":0,"id":"b","type":"function","function":{"name":"lookup","arguments":"{}"}}],"annotations":[{"opaque":"two"}]}}],"usage":{"prompt_tokens":1,"completion_tokens":2}}),
        ];
        let document = assemble(Protocol::ChatCompletions(ChatProfile::Kimi), &frames).unwrap();
        let message = &document["choices"][0]["message"];
        assert_eq!(document["opaque"], json!({"keep":true}));
        assert_eq!(message["content"], "Hello");
        assert_eq!(message["reasoning"], "reason");
        assert_eq!(
            message["reasoning_details"][0],
            json!({"type":"reasoning.text","index":0,"text":"ab","signature":"sig"})
        );
        assert_eq!(message["reasoning_details"][1]["data"], "opaque");
        assert_eq!(message["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(
            message["tool_calls"][0]["function"]["arguments"],
            "{\"x\":1}"
        );
        assert_eq!(message["tool_calls"][1]["id"], "b");
        assert!(message["tool_calls"][0].get("index").is_none());
        assert_eq!(message["annotations"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn chat_partial_json_is_retained_verbatim_without_repair() {
        let frames = [
            json!({"object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"c","function":{"name":"lookup","arguments":r#"{"cut":"#}}]},"finish_reason":"length"}]}),
        ];
        let document = assemble(Protocol::ChatCompletions(ChatProfile::OpenAi), &frames).unwrap();
        assert_eq!(
            document["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            r#"{"cut":"#
        );
    }

    #[test]
    fn gemini_joins_text_without_merging_thoughts_or_signed_parts() {
        let frame = |part: Value| json!({"candidates":[{"index":0,"content":{"role":"model","parts":[part]}}]});
        let frames = [
            frame(json!({"thoughtSignature":"lead"})),
            frame(json!({"text":"rea","thought":true,"opaque":7})),
            frame(json!({"text":"son","thought":true})),
            frame(json!({"text":"Hel"})),
            frame(json!({"text":"lo","thoughtSignature":"answer"})),
            frame(json!({"text":"next","thoughtSignature":"separate"})),
            frame(
                json!({"functionCall":{"name":"lookup","args":{"x":1}},"thoughtSignature":"tool"}),
            ),
            json!({"candidates":[{"index":0,"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":2,"thoughtsTokenCount":3}}),
        ];
        let document = assemble(Protocol::Gemini, &frames).unwrap();
        let parts = document["candidates"][0]["content"]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(
            parts[0],
            json!({"text":"reason","thought":true,"thoughtSignature":"lead","opaque":7})
        );
        assert_eq!(
            parts[1],
            json!({"text":"Hello","thoughtSignature":"answer"})
        );
        assert_eq!(parts[2]["thoughtSignature"], "separate");
        assert_eq!(parts[3]["thoughtSignature"], "tool");
        assert_eq!(document["usageMetadata"]["thoughtsTokenCount"], 3);
    }

    #[test]
    fn anthropic_sparse_indices_are_compact_and_deltas_keep_citations_and_extensions() {
        let frames = [
            json!({"type":"message_start","message":{"type":"message","role":"assistant","content":[],"usage":{"input_tokens":10,"cache_read_input_tokens":4}}}),
            json!({"type":"content_block_start","index":u64::MAX,"content_block":{"type":"text","text":"Hel","opaque":"a"}}),
            json!({"type":"content_block_delta","index":u64::MAX,"delta":{"type":"text_delta","text":"lo","opaque":"b"}}),
            json!({"type":"content_block_delta","index":u64::MAX,"delta":{"type":"citations_delta","citation":{"opaque":"citation"}}}),
            json!({"type":"content_block_stop","index":u64::MAX}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}),
        ];
        let document = assemble(Protocol::Anthropic, &frames).unwrap();
        assert_eq!(document["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            document["content"][0],
            json!({"type":"text","text":"Hello","opaque":"ab","citations":[{"opaque":"citation"}]})
        );
        assert_eq!(
            document["usage"],
            json!({"input_tokens":10,"cache_read_input_tokens":4,"output_tokens":5})
        );
    }

    #[test]
    fn anthropic_invalid_tool_json_never_becomes_an_empty_input_object() {
        for input in ["{\"cut\":", "[1]", "", "null"] {
            let frames = [
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":input}}),
                json!({"type":"content_block_stop","index":0}),
            ];
            assert!(assemble(Protocol::Anthropic, &frames).is_err(), "{input:?}");
        }
    }

    #[test]
    fn ollama_terminal_empty_fields_do_not_erase_message_fragments() {
        let frames = [
            json!({"model":"m","message":{"role":"assistant","content":"a","thinking":"r","tool_calls":[{"function":{"name":"lookup","arguments":{"x":1}}}],"opaque":{"keep":true}},"done":false}),
            json!({"model":"m","message":{"role":"assistant","content":"b","thinking":"s"},"done":false}),
            json!({"model":"m","message":{"content":"","thinking":"","opaque":null},"done":true,"prompt_eval_count":2,"eval_count":3}),
        ];
        let document = assemble(Protocol::Ollama, &frames).unwrap();
        assert_eq!(document["message"]["content"], "ab");
        assert_eq!(document["message"]["thinking"], "rs");
        assert_eq!(document["message"]["opaque"], json!({"keep":true}));
        assert_eq!(
            document["message"]["tool_calls"][0]["function"]["arguments"],
            json!({"x":1})
        );
        assert_eq!(document["eval_count"], 3);
    }

    #[test]
    fn gemini_distinct_signatures_and_empty_text_extensions_remain_separate() {
        let frames = [json!({"candidates":[{"content":{"parts":[
            {"text":"answer","thoughtSignature":"first"},
            {"thoughtSignature":"second"},
            {"text":"","future_extension":{"opaque":"keep"}}
        ]}}]})];
        let document = assemble(Protocol::Gemini, &frames).unwrap();
        let parts = document["candidates"][0]["content"]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["thoughtSignature"], "first");
        assert_eq!(parts[1]["thoughtSignature"], "second");
        assert_eq!(parts[2]["future_extension"], json!({"opaque":"keep"}));
    }
}
