//! SQLite row encoding and task record validation.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, num::NonZeroU32};
use swarmie_durable_tasks::{
    Assignment, StorageError, TaskChange, TaskDefinition, TaskId, TaskRecord, TaskSpec, TaskState,
    TaskStatus, TaskSummary,
};

#[derive(Serialize, Deserialize)]
struct Identity {
    spec: TaskSpec,
    principal: String,
    created_at: u64,
    sequence: u64,
}

#[derive(Serialize, Deserialize)]
struct Progress {
    checkpoint: Option<Value>,
    status: TaskStatus,
    revision: u64,
    delivery: Option<swarmie_durable_tasks::SignalDelivery>,
}

pub(super) const META: &str =
    "id,service,version,principal,revision,sequence,created_at,state,assignment";

pub(super) fn insert(connection: &Connection, record: &TaskRecord) -> Result<(), StorageError> {
    connection
        .execute(
            "INSERT INTO swarmie_tasks(id,data,progress,assignment,service,version,principal,
            revision,sequence,created_at,state,expires_at,wake_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                record.spec.id.to_string(),
                encode(&Identity {
                    spec: record.spec.clone(),
                    principal: record.principal.clone(),
                    created_at: record.created_at,
                    sequence: record.sequence
                })?,
                encode(&Progress {
                    checkpoint: record.checkpoint.clone(),
                    status: record.status.clone(),
                    revision: record.revision,
                    delivery: record.delivery.clone(),
                })?,
                record.assignment.as_ref().map(encode).transpose()?,
                record.spec.definition.service(),
                record.spec.definition.version.get(),
                record.principal,
                record.revision,
                record.sequence,
                record.created_at,
                record.status.state().as_str(),
                record.assignment.as_ref().map(|a| a.expires_at),
                wake_at(&record.status)
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

pub(super) fn update(
    connection: &Connection,
    previous: &TaskRecord,
    record: &TaskRecord,
) -> Result<(), StorageError> {
    if previous.checkpoint != record.checkpoint
        || previous.status != record.status
        || previous.revision != record.revision
        || previous.delivery != record.delivery
    {
        connection
            .execute(
                "UPDATE swarmie_tasks SET progress=?1,state=?2,revision=?3,wake_at=?5 WHERE id=?4",
                params![
                    encode(&Progress {
                        checkpoint: record.checkpoint.clone(),
                        status: record.status.clone(),
                        revision: record.revision,
                        delivery: record.delivery.clone(),
                    })?,
                    record.status.state().as_str(),
                    record.revision,
                    record.spec.id.to_string(),
                    wake_at(&record.status)
                ],
            )
            .map_err(sql_error)?;
    }
    if previous.assignment != record.assignment {
        connection
            .execute(
                "UPDATE swarmie_tasks SET assignment=?1,expires_at=?2 WHERE id=?3",
                params![
                    record.assignment.as_ref().map(encode).transpose()?,
                    record.assignment.as_ref().map(|a| a.expires_at),
                    record.spec.id.to_string()
                ],
            )
            .map_err(sql_error)?;
    }
    Ok(())
}

pub(super) type Metadata = (
    String,
    String,
    u32,
    String,
    u64,
    u64,
    u64,
    String,
    Option<String>,
);

pub(super) fn metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<Metadata> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

pub(super) fn summary(
    (id, service, version, principal, revision, sequence, created_at, state, assignment): Metadata,
) -> Result<TaskSummary, StorageError> {
    let id = serde_json::from_value(Value::String(id)).map_err(invalid)?;

    let definition = TaskDefinition::new(
        service,
        NonZeroU32::new(version)
            .ok_or_else(|| StorageError::Invalid("zero task version".into()))?,
    )?;

    let state = match state.as_str() {
        "ready" => TaskState::Ready,
        "sleeping" => TaskState::Sleeping,
        "waiting" => TaskState::Waiting,
        "completed" => TaskState::Completed,
        "failed" => TaskState::Failed,
        _ => return Err(StorageError::Invalid("unknown task state".into())),
    };

    Ok(TaskSummary {
        id,
        definition,
        principal,
        revision,
        sequence,
        created_at,
        state,
        assignment: assignment
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(invalid)?,
    })
}

pub(super) fn read(
    connection: &Connection,
    id: TaskId,
) -> Result<Option<TaskRecord>, StorageError> {
    let row = connection
        .query_row(
            "SELECT data,progress,assignment FROM swarmie_tasks WHERE id=?1",
            [id.to_string()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;

    row.map(|(data, progress, assignment)| {
        let identity: Identity = serde_json::from_str(&data).map_err(invalid)?;

        let progress: Progress = serde_json::from_str(&progress).map_err(invalid)?;

        let assignment: Option<Assignment> = assignment
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(invalid)?;

        TaskDefinition::new(
            identity.spec.definition.service(),
            identity.spec.definition.version,
        )?;

        if identity.spec.id != id || identity.principal.is_empty() {
            return Err(StorageError::Invalid("task identity mismatch".into()));
        }
        Ok(TaskRecord {
            spec: identity.spec,
            principal: identity.principal,
            checkpoint: progress.checkpoint,
            status: progress.status,
            revision: progress.revision,
            created_at: identity.created_at,
            sequence: identity.sequence,
            assignment,
            delivery: progress.delivery,
        })
    })
    .transpose()
}

fn wake_at(status: &TaskStatus) -> Option<u64> {
    match status {
        TaskStatus::Waiting(swarmie_durable_tasks::WaitState::Until(at)) => Some(*at),
        _ => None,
    }
}

pub(super) fn time(connection: &Connection) -> Result<u64, StorageError> {
    connection
        .query_row(
            "SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER)",
            [],
            |r| r.get(0),
        )
        .map_err(sql_error)
}

fn encode(value: &impl Serialize) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(invalid)
}

fn invalid(error: serde_json::Error) -> StorageError {
    StorageError::Invalid(error.to_string())
}

pub(super) fn sql_error(error: rusqlite::Error) -> StorageError {
    if crate::owner::invalid_sql(&error) {
        StorageError::Invalid(error.to_string())
    } else {
        StorageError::Unavailable(error.to_string())
    }
}

pub(super) fn read_signal_receipts(
    connection: &Connection,
    changes: &[TaskChange],
) -> Result<swarmie_durable_tasks::SignalSnapshot, StorageError> {
    let keys = swarmie_durable_tasks::required_receipts(changes);
    let mut receipts = BTreeMap::new();
    if keys.is_empty() {
        return Ok(receipts);
    }
    let mut statement = connection
        .prepare_cached("SELECT value,consumed FROM swarmie_task_signals WHERE task=?1 AND key=?2")
        .map_err(sql_error)?;
    for (id, key) in keys {
        let row: Option<(String, bool)> = statement
            .query_row(params![id.to_string(), key.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(sql_error)?;
        let receipt = row
            .map(|(value, consumed)| -> Result<_, StorageError> {
                Ok(swarmie_durable_tasks::SignalReceipt {
                    value: serde_json::from_str(&value).map_err(invalid)?,
                    consumed,
                })
            })
            .transpose()?;
        receipts.insert((id, key), receipt);
    }
    Ok(receipts)
}

pub(super) fn write_signal_receipts(
    connection: &Connection,
    previous: &swarmie_durable_tasks::SignalSnapshot,
    next: &swarmie_durable_tasks::SignalReceipts,
) -> Result<(), StorageError> {
    for ((id, key), receipt) in next {
        if previous.get(&(*id, *key)).and_then(Option::as_ref) == Some(receipt) {
            continue;
        }
        connection
            .execute(
                "INSERT INTO swarmie_task_signals(task,key,value,consumed) VALUES(?1,?2,?3,?4)
            ON CONFLICT(task,key) DO UPDATE SET consumed=excluded.consumed",
                params![
                    id.to_string(),
                    key.to_string(),
                    encode(&receipt.value)?,
                    receipt.consumed
                ],
            )
            .map_err(sql_error)?;
    }
    Ok(())
}
