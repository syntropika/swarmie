mod apply;
mod control;
mod plan;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use swarmie_composition::{EffectiveComposition, EffectiveInstance};

pub use plan::ReconciliationError;

/// Declarative difference from the last acknowledged application selections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Replaced,
    Reconfigured,
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceChange {
    pub instance_id: String,
    pub kind: ChangeKind,
    pub before: Option<EffectiveInstance>,
    pub after: Option<EffectiveInstance>,
}

/// Validated desired metadata; construction performs no installation or data access.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationPlan {
    pub desired: EffectiveComposition,
    pub changes: Vec<InstanceChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionOutcome {
    Applied,
    Unchanged,
    Failed,
    RestartRequired,
    RecoveryRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionResult {
    pub outcome: TransitionOutcome,
    pub diagnostic: String,
}

/// Acknowledged transitions only. Desired persistence never establishes runtime success.
/// Browser observations remain session-specific and are not made atomic with server changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub plan: ReconciliationPlan,
    pub desired_persisted: bool,
    pub results: BTreeMap<String, TransitionResult>,
    pub recovery_required: bool,
}
