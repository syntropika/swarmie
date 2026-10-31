use serde::{Deserialize, Serialize};
use swarmie_conversations::{EntryId, Head, HistoryPoint};

/// Bounded lookup of one immutable result independently of a paused turn's head.
/// Storage must verify that `after` is the actual append parent and return exactly
/// the selected entry, with no continuation cursor. This value grants no authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Locator", into = "Locator")]
pub struct ArtifactRef {
    at: HistoryPoint,
    after: Option<EntryId>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    at: Head,
    after: Option<EntryId>,
}

impl ArtifactRef {
    pub fn new(at: Head, after: Option<EntryId>) -> tembra::Result<Self> {
        Locator { at, after }.try_into()
    }

    pub fn at(self) -> Head {
        self.at.into()
    }

    pub fn entry(self) -> EntryId {
        self.at.entry
    }

    pub fn after(self) -> Option<EntryId> {
        self.after
    }
}

impl TryFrom<Locator> for ArtifactRef {
    type Error = tembra::Error;

    fn try_from(value: Locator) -> Result<Self, Self::Error> {
        let entry = value
            .at
            .entry
            .ok_or_else(|| crate::invalid("artifact locator has no entry"))?;
        if value.after == Some(entry) {
            return Err(crate::invalid("artifact locator cannot follow itself"));
        }
        Ok(Self {
            at: HistoryPoint {
                conversation: value.at.conversation,
                entry,
            },
            after: value.after,
        })
    }
}

impl From<ArtifactRef> for Locator {
    fn from(value: ArtifactRef) -> Self {
        Self {
            at: value.at(),
            after: value.after,
        }
    }
}
