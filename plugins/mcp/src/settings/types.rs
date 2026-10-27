use crate::{Configuration, Endpoint, Limits, Observation, Protocol, ServerId, invalid};
use serde::{Deserialize, Serialize};

/// Definitions belong to the authenticated user on the execution node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    User,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub id: ServerId,
    pub name: String,
    pub scope: Scope,
    pub endpoint: Endpoint,
    pub protocol: Protocol,
    pub enabled: bool,
    pub revision: u64,
}

impl Definition {
    pub fn validate(&self) -> tembra::Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 128 || self.revision == 0 {
            return Err(invalid(
                "provide a server name and a positive definition revision",
            ));
        }
        self.configuration().validate()
    }

    pub(crate) fn configuration(&self) -> Configuration {
        Configuration {
            id: self.id.clone(),
            endpoint: self.endpoint.clone(),
            protocol: self.protocol,
            limits: Limits::default(),
        }
    }

    pub(crate) fn key(&self) -> Vec<String> {
        vec![self.id.as_str().into(), self.revision.to_string()]
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub definition: Definition,
    pub observation: Observation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveDefinition {
    pub definition: Definition,
    pub expected_revision: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerAction {
    pub id: ServerId,
    pub revision: u64,
}
