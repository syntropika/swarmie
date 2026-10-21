use swarmie_task_contracts::{
    TaskOutboxCompaction, TaskOutboxConsumerState, TaskOutboxRetention, TaskOutboxStatus,
};
use tembra::prelude::*;

use crate::BindingKey;

/// Bounded serial publication; limit is 1..=256 source facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PublishRequest {
    #[schemars(range(min = 1, max = 256))]
    pub limit: u32,
}

/// Counts only source facts durably acknowledged in this pass, including explicit exclusions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PublishReceipt {
    pub published: u32,
    pub excluded: u32,
    pub consumer: TaskOutboxConsumerState,
}

/// Authoritative capacity/positions plus bounded volatile failure diagnostics. A restart clears
/// failure counters, not durable positions. Lag is a lower bound when lag_complete is false.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PublisherStatus {
    pub source: TaskOutboxStatus,
    pub consumer: Option<TaskOutboxConsumerState>,
    pub lag: u64,
    pub lag_complete: bool,
    pub failures: u64,
    pub last_error: Option<String>,
    pub missing_binding: Option<BindingKey>,
    pub capacity_exhausted: bool,
}

/// Managed Facet publisher. One stable configured consumer, one serial pass at a time.
/// Append/ACK errors can follow committed work. Every retry reconstructs the identical batch;
/// no later fact is acknowledged past an unresolved one. Physical-settlement hooks are excluded.
#[tembra::contract(local, name = "swarmie.task-session-publisher", version = 1)]
pub trait TaskSessionPublisher {
    #[portable]
    async fn publish(&self, request: PublishRequest) -> Result<PublishReceipt>;

    #[portable]
    async fn diagnostics(&self) -> Result<PublisherStatus>;

    /// Explicit cleanup only; the source protects every registered consumer's ACK boundary.
    #[portable]
    async fn compact(&self, request: TaskOutboxCompaction) -> Result<TaskOutboxRetention>;
}
