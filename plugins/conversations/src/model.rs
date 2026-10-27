use crate::{Rejection, StorageError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::num::NonZeroU16;
use uuid::Uuid;

macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

identity!(ConversationId);
identity!(EntryId);

/// A nonempty prefix visible in this conversation, including inherited entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPoint {
    pub conversation: ConversationId,
    pub entry: EntryId,
}

/// A selected prefix, or the empty prefix of a root conversation.
/// For append, this is the expected current tail, not permission to overwrite history.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub conversation: ConversationId,
    pub entry: Option<EntryId>,
}

impl From<HistoryPoint> for Head {
    fn from(point: HistoryPoint) -> Self {
        Self {
            conversation: point.conversation,
            entry: Some(point.entry),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversation {
    pub head: Head,
    pub origin: Option<HistoryPoint>,
}

/// Caller-defined typed values are encoded at this boundary. History does not
/// interpret agent messages, tools, context projections or execution phases.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryDraft {
    pub id: EntryId,
    pub data: Value,
}

impl EntryDraft {
    pub fn encode<T: Serialize>(value: T) -> Result<Self, StorageError> {
        Ok(Self {
            id: EntryId::new(),
            data: serde_json::to_value(value).map_err(|e| StorageError::Invalid(e.to_string()))?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: EntryId,
    /// The conversation that originally appended this immutable entry.
    pub conversation: ConversationId,
    pub data: Value,
}

impl Entry {
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, StorageError> {
        serde_json::from_value(self.data.clone()).map_err(|e| StorageError::Invalid(e.to_string()))
    }
}

/// All pages use the same fixed head. A cursor from another branch is refused.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub at: Head,
    pub after: Option<EntryId>,
    pub limit: NonZeroU16,
}

impl Read {
    pub const MAX_LIMIT: NonZeroU16 = NonZeroU16::new(256).unwrap();

    pub fn new(at: Head) -> Self {
        Self {
            at,
            after: None,
            limit: NonZeroU16::new(64).unwrap(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Last returned entry when another page exists; reuse it with the same head.
    pub next: Option<EntryId>,
}

/// Storage applies these operations in a single authority transaction. A fork's
/// origin is immutable, and append must compare the complete expected head.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Write {
    Create {
        id: ConversationId,
        origin: Option<HistoryPoint>,
    },
    Append {
        at: Head,
        entries: Vec<EntryDraft>,
    },
}

impl Write {
    pub fn head(&self) -> Result<Head, StorageError> {
        match self {
            Self::Create { id, origin } => Ok(Head {
                conversation: *id,
                entry: origin.map(|point| point.entry),
            }),
            Self::Append { at, entries } => Ok(Head {
                conversation: at.conversation,
                entry: Some(
                    entries
                        .last()
                        .ok_or(StorageError::Refused(Rejection::Invalid))?
                        .id,
                ),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteConfirmation {
    /// The enclosing shared commit has not confirmed this proposed head yet.
    Staged,
    Committed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteReceipt {
    pub head: Head,
    pub confirmation: WriteConfirmation,
}
