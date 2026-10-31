//! Typed compaction work over the host's existing Durable Tasks scheduler.
//! The host owns the handler, protected physical scope and compatible storage commit.

use crate::{Plan, WorkFence};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use swarmie_providers::OperationId;

pub const SERVICE: &str = "swarmie.context.compaction";

/// Provider operation identity and fixed selection. Durable TaskId and physical
/// AttemptId remain distinct, explicitly associated identities in the host backend.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub operation: OperationId,
    pub fence: WorkFence,
    pub plan: Plan,
}

pub fn definition() -> swarmie_durable_tasks::TaskDefinition {
    swarmie_durable_tasks::TaskDefinition::new(SERVICE, NonZeroU32::MIN)
        .expect("static context task identity")
}
