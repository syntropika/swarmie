//! Origin commits of scheduler-owned task mutations and their associated session evidence.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use swarmie_session_contracts::{AppendReceipt, StoreHead};
use swarmie_task_contracts::{
    MAX_PORTABLE_INTEGER, TaskId, TaskRecord, TaskSessionAppend, TaskWrite,
    validate_task_payload_size,
};
use tembra::prelude::*;

/// Live domain identities sharing one actual persistence owner. Equality of labels or paths
/// does not admit independent connections into a joint transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSessionCommitIdentity {
    pub task_store_id: String,
    pub session_store_id: String,
}

/// Complete immutable operation, independent of the current ephemeral scheduler owner UUID.
/// Exact retries retain this payload and operation ID across owner replacement. A provider
/// validates the current claim before consulting any retained successful receipt.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSessionCommitOperation {
    pub operation_id: String,
    pub identity: TaskSessionCommitIdentity,
    pub writes: Vec<TaskWrite>,
    /// Explicit handler evidence; an empty list still admits an atomic task-only mutation.
    pub history: Vec<TaskSessionAppend>,
}

/// Current authority accompanies, but is not part of, the immutable operation's replay identity.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSessionCommitRequest {
    pub owner: String,
    pub operation: TaskSessionCommitOperation,
}

/// Exact original receipt, retained in the same transaction as the accepted writes.
/// Replaying it does not claim that these records or watermarks are still the latest values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSessionCommitReceipt {
    pub operation_id: String,
    pub task_sequence: u64,
    pub records: Vec<TaskRecord>,
    /// Receipts for explicit history only, in request order. Provider-owned structural
    /// projections never alter this list.
    pub appends: Vec<AppendReceipt>,
    pub head: StoreHead,
}

/// Known CAS refusal changes neither domain nor the receipt ledger. An I/O error can have an
/// unknown outcome. Reconcile authoritative task/history state or retry the identical operation;
/// never invent a different decision from an unconfirmed response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::commit::tagged_union)]
pub enum TaskSessionCommitResult {
    Committed {
        receipt: TaskSessionCommitReceipt,
    },
    Conflict {
        id: TaskId,
        actual_revision: Option<u64>,
    },
}

fn tagged_union(schema: &mut tembra::schemars::Schema) {
    if let Some(object) = schema.as_object_mut()
        && let Some(branches) = object.remove("oneOf")
    {
        object.insert("anyOf".into(), branches);
    }
}

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::invalid("task_session_commit_invalid", message)
}

impl TaskSessionCommitOperation {
    /// Freezes a bounded typed operation and derives its ID from canonical immutable content.
    ///
    /// # Errors
    /// Refuses invalid identities, empty task writes, oversized batches, duplicate tasks/batches and
    /// history outside the written tasks. No storage is touched.
    pub fn new(
        identity: TaskSessionCommitIdentity,
        writes: Vec<TaskWrite>,
        history: Vec<TaskSessionAppend>,
    ) -> Result<Self> {
        let mut operation = Self {
            operation_id: String::new(),
            identity,
            writes,
            history,
        };
        operation.operation_id = operation.content_id()?;
        operation.validate()?;
        Ok(operation)
    }

    fn content_id(&self) -> Result<String> {
        let content = swarmie_session_contracts::canonical(&(
            "swarmie.task-session-commit.v1",
            &self.identity,
            &self.writes,
            &self.history,
        ))?;
        Ok(format!(
            "joint:{}",
            hex::encode(Sha256::digest(content.as_bytes()))
        ))
    }

    /// Checks immutable identity and complete bounds before provider mutation or ledger replay.
    ///
    /// # Errors
    /// Rejects malformed or changed content under an operation ID, including foreign history.
    pub fn validate(&self) -> Result<()> {
        swarmie_task_contracts::validate_identity(&self.identity.task_store_id)?;
        swarmie_session_contracts::identity(&self.identity.session_store_id)?;
        if !(1..=256).contains(&self.writes.len()) || self.history.len() > 256 {
            return Err(invalid(
                "joint commits require 1..=256 task writes and at most 256 history batches",
            ));
        }
        let tasks = self
            .writes
            .iter()
            .map(|write| write.record.id.as_str())
            .collect::<BTreeSet<_>>();
        if tasks.len() != self.writes.len() {
            return Err(invalid("joint task writes must have distinct identities"));
        }
        let mut batches = BTreeSet::new();
        for history in &self.history {
            if history.session_store_id != self.identity.session_store_id {
                return Err(invalid(
                    "history destination differs from the admitted joint owner",
                ));
            }
            swarmie_session_contracts::validate_append(&history.append)?;
            if !batches.insert((&history.append.session_id, &history.append.batch_id)) {
                return Err(invalid(
                    "joint history batches must have distinct identities",
                ));
            }
            if history.append.events.iter().any(|event| {
                event
                    .correlation
                    .task_id
                    .as_deref()
                    .is_none_or(|id| !tasks.contains(id))
            }) {
                return Err(invalid(
                    "joint history must identify one of the written tasks",
                ));
            }
        }
        if self.operation_id != self.content_id()? {
            return Err(invalid(
                "joint operation ID differs from its immutable content",
            ));
        }
        validate_task_payload_size(self)?;
        Ok(())
    }
}

/// Checks a bounded receipt against the exact operation, without treating later store state as
/// evidence for an earlier uncertain result. The order of task and append receipts is preserved.
///
/// # Errors
/// Refuses mismatched records, identities, counts or sequence ranges before local publication.
pub fn validate_task_session_commit_receipt(
    operation: &TaskSessionCommitOperation,
    receipt: &TaskSessionCommitReceipt,
) -> Result<()> {
    validate_task_payload_size(receipt)?;
    if receipt.operation_id != operation.operation_id
        || !(1..=MAX_PORTABLE_INTEGER).contains(&receipt.task_sequence)
        || receipt.records.len() != operation.writes.len()
        || receipt
            .records
            .iter()
            .any(|record| record.revision > receipt.task_sequence)
        || receipt.appends.len() != operation.history.len()
        || receipt.head.store_id != operation.identity.session_store_id
        || receipt.head.commit_sequence > MAX_PORTABLE_INTEGER
    {
        return Err(invalid(
            "joint receipt differs from its immutable operation",
        ));
    }
    for (actual, expected) in receipt.records.iter().zip(&operation.writes) {
        if swarmie_session_contracts::canonical(actual)?
            != swarmie_session_contracts::canonical(&expected.record)?
        {
            return Err(invalid(
                "joint task receipt differs from its canonical replacement",
            ));
        }
    }
    for (append, history) in receipt.appends.iter().zip(&operation.history) {
        let count = history.append.events.len() as u64;
        if count == 0 {
            return Err(invalid("joint append receipt has no corresponding events"));
        }
        if append.store_id != history.session_store_id
            || append.session_id != history.append.session_id
            || append.batch_id != history.append.batch_id
            || append.first_sequence == 0
            || append.first_sequence.checked_add(count - 1) != Some(append.last_sequence)
            || append.last_sequence > MAX_PORTABLE_INTEGER
            || append.commit_sequence == 0
            || append.commit_sequence > receipt.head.commit_sequence
            || history
                .append
                .expected_head
                .is_some_and(|head| head.checked_add(1) != Some(append.first_sequence))
        {
            return Err(invalid(
                "joint append receipt differs from its bound history batch",
            ));
        }
    }
    Ok(())
}

/// Explicit same-owner capability. The scheduler supplies its current claim and complete task
/// mutation only after handler settlement and final cancellation/ownership checks. A successful
/// call commits every task write, its ordinary outbox fact, all associated history and this
/// operation's receipt within one physical transaction. It never schedules or invokes handlers.
/// Independent providers cannot implement this contract by sequencing ordinary async calls.
///
/// Missing capabilities, identity/authority failures, invalid routes and known CAS conflicts
/// commit nothing. Invalid errors named `session_store_invalid` or `session_store_too_large`
/// also refuse the entire transaction, including any task writes preceding append admission.
/// Unavailable errors can have an unknown outcome. A caller may halt and reopen to recover
/// authoritative task/history state after an unknown outcome. Any explicit retry retains the identical operation; current authority is
/// checked even for an existing receipt. External effects remain outside this transaction.
#[tembra::contract(local, name = "swarmie.task-session-commit", version = 1)]
pub trait TaskSessionCommit {
    #[portable]
    async fn identity(&self) -> Result<TaskSessionCommitIdentity>;

    #[portable]
    async fn commit(&self, request: TaskSessionCommitRequest) -> Result<TaskSessionCommitResult>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_session_contracts::{AppendEvents, Correlation, NewEvent};
    use swarmie_task_contracts::TaskSpec;

    fn operation() -> TaskSessionCommitOperation {
        let mut record = TaskRecord::new(
            TaskSpec {
                id: "task".into(),
                kind: "fixture.task".into(),
                definition_version: 1,
                input: serde_json::json!({}),
                checkpoint: serde_json::json!({"prepared":true}),
                phase: "effect".into(),
            },
            None,
        );
        record.revision = 2;
        TaskSessionCommitOperation::new(
            TaskSessionCommitIdentity {
                task_store_id: "tasks".into(),
                session_store_id: "sessions".into(),
            },
            vec![TaskWrite {
                provenance: None,
                expected_revision: Some(1),
                record,
            }],
            vec![TaskSessionAppend {
                session_store_id: "sessions".into(),
                append: AppendEvents {
                    session_id: "chat".into(),
                    batch_id: "intent".into(),
                    expected_head: None,
                    events: vec![NewEvent {
                        event_id: "intent".into(),
                        kind: "fixture.intent".into(),
                        version: 1,
                        correlation: Correlation {
                            task_id: Some("task".into()),
                            ..Default::default()
                        },
                        occurred_at_ms: None,
                        payload: serde_json::json!({"operation":"effect"}),
                    }],
                },
            }],
        )
        .unwrap()
    }

    fn receipt(operation: &TaskSessionCommitOperation) -> TaskSessionCommitReceipt {
        TaskSessionCommitReceipt {
            operation_id: operation.operation_id.clone(),
            task_sequence: 2,
            records: operation
                .writes
                .iter()
                .map(|write| write.record.clone())
                .collect(),
            appends: vec![AppendReceipt {
                store_id: "sessions".into(),
                session_id: "chat".into(),
                batch_id: "intent".into(),
                first_sequence: 1,
                last_sequence: 1,
                commit_sequence: 1,
            }],
            head: StoreHead {
                store_id: "sessions".into(),
                commit_sequence: 1,
            },
        }
    }

    #[test]
    fn immutable_operation_identity_excludes_current_owner_and_refuses_changed_content() {
        let operation = operation();
        let first = TaskSessionCommitRequest {
            owner: "first-live-owner".into(),
            operation: operation.clone(),
        };
        let replacement = TaskSessionCommitRequest {
            owner: "replacement-live-owner".into(),
            operation: operation.clone(),
        };
        assert_eq!(
            first.operation.operation_id,
            replacement.operation.operation_id
        );
        let repeated = TaskSessionCommitOperation::new(
            operation.identity.clone(),
            operation.writes.clone(),
            operation.history.clone(),
        )
        .unwrap();
        assert_eq!(operation.operation_id, repeated.operation_id);
        let mut changed = operation.clone();
        changed.history[0].append.events[0].payload = serde_json::json!({"operation":"other"});
        assert!(changed.validate().is_err());
        let new =
            TaskSessionCommitOperation::new(changed.identity, changed.writes, changed.history)
                .unwrap();
        assert_ne!(operation.operation_id, new.operation_id);
    }

    #[test]
    fn history_is_bound_to_the_live_destination_and_written_task() {
        let operation = operation();
        let mut foreign = operation.history.clone();
        foreign[0].session_store_id = "replacement-sessions".into();
        assert!(
            TaskSessionCommitOperation::new(
                operation.identity.clone(),
                operation.writes.clone(),
                foreign
            )
            .is_err()
        );
        let mut unrelated = operation.history.clone();
        unrelated[0].append.events[0].correlation.task_id = Some("unwritten-task".into());
        assert!(
            TaskSessionCommitOperation::new(
                operation.identity.clone(),
                operation.writes.clone(),
                unrelated
            )
            .is_err()
        );
        assert!(
            TaskSessionCommitOperation::new(operation.identity, operation.writes, Vec::new())
                .is_ok()
        );
    }

    #[test]
    fn task_only_commits_accept_an_untouched_session_watermark() {
        let initial = operation();
        let task_only =
            TaskSessionCommitOperation::new(initial.identity, initial.writes, Vec::new()).unwrap();
        let mut receipt = receipt(&task_only);
        receipt.appends.clear();
        receipt.head.commit_sequence = 0;
        validate_task_session_commit_receipt(&task_only, &receipt).unwrap();
        receipt.head.commit_sequence = MAX_PORTABLE_INTEGER + 1;
        assert!(validate_task_session_commit_receipt(&task_only, &receipt).is_err());
    }

    #[test]
    fn receipt_validation_refuses_corrupt_identity_counts_and_ranges() {
        let operation = operation();
        let receipt = receipt(&operation);
        validate_task_session_commit_receipt(&operation, &receipt).unwrap();
        let mut bad = Vec::new();
        let mut value = receipt.clone();
        value.operation_id = "foreign".into();
        bad.push(value);
        let mut value = receipt.clone();
        value.task_sequence = 0;
        bad.push(value);
        let mut value = receipt.clone();
        value.task_sequence = 1;
        bad.push(value);
        let mut value = receipt.clone();
        value.records[0].checkpoint = serde_json::json!({"other":true});
        bad.push(value);
        let mut value = receipt.clone();
        value.records.clear();
        bad.push(value);
        let mut value = receipt.clone();
        value.appends.clear();
        bad.push(value);
        let mut value = receipt.clone();
        value.appends[0].store_id = "foreign".into();
        bad.push(value);
        let mut value = receipt.clone();
        value.appends[0].last_sequence = 2;
        bad.push(value);
        let mut value = receipt.clone();
        value.appends[0].commit_sequence = 2;
        bad.push(value);
        let mut value = receipt.clone();
        value.head.store_id = "foreign".into();
        bad.push(value);
        for corrupt in bad {
            assert!(
                validate_task_session_commit_receipt(&operation, &corrupt).is_err(),
                "{corrupt:?}"
            );
        }
        let mut empty = operation;
        empty.history[0].append.events.clear();
        assert!(validate_task_session_commit_receipt(&empty, &receipt).is_err());
    }
}
