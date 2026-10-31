//! Provider diagnostic observations. The host supplies the sink and operation identity.
mod adapter;
pub use adapter::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    pub scope: Option<String>,
    pub order: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    pub name: String,
}
impl Emitter {
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    Handler,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Adapter { observation: AdapterObservation },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub subject: Subject,
    pub stage: Stage,
    pub emitter: Emitter,
    pub action: Action,
}
impl Observation {
    pub fn new(subject: Subject, stage: Stage, emitter: Emitter, action: Action) -> Self {
        Self {
            subject,
            stage,
            emitter,
            action,
        }
    }
}
pub trait Witness: Send + Sync {
    fn observe(&self, observation: Observation);
}
