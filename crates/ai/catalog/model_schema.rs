use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Published model facts. Missing fields remain unknown. These facts do not
/// qualify an adapter or grant account access.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_model_id: Option<String>,
    #[serde(default)]
    pub limit: ModelLimits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Modalities>,
    #[serde(default, rename = "tool_call", skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_options: Option<Vec<ReasoningOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interleaved: Option<Interleaved>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Pricing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<Compatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_weights: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Extra source fields, including provider overrides and service modes.
    /// They are descriptive and never applied to request credentials or routing.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// An absent list is unknown. An explicit empty list reports no modalities.
/// Unknown modality names are retained as exact strings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modalities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningOption {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Interleaved {
    Supported(bool),
    Field {
        field: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
}

/// List prices in USD per million tokens. Missing prices are not zero.
/// Additional pricing tiers and media rates remain in extra.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pricing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_audio: Option<f64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Explicit reviewed wire facts. Values do not enable protocol features by
/// themselves. No model-name or provider-family inference is performed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_control: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_retention: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_off: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_conversation_system: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejects_forced_tool_choice: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binds_context: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_options: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_tools_need_reasoning_off: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Provider {
    pub models: BTreeMap<String, ModelSpec>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub(crate) fn identifier(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}

pub(crate) fn providers(raw: &[u8]) -> Result<BTreeMap<String, Provider>, String> {
    let value = unique_json(raw)?;
    let providers: BTreeMap<String, Provider> =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    for (provider_id, provider) in &providers {
        if !identifier(provider_id) {
            return Err("invalid provider identity".into());
        }
        for (model_id, model) in &provider.models {
            if !identifier(model_id) || model.id != *model_id {
                return Err("invalid or ambiguous model identity".into());
            }
            if let Some(cost) = &model.cost {
                let rates = [
                    cost.input,
                    cost.output,
                    cost.cache_read,
                    cost.cache_write,
                    cost.reasoning,
                    cost.input_audio,
                    cost.output_audio,
                ];
                if rates
                    .into_iter()
                    .flatten()
                    .any(|rate| !rate.is_finite() || rate < 0.0)
                {
                    return Err("invalid model list price".into());
                }
            }
            if model.reasoning_options.as_ref().is_some_and(|options| {
                options.iter().any(|option| {
                    !identifier(&option.kind)
                        || option
                            .min
                            .zip(option.max)
                            .is_some_and(|(min, max)| min > max)
                })
            }) {
                return Err("invalid reasoning options".into());
            }
        }
    }
    Ok(providers)
}

/// Reject duplicate keys at every depth before serde can discard a value.
pub(crate) fn unique_json(raw: &[u8]) -> Result<Value, String> {
    if raw.len() > 16 * 1024 * 1024 {
        return Err("catalog exceeds the input limit".into());
    }
    struct Unique;
    impl<'de> Deserialize<'de> for Unique {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Unique;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("JSON with unique object keys")
                }
                fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_string<E: serde::de::Error>(self, _: String) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_none<E: serde::de::Error>(self) -> Result<Unique, E> {
                    Ok(Unique)
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> Result<Unique, A::Error> {
                    while seq.next_element::<Unique>()?.is_some() {}
                    Ok(Unique)
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> Result<Unique, A::Error> {
                    let mut keys = std::collections::BTreeSet::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if !keys.insert(key) {
                            return Err(serde::de::Error::custom("duplicate catalog key"));
                        }
                        map.next_value::<Unique>()?;
                    }
                    Ok(Unique)
                }
            }
            deserializer.deserialize_any(Visitor)
        }
    }
    // Validate structure without interpreting serde_json's private number-map
    // representation. Its own Value decoder then retains exact numeric values.
    serde_json::from_slice::<Unique>(raw).map_err(|error| error.to_string())?;
    serde_json::from_slice(raw).map_err(|error| error.to_string())
}

/// Stable JSON content identity across serde_json feature combinations.
pub(crate) fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    value.sort_all_objects();
    serde_json::to_vec(&value).map_err(|error| error.to_string())
}
