//! Context projection and compaction transitions, independent of hosting and scheduling.
//! The execution owner must persist each proposed checkpoint before external work.
//! History payloads remain in Conversations; checkpoints contain fixed references.

mod artifact;
mod budget;
mod fence;
mod operation;
mod service;
pub mod textual;

#[cfg(feature = "durable-tasks")]
pub mod durable;

pub use artifact::ArtifactRef;
pub use budget::{
    Budget, BudgetDecision, BudgetPolicy, COMPACTION_REQUIRED, INPUT_TOO_LARGE,
    compaction_required, input_too_large,
};
pub use fence::{CompactArguments, CompactionRequest, WorkFence};
pub use operation::{
    Activation, Candidate, Checkpoint, Compaction, Configuration, Phase, Plan, Projection, Request,
    Requested, Strategy,
};
pub use service::{
    Admission, Backend, Inspection, Receipt, SERVICE, Session, Snapshot, compact_command, orb,
};

/// Safe status text and State budgets, separate from user input and model limits.
pub const MAX_REASON_BYTES: usize = 1024;

pub const MAX_SNAPSHOT_BYTES: usize = 16 * 1024;

fn validate_reason(reason: &str) -> tembra::Result<()> {
    if reason.trim().is_empty() || reason.len() > MAX_REASON_BYTES {
        return Err(invalid(
            "context status reason is empty or exceeds its display budget",
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> tembra::Error {
    tembra::Error::invalid("swarmie.context.operation", message)
}
