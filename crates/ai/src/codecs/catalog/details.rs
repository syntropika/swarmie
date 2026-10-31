use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::spec::Modalities;

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelDetails {
    pub description: Option<String>,
    pub family: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub canonical_model_id: Option<String>,
    pub attachment: Option<bool>,
    pub interleaved: Option<Interleaved>,
    pub output: Option<Modalities>,
    pub knowledge: Option<String>,
    pub release_date: Option<String>,
    pub last_updated: Option<String>,
    pub open_weights: Option<bool>,
    pub status: Option<String>,
    pub cost: CostDetails,
    pub provider: Option<ProviderOverride>,
    pub modes: BTreeMap<String, Mode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Interleaved {
    Supported(bool),
    Field { field: String },
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CostDetails {
    pub reasoning: Option<f64>,
    pub input_audio: Option<f64>,
    pub output_audio: Option<f64>,
    pub tiers: Vec<CostTier>,
    pub context_over_200k: Option<Rates>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rates {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_audio: Option<f64>,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostTier {
    #[serde(flatten)]
    pub rates: Rates,
    pub tier: Tier,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tier {
    #[serde(rename = "type")]
    pub kind: String,
    pub size: u64,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<Rates>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderOverride>,
}
