use serde::{Deserialize, Serialize};
use swarmie_providers::OperationId;
use uuid::Uuid;

/// Non-secret admission precondition. Only the backend's durable ledger can issue
/// a current value or confirm its scope. A wire value grants no authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "Uuid", into = "Uuid")]
pub struct WorkFence(Uuid);

impl TryFrom<Uuid> for WorkFence {
    type Error = tembra::Error;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        if value.is_nil() {
            return Err(crate::invalid("an admission fence cannot be nil"));
        }
        Ok(Self(value))
    }
}

impl From<WorkFence> for Uuid {
    fn from(value: WorkFence) -> Self {
        value.0
    }
}

/// Captured before asynchronous admission. Recognition must retain this fence;
/// a delayed request cannot silently bind to the current value after Stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionRequest {
    pub operation: OperationId,
    pub fence: WorkFence,
}

/// Strict semantic-command arguments. Generic Commands invocation is unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactArguments {
    pub fence: WorkFence,
}
