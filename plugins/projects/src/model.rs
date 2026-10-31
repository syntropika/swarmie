use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub archived: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalFolders {
    pub project_id: String,
    pub node_id: String,
    pub paths: Vec<String>,
}

/// Membership references do not own conversation data or grant filesystem access.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub revision: u64,
    pub records: Vec<Project>,
    pub links: BTreeMap<String, String>,
    pub personal_folders: Vec<PersonalFolders>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Create {
        id: String,
        name: String,
        description: String,
    },
    Update {
        id: String,
        name: String,
        description: String,
    },
    Archive {
        id: String,
        archived: bool,
    },
    Assign {
        conversation: String,
        project: String,
    },
    Unassign {
        conversation: String,
    },
    Folders {
        project: String,
        node: String,
        paths: Vec<String>,
    },
    /// Import into an empty native collection only. Source data remains owned by its exporter.
    Import {
        data: Snapshot,
    },
}

pub(crate) fn invalid(message: &str) -> tembra::Error {
    tembra::Error::invalid("swarmie.projects.input", message)
}

fn identity(value: &str) -> tembra::Result<()> {
    if value.trim().is_empty() {
        return Err(invalid("identity must not be empty"));
    }
    Ok(())
}

impl Snapshot {
    pub fn validate(&self) -> tembra::Result<()> {
        let mut ids = BTreeSet::new();
        for record in &self.records {
            identity(&record.id)?;
            if record.name.trim().is_empty() || !ids.insert(&record.id) {
                return Err(invalid(
                    "project names must be present and identities unique",
                ));
            }
        }
        for (conversation, project) in &self.links {
            identity(conversation)?;
            if !ids.contains(project) {
                return Err(invalid("membership refers to an unknown project"));
            }
        }
        let mut folders = BTreeSet::new();
        for entry in &self.personal_folders {
            identity(&entry.node_id)?;
            if !ids.contains(&entry.project_id)
                || !folders.insert((&entry.project_id, &entry.node_id))
                || entry.paths.iter().any(String::is_empty)
            {
                return Err(invalid("folder bindings must identify a project and node"));
            }
        }
        Ok(())
    }

    fn present(&self, id: &str) -> tembra::Result<()> {
        if self
            .records
            .iter()
            .any(|entry| entry.id == id && !entry.archived)
        {
            Ok(())
        } else {
            Err(invalid("the project is unavailable"))
        }
    }

    pub fn apply(mut self, command: Command) -> tembra::Result<Self> {
        match command {
            Command::Create {
                id,
                name,
                description,
            } => {
                identity(&id)?;
                if self.records.iter().any(|entry| entry.id == id) {
                    return Err(invalid("project identity already exists"));
                }
                self.records.push(Project {
                    id,
                    name: name.trim().into(),
                    description: description.trim().into(),
                    archived: false,
                });
            }
            Command::Update {
                id,
                name,
                description,
            } => {
                self.present(&id)?;
                let record = self
                    .records
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| invalid("the project is unavailable"))?;
                record.name = name.trim().into();
                record.description = description.trim().into();
            }
            Command::Archive { id, archived } => {
                let record = self
                    .records
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| invalid("the project is unavailable"))?;
                record.archived = archived;
            }
            Command::Assign {
                conversation,
                project,
            } => {
                identity(&conversation)?;
                self.present(&project)?;
                self.links.insert(conversation, project);
            }
            Command::Unassign { conversation } => {
                self.links.remove(&conversation);
            }
            Command::Folders {
                project,
                node,
                paths,
            } => {
                self.present(&project)?;
                identity(&node)?;
                self.personal_folders
                    .retain(|entry| entry.project_id != project || entry.node_id != node);
                let mut seen = BTreeSet::new();
                let paths: Vec<_> = paths
                    .into_iter()
                    .filter(|path| !path.is_empty() && seen.insert(path.clone()))
                    .collect();
                if !paths.is_empty() {
                    self.personal_folders.push(PersonalFolders {
                        project_id: project,
                        node_id: node,
                        paths,
                    });
                }
            }
            Command::Import { mut data } => {
                if self.revision != 0
                    || !self.records.is_empty()
                    || !self.links.is_empty()
                    || !self.personal_folders.is_empty()
                {
                    return Err(invalid(
                        "import requires an empty native project collection",
                    ));
                }
                data.revision = self.revision;
                self = data;
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("project revision exhausted"))?;
        self.validate()?;
        Ok(self)
    }
}
