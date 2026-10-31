use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Api(Cow<'static, str>);

impl Api {
    pub const fn from_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for Api {
    fn from(name: &'static str) -> Self {
        Self::from_static(name)
    }
}

impl From<String> for Api {
    fn from(name: String) -> Self {
        Self(Cow::Owned(name))
    }
}

impl std::fmt::Display for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub api: Api,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Fingerprint>,
}

impl Origin {
    pub fn new(api: impl Into<Api>, provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api: api.into(),
            provider: provider.into(),
            model: model.into(),
            response_model: None,
            response_id: None,
            context: None,
        }
    }

    pub fn same_model(&self, api: &Api, provider: &str, model: &str) -> bool {
        &self.api == api && self.provider == provider && self.model == model
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error(String),
    Aborted(String),
}

impl StopReason {
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Error(_) | Self::Aborted(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(u64);

impl Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{:016x}", self.0))
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = std::borrow::Cow::<'de, str>::deserialize(deserializer)?;
        u64::from_str_radix(&text, 16)
            .map(Self)
            .map_err(|_| serde::de::Error::custom("a fingerprint is 16 hex digits"))
    }
}

impl Fingerprint {
    pub fn of(value: &impl Serialize) -> Self {
        let mut hash = Fnv(0xcbf2_9ce4_8422_2325);
        // Message types always serialize; a failure would only leave a
        // fingerprint no native item matches, which replays canonically.
        if let Ok(value) = serde_json::to_value(value) {
            let _ = serde_json::to_writer(&mut hash, &Canonical(&value));
        }
        Self(hash.0)
    }

    pub(crate) fn of_canonical(value: &impl Serialize) -> Self {
        let mut hash = Fnv(0xcbf2_9ce4_8422_2325);
        let _ = serde_json::to_writer(&mut hash, value);
        Self(hash.0)
    }
}

struct Fnv(u64);

impl std::io::Write for Fnv {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Canonical<'a, T: ?Sized>(pub(crate) &'a T);

impl Serialize for Canonical<'_, serde_json::Map<String, serde_json::Value>> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields: Vec<_> = self.0.iter().collect();
        fields.sort_by_key(|(left, _)| *left);
        serializer.collect_map(
            fields
                .into_iter()
                .map(|(key, value)| (key, Canonical(value))),
        )
    }
}

impl Serialize for Canonical<'_, serde_json::Value> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            serde_json::Value::Object(fields) => Canonical(fields).serialize(serializer),
            serde_json::Value::Array(values) => {
                serializer.collect_seq(values.iter().map(Canonical))
            }
            serde_json::Value::Number(number) => {
                if let Some(float) = number
                    .as_f64()
                    .filter(|value| number.is_f64() && value.fract() == 0.0)
                {
                    if float == 0.0 && float.is_sign_negative() {
                        return serializer.serialize_f64(float);
                    }
                    let whole = format!("{float:.0}");
                    if let Ok(value) = whole.parse::<i64>() {
                        return serializer.serialize_i64(value);
                    }
                    if let Ok(value) = whole.parse::<u64>() {
                        return serializer.serialize_u64(value);
                    }
                    return serializer.serialize_f64(float);
                }
                number.serialize(serializer)
            }
            value => value.serialize(serializer),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Native {
    pub item: serde_json::Value,
    pub fingerprint: Fingerprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "crate::codecs::json_utils::is_false")]
    pub edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_slot: Option<Cow<'static, str>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blob<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

impl Native {
    pub fn reasoning_blob(&self) -> Option<Blob<'_>> {
        let key = self.blob_slot.as_deref()?;
        let value = self.item.pointer(key)?.as_str()?;
        (!value.is_empty()).then_some(Blob { key, value })
    }

    pub(crate) fn find_blob(&mut self, slots: &[&'static str]) {
        let holds = |slot: &&str| {
            self.item
                .pointer(slot)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
        };
        self.blob_slot = slots.iter().copied().find(holds).map(Cow::Borrowed);
    }
}

pub(super) fn baseline(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Number(number) if !number.is_f64() => Value::String(format!("i:{number}")),
        Value::String(text) => Value::String(format!("s:{text}")),
        Value::Array(values) => Value::Array(values.into_iter().map(baseline).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, baseline(value)))
                .collect(),
        ),
        value => value,
    }
}

pub(super) fn matches_baseline(saved: &serde_json::Value, current: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (saved, current) {
        (Value::String(saved), Value::Number(number)) => {
            let Some(integer) = saved.strip_prefix("i:") else {
                return false;
            };
            if number.is_f64() {
                number
                    .as_f64()
                    .filter(|value| value.fract() == 0.0)
                    .is_some_and(|value| format!("{value:.0}") == integer)
            } else {
                number.to_string() == integer
            }
        }
        (Value::String(saved), Value::String(current)) => {
            saved.strip_prefix("s:") == Some(current.as_str())
        }
        (Value::Array(saved), Value::Array(current)) => {
            saved.len() == current.len()
                && saved
                    .iter()
                    .zip(current)
                    .all(|(saved, current)| matches_baseline(saved, current))
        }
        (Value::Object(saved), Value::Object(current)) => {
            saved.len() == current.len()
                && saved.iter().all(|(key, saved)| {
                    current
                        .get(key)
                        .is_some_and(|current| matches_baseline(saved, current))
                })
        }
        (Value::Number(_), Value::Number(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Null, Value::Null) => saved == current,
        _ => false,
    }
}

pub(crate) fn lenient<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Native>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| Native::deserialize(value).ok()))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opaque {
    pub item: serde_json::Value,
    pub replay: bool,
}

impl Opaque {
    pub fn kind(&self) -> Option<&str> {
        self.item.get("type").and_then(serde_json::Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::codecs::message::{
        AssistantContent, AssistantMessage, CallId, Fingerprint, Message, Opaque, Origin,
        StopReason, Text, ToolCall, ToolFunction, ToolName,
    };

    fn call(arguments: serde_json::Value) -> AssistantContent {
        let name = ToolName::new("lookup").expect("a tool name");
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_1"),
            ToolFunction::new(name, arguments),
        ))
    }

    #[test]
    fn fingerprints_survive_a_serde_round_trip() {
        // Object order is canonicalized before hashing.
        let arguments = json!({"zeta": 1, "alpha": {"y": 0.1, "x": [2.718_281_828_459_1, 1e-7]}});
        let block = call(arguments).with_native(json!({"type": "function_call", "id": "fc_1"}));
        let saved = serde_json::to_string(&block).expect("serialize");
        let loaded: AssistantContent = serde_json::from_str(&saved).expect("deserialize");
        assert_eq!(loaded.fingerprint(), block.fingerprint());
        assert!(loaded.native_item().is_some());
    }

    #[test]
    fn native_calls_survive_repeated_float_storage() {
        let floats = [
            0.1,
            std::f64::consts::E,
            51.248_178_375_505_404,
            -93.311_370_376_880_33,
            2.003_039_774_426_776_2e-253,
            7.101_215_824_554_616e260,
            1.769_268_377_902_049e74,
            3.928_753_217_337_331_5e299,
            1e20,
            -0.0,
        ];
        for float in floats {
            let mut block = call(json!({"nested": [{"value": float}]}))
                .with_native(json!({"type": "function_call", "id": "fc_1", "signature": "keep"}));
            let fingerprint = block.fingerprint();
            for cycle in 0..9 {
                block = match cycle % 3 {
                    0 => serde_json::from_str(&serde_json::to_string(&block).expect("serialize"))
                        .expect("load JSON"),
                    1 => serde_json::from_value(serde_json::to_value(&block).expect("to value"))
                        .expect("load value"),
                    _ => {
                        let value = serde_json::to_value(&block).expect("to value");
                        serde_json::from_str(&value.to_string()).expect("load stored value")
                    }
                };
                assert_eq!(
                    block.native_item().and_then(|item| item.get("signature")),
                    Some(&json!("keep")),
                    "{float} after storage cycle {cycle}"
                );
                let AssistantContent::ToolCall(tool) = &block else {
                    panic!("a call stays a call");
                };
                assert_eq!(
                    tool.native.as_ref().map(|native| native.fingerprint),
                    Some(fingerprint)
                );
            }
        }
    }

    #[test]
    fn adjacent_float_edits_stay_stale_before_and_after_storage() {
        for original in [
            0.100_000_000_000_000_03_f64,
            f64::from_bits(0x7617_17c1_3ccb_d15f),
            9.999_999_999_999_84e19,
        ] {
            let edited = f64::from_bits(original.to_bits() + 1);
            let mut block = call(json!({"value": original}))
                .with_native(json!({"type": "function_call", "id": "fc_1"}));
            if let AssistantContent::ToolCall(call) = &mut block {
                call.function
                    .arguments
                    .insert("value".into(), json!(edited));
            }
            assert!(block.native_item().is_none());
            for _ in 0..4 {
                let saved = serde_json::to_string(&block).expect("serialize edited call");
                block = serde_json::from_str(&saved).expect("load edited call");
                assert!(block.native_item().is_none(), "{saved}");
            }
        }
        let original = 0.100_000_000_000_000_03_f64;

        let mut loaded: AssistantContent = serde_json::from_str(
            &serde_json::to_string(
                &call(json!({"value": original})).with_native(json!({"id": "fc_1"})),
            )
            .expect("serialize"),
        )
        .expect("load");
        if let AssistantContent::ToolCall(call) = &mut loaded {
            let current = call
                .function
                .arguments
                .get("value")
                .and_then(serde_json::Value::as_f64)
                .expect("a float");
            call.function
                .arguments
                .insert("value".into(), json!(f64::from_bits(current.to_bits() + 1)));
        }
        assert!(loaded.native_item().is_none());
    }

    #[test]
    fn float_baselines_detect_integer_precision_lost_by_a_store() {
        fn through_f64(value: &mut serde_json::Value) {
            use serde_json::Value;
            match value {
                Value::Number(number) => {
                    if let Some(float) = number.as_f64().and_then(serde_json::Number::from_f64) {
                        *number = float;
                    }
                }
                Value::Array(values) => values.iter_mut().for_each(through_f64),
                Value::Object(fields) => fields.values_mut().for_each(through_f64),
                _ => {}
            }
        }
        for (big, current) in [(12_345_678_901_234_567_u64, false), (20, true)] {
            let block =
                call(json!({"big": big, "scale": 0.5, "nested": [{"a/b~c": big}], "text": "i:20"}))
                    .with_native(json!({"arguments": format!(r#"{{"big":{big},"scale":0.5}}"#)}));
            let mut stored = serde_json::to_value(&block).expect("save call");
            through_f64(&mut stored);
            let loaded: AssistantContent = serde_json::from_value(stored).expect("load call");
            assert_eq!(loaded.native_item().is_some(), current, "integer {big}");
        }
    }

    #[test]
    fn legacy_native_records_keep_exact_fingerprint_validation() {
        let block = call(json!({"limit": 20})).with_native(json!({"id": "fc_1"}));
        let mut stored = serde_json::to_value(&block).expect("serialize");
        if let Some(native) = stored
            .get_mut("native")
            .and_then(serde_json::Value::as_object_mut)
        {
            native.retain(|key, _| key != "baseline");
        }
        let mut loaded: AssistantContent = serde_json::from_value(stored).expect("legacy load");
        assert!(loaded.native_item().is_some());
        if let AssistantContent::ToolCall(call) = &mut loaded {
            call.function.arguments.insert("limit".into(), json!(21));
        }
        assert!(loaded.native_item().is_none());
    }

    #[test]
    fn opaque_items_carry_no_separate_native() {
        let opaque = AssistantContent::Opaque(Opaque {
            item: json!({"type": "web_search_call", "id": "ws_1"}),
            replay: true,
        });
        assert_eq!(opaque.clone().with_native(json!({"ignored": true})), opaque);
        assert_eq!(opaque.native_item(), None);
    }

    #[test]
    fn an_assistant_message_serializes_flat_under_its_role() {
        let turn = AssistantMessage {
            content: vec![AssistantContent::Text(Text::new("hi"))],
            origin: Some(Origin::new("anthropic.messages", "anthropic", "claude")),
            stop: Some(StopReason::Error("refused".into())),
        };
        let value = serde_json::to_value(Message::Assistant(turn.clone())).expect("serialize");
        assert_eq!(
            value,
            json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "hi"}],
                "origin": {"api": "anthropic.messages", "provider": "anthropic", "model": "claude"},
                "stop": {"error": "refused"},
            })
        );
        assert_eq!(
            serde_json::from_value::<Message>(value).expect("deserialize"),
            Message::Assistant(turn)
        );
    }

    #[test]
    fn a_store_that_writes_whole_numbers_as_integers_keeps_the_item_current() {
        let block = call(json!({"limit": 20.0, "scale": 0.5}))
            .with_native(json!({"type": "function_call", "id": "fc_1"}));
        let stored = serde_json::to_string(&block)
            .expect("a block serializes")
            .replace("20.0", "20");
        let loaded: AssistantContent = serde_json::from_str(&stored).expect("a stored block loads");
        assert!(loaded.native_item().is_some(), "{stored}");
        assert_ne!(
            Fingerprint::of(&json!({"scale": 0.5})),
            Fingerprint::of(&json!({"scale": 1}))
        );
    }

    #[test]
    fn a_stored_item_that_cannot_be_read_loads_as_no_item() {
        for fingerprint in [json!(1234), json!("not hex"), json!(null)] {
            let block: AssistantContent = serde_json::from_value(json!({
                "type": "text",
                "text": "hi",
                "native": {"item": {"id": "msg_1"}, "fingerprint": fingerprint}
            }))
            .expect("the block loads");
            assert_eq!(block, AssistantContent::text("hi"), "{fingerprint}");
        }
        let current = AssistantContent::text("hi").with_native(json!({"id": "msg_1"}));
        let stored = serde_json::to_value(&current).expect("serializes");
        assert_eq!(
            serde_json::from_value::<AssistantContent>(stored).expect("loads"),
            current
        );
    }

    #[test]
    fn a_reasoning_blob_is_read_where_its_wire_put_it() {
        use crate::codecs::message::{Blob, Native};
        let slots = ["/signature", "/data"];
        let mut redacted = Native {
            item: json!({"type": "redacted_thinking", "data": "r"}),
            fingerprint: Fingerprint::of(&"x"),
            baseline: None,
            edited: false,
            blob_slot: None,
        };
        redacted.find_blob(&slots);
        let stored = serde_json::to_value(&redacted).expect("serializes");
        let stored: Native = serde_json::from_value(stored).expect("loads");
        let found = stored.reasoning_blob();
        let mut unsigned = Native {
            item: json!({"type": "thinking", "thinking": "t", "signature": ""}),
            ..redacted.clone()
        };
        unsigned.find_blob(&slots);
        assert_eq!(
            found,
            Some(Blob {
                key: "/data",
                value: "r"
            })
        );
        assert_eq!(
            (&unsigned.blob_slot, unsigned.reasoning_blob()),
            (&None, None)
        );
    }

    #[test]
    fn fingerprints_are_pinned() {
        use crate::codecs::completion::ToolDefinition;
        use crate::codecs::message::{
            DocumentSourceKind, Image, ImageDetail, ImageMediaType, Reasoning, ToolCall,
            ToolFunction,
        };
        let tool = |name: &str, parameters| {
            ToolDefinition::new(
                ToolName::new(name).expect("a tool name"),
                format!("{name} things"),
                parameters,
            )
        };
        let tools = [
            tool(
                "search",
                json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            ),
            tool(
                "add",
                json!({"properties": {"b": {"maximum": 1.0}}, "type": "object"}),
            ),
        ];
        let arguments = json!({"b": {"z": 1, "a": 20.0}, "a": [1.5, 2.0, -0.0, 1e20]});
        let local = AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire(""),
            ToolFunction::new(ToolName::new("lookup").expect("a tool name"), json!({})),
        ));
        let image = AssistantContent::Image(Image {
            data: DocumentSourceKind::base64("aGk="),
            media_type: Some(ImageMediaType::PNG),
            detail: Some(ImageDetail::High),
            native: None,
        });
        let opaque = AssistantContent::Opaque(Opaque {
            item: json!({"type": "web_search_call"}),
            replay: true,
        });
        let reasoning = AssistantContent::Reasoning(Reasoning {
            redacted: true,
            ..Reasoning::new("plan")
        });
        let hex = |fingerprint: Fingerprint| format!("{:016x}", fingerprint.0);
        let pinned = [
            hex(AssistantContent::text("hello").fingerprint()),
            hex(reasoning.fingerprint()),
            hex(call(arguments).fingerprint()),
            hex(local.fingerprint()),
            hex(image.fingerprint()),
            hex(opaque.fingerprint()),
            hex(Fingerprint::of(&json!(tools))),
        ];
        assert_eq!(
            pinned,
            [
                "be5cdb8169d835fd", // text
                "c2dc7b0f4e836ea1", // reasoning
                "3df7dcbe7a89ecc8", // call with a provider id
                "7e2d775ebdb398fc", // call with a local id
                "37e9c9a0147c5070", // image
                "9604486c7b29d4c5", // opaque
                "2fc10d4b16d1b11c", // tools, as a request's context hashes them
            ]
        );
    }
}
