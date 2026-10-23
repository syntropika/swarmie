use swarmie_task_contracts::{
    CommitResult, TaskSessionAppend, TaskStorageRef, TaskTransaction, validate_task_payload_size,
};
use tembra::{Context, Result, RuntimeError};

/// Already-admitted commit capability. Retaining its Reference allows cleanup calls while the
/// Host refuses new acquisitions; the owner Resource also declares its teardown dependency.
#[derive(Clone)]
pub(crate) enum Committer {
    TaskStorage,
    TaskSession(swarmie_task_session_contracts::TaskSessionCommitRef),
}

/// Selects one durable owner for the entire mutation; history never receives a two-write fallback.
pub(crate) async fn commit(
    storage: &TaskStorageRef,
    context: &Context,
    committer: &Committer,
    transaction: TaskTransaction,
    history: Vec<TaskSessionAppend>,
) -> Result<CommitResult> {
    use swarmie_task_session_contracts::{
        TaskSessionCommitOperation, TaskSessionCommitRequest, TaskSessionCommitResult,
        validate_task_session_commit_receipt,
    };
    let joint = match committer {
        Committer::TaskStorage => {
            if !history.is_empty() {
                return Err(RuntimeError::invalid(
                    "durable_tasks_joint_commit",
                    "staged history requires task_session commit selection",
                ));
            }
            return storage.with_context(context).commit(transaction).await;
        }
        Committer::TaskSession(joint) => joint,
    };
    let identity = joint.with_context(context).identity().await?;
    let source = storage.with_context(context).identity().await?;
    if identity.task_store_id != source.store_id {
        return Err(RuntimeError::invalid(
            "durable_tasks_joint_commit",
            "joint provider differs from the selected task owner",
        ));
    }
    // Construction only validates immutable local content, before any provider commit call.
    // Its Session validation errors describe a refused handler decision, never uncertain I/O.
    let operation = TaskSessionCommitOperation::new(identity, transaction.writes, history)
        .map_err(|error| RuntimeError::invalid("durable_tasks_joint_commit", error.to_string()))?;
    let request = TaskSessionCommitRequest {
        owner: transaction.owner,
        operation: operation.clone(),
    };
    validate_task_payload_size(&request)?;
    // Unknown I/O follows the ordinary task-store policy: stop the scheduler without a
    // replacement write. On reopen, committed task/history state selects the resumable
    // phase; an uncommitted decision leaves its old checkpoint. External-effect evidence
    // remains independently durable. This call never re-invokes a handler to resolve an
    // unconfirmed reply.
    match joint.with_context(context).commit(request).await? {
        TaskSessionCommitResult::Committed { receipt } => {
            // An invalid reply may follow COMMIT. Do not turn uncertainty into a new fault
            // write or infer this operation's success from a later store watermark.
            validate_task_session_commit_receipt(&operation, &receipt).map_err(|error| {
                RuntimeError::unavailable("durable_tasks_joint_receipt", error.to_string())
            })?;
            Ok(CommitResult::Committed {
                sequence: receipt.task_sequence,
                records: receipt.records,
            })
        }
        TaskSessionCommitResult::Conflict {
            id,
            actual_revision,
        } => {
            if !operation.writes.iter().any(|write| write.record.id == id)
                || actual_revision.is_some_and(|revision| {
                    revision == 0 || revision > swarmie_task_contracts::MAX_PORTABLE_INTEGER
                })
            {
                return Err(RuntimeError::unavailable(
                    "durable_tasks_joint_receipt",
                    "joint conflict identifies an unknown task or invalid revision",
                ));
            }
            Ok(CommitResult::Conflict {
                id,
                actual_revision,
            })
        }
    }
}
