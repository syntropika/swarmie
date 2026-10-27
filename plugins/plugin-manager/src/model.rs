use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroU32, str::FromStr};

/// A stable plugin identity, distinct from Tembra's host-local orb handles.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PluginId(String);

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("plugin IDs must contain 1..=255 ASCII letters, digits, dots, underscores or hyphens")]
pub struct PluginIdError;

impl PluginId {
    /// Validates a stable identity without resolving or authorizing a plugin.
    pub fn new(value: impl Into<String>) -> Result<Self, PluginIdError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 255
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err(PluginIdError);
        }
        Ok(Self(value))
    }

    /// Returns the validated plugin identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PluginId {
    type Error = PluginIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PluginId> for String {
    fn from(value: PluginId) -> Self {
        value.0
    }
}

impl FromStr for PluginId {
    type Err = PluginIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Desired durable state. Observed mount failures never overwrite this record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub id: PluginId,
    pub version: NonZeroU32,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OrbInfo {
    pub name: String,
    pub api_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MountFailure {
    DefinitionMissing,
    VersionMismatch { available: NonZeroU32 },
    Factory { message: String },
    Installation { message: String },
    EmptyPlugin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockReason {
    Dependencies { plugins: Vec<PluginId> },
}

/// Actual orb installation in this host, separate from desired enabled state.
///
/// Mounted means declarations are installed. Tembra activates service objects on
/// acquisition; it owns their later resource failures and invocation lifecycle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginStatus {
    Disabled,
    Mounted { orbs: Vec<OrbInfo> },
    Failed { failure: MountFailure },
    Blocked { reason: BlockReason },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginView {
    pub id: PluginId,
    /// Absent if another writer removed a record whose native orbs remain live.
    pub desired: Option<InstalledPlugin>,
    pub observed: PluginStatus,
    /// Native removal and replacement take effect at the next host start.
    pub restart_required: bool,
}

/// An owned inspection snapshot. Mutating it cannot change the manager.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginSnapshot {
    pub inventory_revision: u64,
    pub plugins: Vec<PluginView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_identifiers_and_zero_versions_are_refused_at_deserialization() {
        for id in ["", "with spaces", "../outside", "unicode-ñ"] {
            assert!(PluginId::new(id).is_err());
            assert!(
                serde_json::from_value::<PluginId>(serde_json::Value::String(id.into())).is_err()
            );
        }
        assert!(PluginId::new("a".repeat(256)).is_err());
        let record = InstalledPlugin {
            id: PluginId::new("core.counter").unwrap(),
            version: NonZeroU32::new(1).unwrap(),
            enabled: true,
        };
        let mut encoded = serde_json::to_value(&record).unwrap();
        assert_eq!(
            serde_json::from_value::<InstalledPlugin>(encoded.clone()).unwrap(),
            record
        );
        encoded["version"] = 0.into();
        assert!(serde_json::from_value::<InstalledPlugin>(encoded).is_err());
    }
}
