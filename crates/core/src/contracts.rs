use crate::{Capabilities, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAvailability {
    Available,
    Unavailable,
    Unsupported,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    Harness,
    Runtime,
    Extension,
}
/// Evidence describes the declaration, not certification of live model execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEvidence {
    Declared,
    Negotiated,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub availability: CapabilityAvailability,
    pub source: CapabilitySource,
    pub evidence: CapabilityEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}
impl Default for Capability {
    fn default() -> Self {
        false.into()
    }
}
impl From<bool> for Capability {
    fn from(supported: bool) -> Self {
        Self {
            availability: if supported {
                CapabilityAvailability::Available
            } else {
                CapabilityAvailability::Unsupported
            },
            source: CapabilitySource::Harness,
            evidence: CapabilityEvidence::Declared,
            reason: (!supported).then(|| "The adapter does not support this operation".into()),
            source_id: None,
        }
    }
}
impl Capability {
    pub fn is_available(&self) -> bool {
        self.availability == CapabilityAvailability::Available
    }
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            availability: CapabilityAvailability::Unknown,
            reason: Some(reason.into()),
            ..Self::default()
        }
    }
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            availability: CapabilityAvailability::Unavailable,
            reason: Some(reason.into()),
            ..Self::default()
        }
    }
    pub fn negotiated(supported: bool) -> Self {
        Self {
            evidence: CapabilityEvidence::Negotiated,
            reason: (!supported)
                .then(|| "The native session did not advertise this capability".into()),
            ..supported.into()
        }
    }
    pub fn require(&self, operation: &str) -> Result<()> {
        if self.is_available() {
            Ok(())
        } else {
            Err(Error::Unsupported(format!(
                "{operation}: {}",
                self.reason.as_deref().unwrap_or("not available")
            )))
        }
    }
}

/// Application scope is independent of whether an execution is currently busy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingApplication {
    CreationOnly,
    NextTurn,
    CurrentTurn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingType {
    Select,
    Boolean,
    String,
    Number,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SettingChoice {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingDescriptor {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SettingType,
    /// Absent means unobserved; a present null is an acknowledged reset.
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<SettingChoice>,
    pub application: SettingApplication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}
fn present_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
impl SettingDescriptor {
    pub fn validate(&self, value: &Value) -> Result<()> {
        let valid = match self.kind {
            SettingType::Select => self
                .choices
                .iter()
                .any(|choice| value.as_str() == Some(choice.id.as_str())),
            SettingType::Boolean => value.is_boolean(),
            SettingType::String => value.is_string(),
            SettingType::Number => value.is_number(),
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Invalid(format!("invalid value for {}", self.id)))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelDescriptor {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationReceipt {
    pub values: BTreeMap<String, Value>,
    pub execution_policy: crate::ExecutionPolicy,
    pub configuration: Vec<SettingDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationPreview {
    pub capabilities: Capabilities,
    pub execution_policy: crate::ExecutionPolicy,
    /// None means unavailable; an empty array is an available, empty catalog.
    pub configuration: Option<Vec<SettingDescriptor>>,
    pub models: Option<Vec<ModelDescriptor>>,
}
