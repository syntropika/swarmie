use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ToolName(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a tool name cannot be empty")]
pub struct EmptyToolName;

impl ToolName {
    pub fn new(name: impl Into<String>) -> Result<Self, EmptyToolName> {
        let name = name.into();
        if name.is_empty() {
            Err(EmptyToolName)
        } else {
            Ok(Self(name))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ToolName {
    type Error = EmptyToolName;

    fn try_from(name: String) -> Result<Self, EmptyToolName> {
        Self::new(name)
    }
}

impl TryFrom<&str> for ToolName {
    type Error = EmptyToolName;

    fn try_from(name: &str) -> Result<Self, EmptyToolName> {
        Self::new(name)
    }
}

impl From<ToolName> for String {
    fn from(name: ToolName) -> Self {
        name.0
    }
}

impl std::ops::Deref for ToolName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ToolName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for ToolName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for ToolName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ToolName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for ToolName {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl PartialEq<ToolName> for String {
    fn eq(&self, other: &ToolName) -> bool {
        *self == other.0
    }
}

impl PartialEq<ToolName> for str {
    fn eq(&self, other: &ToolName) -> bool {
        self == other.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallId {
    Provider(ProviderCallId),
    Local(LocalCallId),
}

impl CallId {
    pub fn from_wire(call_id: impl Into<String>) -> Self {
        ProviderCallId::new(call_id).map_or_else(|| Self::Local(LocalCallId::new()), Self::Provider)
    }

    pub fn provider(&self) -> Option<&ProviderCallId> {
        match self {
            Self::Provider(provider) => Some(provider),
            Self::Local(_) => None,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    pub fn wire(&self) -> Cow<'_, str> {
        match self {
            Self::Provider(provider) => Cow::Borrowed(provider.as_str()),
            Self::Local(local) => Cow::Owned(local.to_string()),
        }
    }
}

impl From<ProviderCallId> for CallId {
    fn from(provider: ProviderCallId) -> Self {
        Self::Provider(provider)
    }
}

impl From<LocalCallId> for CallId {
    fn from(local: LocalCallId) -> Self {
        Self::Local(local)
    }
}

impl std::fmt::Display for CallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.wire())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalCallId(uuid::Uuid);

impl LocalCallId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for LocalCallId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for LocalCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderCallId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a provider call id cannot be empty")]
pub struct EmptyCallId;

impl ProviderCallId {
    pub fn new(call_id: impl Into<String>) -> Option<Self> {
        let call_id = call_id.into();
        (!call_id.is_empty()).then_some(Self(call_id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProviderCallId {
    type Error = EmptyCallId;

    fn try_from(call_id: String) -> Result<Self, EmptyCallId> {
        Self::new(call_id).ok_or(EmptyCallId)
    }
}

impl From<ProviderCallId> for String {
    fn from(id: ProviderCallId) -> Self {
        id.0
    }
}
