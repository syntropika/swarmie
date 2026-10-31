use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityEntry {
    pub id: String,
    pub owner: String,
    pub entity: String,
    pub title: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub unread: bool,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityInput {
    /// Publishing capability identity for grouping. This value does not grant authority.
    pub owner: String,
    pub entity: String,
    pub title: String,
    pub text: String,
    pub conversation_id: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub revision: u64,
    pub items: Vec<ActivityEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Publish { input: ActivityInput },
    Read { id: String },
}

pub(crate) fn invalid(message: &str) -> tembra::Error {
    tembra::Error::invalid("swarmie.activity.input", message)
}

impl Snapshot {
    pub fn validate(&self) -> tembra::Result<()> {
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.items {
            let id = serde_json::to_string(&(&item.owner, &item.entity))
                .map_err(|_| invalid("invalid activity entry identity"))?;
            if item.owner.trim().is_empty()
                || item.entity.trim().is_empty()
                || item.title.trim().is_empty()
                || item.id != id
                || !ids.insert(&item.id)
            {
                return Err(invalid("invalid or duplicate activity entry"));
            }
        }
        Ok(())
    }

    pub fn apply(mut self, command: Command, now: u64) -> tembra::Result<Self> {
        match command {
            Command::Publish { input } => {
                let id = serde_json::to_string(&(&input.owner, &input.entity))
                    .map_err(|_| invalid("invalid activity entry identity"))?;
                self.items.retain(|entry| entry.id != id);
                self.items.insert(
                    0,
                    ActivityEntry {
                        id,
                        owner: input.owner,
                        entity: input.entity,
                        title: input.title,
                        text: input.text,
                        conversation_id: input.conversation_id,
                        unread: true,
                        updated_at: now,
                    },
                );
            }
            Command::Read { id } => {
                let item = self
                    .items
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| invalid("activity entry is unavailable"))?;
                item.unread = false;
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("activity entry revision exhausted"))?;
        self.validate()?;
        Ok(self)
    }
}
