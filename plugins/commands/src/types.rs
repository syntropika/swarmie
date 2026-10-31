use crate::invalid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use swarmie_providers::OperationId;

/// Stable semantic identity, independent of a slash alias or translated label.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommandId(String);

impl CommandId {
    pub fn new(value: impl Into<String>) -> tembra::Result<Self> {
        let value = value.into();
        if value.len() > 255
            || value.split('.').any(|segment| {
                !segment
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_lowercase)
                    || !segment.bytes().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-')
                    })
            })
        {
            return Err(invalid(
                "command identity requires lowercase nonempty name segments",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CommandId {
    type Error = tembra::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CommandId> for String {
    fn from(value: CommandId) -> Self {
        value.0
    }
}

/// Ordinary Tembra address. Neither metadata nor invocation arguments confer authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub service: String,
    pub key: Vec<String>,
    pub method: String,
}

impl Target {
    pub fn validate(&self) -> tembra::Result<()> {
        if self.key.is_empty()
            || self
                .key
                .iter()
                .chain([&self.service, &self.method])
                .any(|v| v.is_empty() || v.len() > 255 || v.chars().any(char::is_control))
        {
            return Err(invalid(
                "command target requires a service, key and method without controls",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: CommandId,
    pub label: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub target: Target,
}

impl Command {
    pub fn validate(&self) -> tembra::Result<()> {
        self.target.validate()?;
        if self.label.trim().is_empty() || self.description.trim().is_empty() {
            return Err(invalid("command label and description must not be empty"));
        }
        let mut aliases = std::collections::BTreeSet::new();
        for alias in &self.aliases {
            let Some(name) = alias.strip_prefix('/') else {
                return Err(invalid("slash alias must start with a slash"));
            };
            if name.is_empty()
                || name.len() > 255
                || !name.bytes().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-')
                })
                || !aliases.insert(alias)
            {
                return Err(invalid("slash aliases must be distinct lowercase names"));
            }
        }
        Ok(())
    }
}

/// The handler owns admission, coalescing and durable recognition of this identity.
/// The dispatcher does not retry a handler after a lost reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub command: CommandId,
    pub operation: OperationId,
    pub arguments: Value,
}
