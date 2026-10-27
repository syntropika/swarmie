use crate::{
    Assignment, AttemptId, PreparedTasks, Rejection, SignalReceipts, SignalSnapshot, TaskChange,
    TaskId, TaskRecord, TaskStatus,
};
use std::collections::BTreeMap;

/// Computes an atomic candidate from task state and a complete targeted receipt snapshot.
/// Adapters must persist both `records` and `signals` before acknowledging success.
/// Missing receipt reads are rejected before evaluating any transition. All reads and
/// writes belong to the same authority transaction; an earlier snapshot is not authority.
pub fn apply_changes(
    current: &BTreeMap<TaskId, TaskRecord>,
    receipts: &SignalSnapshot,
    now: u64,
    last_sequence: u64,
    changes: Vec<TaskChange>,
) -> Result<PreparedTasks, Rejection> {
    if changes
        .iter()
        .filter_map(TaskChange::receipt_key)
        .any(|key| !receipts.contains_key(&key))
    {
        return Err(Rejection::IncompleteReceipts);
    }
    let mut next = current.clone();
    let mut receipts: SignalReceipts = receipts
        .iter()
        .filter_map(|(key, value)| value.clone().map(|value| (*key, value)))
        .collect();
    let mut sequence = last_sequence;
    for change in changes {
        match change {
            TaskChange::Create { spec, principal } => {
                if next.contains_key(&spec.id) {
                    return Err(Rejection::AlreadyExists);
                }
                if spec.definition.validate().is_err() || principal.is_empty() {
                    return Err(Rejection::Invalid);
                }
                sequence = sequence.checked_add(1).ok_or(Rejection::Exhausted)?;
                next.insert(
                    spec.id,
                    TaskRecord {
                        spec,
                        principal,
                        checkpoint: None,
                        status: TaskStatus::Ready,
                        revision: 0,
                        created_at: now,
                        sequence,
                        assignment: None,
                        delivery: None,
                    },
                );
            }
            TaskChange::Acquire {
                id,
                revision,
                executor,
                attempt,
                lease_ms,
            } => {
                let task = next.get_mut(&id).ok_or(Rejection::NotFound)?;
                match &task.status {
                    TaskStatus::Ready => {}
                    TaskStatus::Waiting(crate::WaitState::Until(at)) if *at <= now => {
                        task.status = TaskStatus::Ready;
                    }
                    _ => return Err(Rejection::Terminal),
                }
                if task.revision != revision {
                    return Err(Rejection::Revision);
                }
                if task.assignment.as_ref().is_some_and(|a| a.expires_at > now) {
                    return Err(Rejection::Assigned);
                }
                task.assignment = Some(Assignment {
                    executor,
                    attempt,
                    expires_at: now
                        .checked_add(lease_ms.get())
                        .ok_or(Rejection::Exhausted)?,
                });
            }
            TaskChange::Signal { id, key, value } => {
                let task = next.get_mut(&id).ok_or(Rejection::NotFound)?;
                let (signals, receipt_key) = (&mut receipts, (id, key));
                if let Some(receipt) = signals.get(&receipt_key) {
                    if receipt.value != value {
                        return Err(Rejection::Invalid);
                    }
                    continue;
                }
                if matches!(
                    task.status,
                    TaskStatus::Completed(_) | TaskStatus::Failed(_)
                ) {
                    return Err(Rejection::Terminal);
                }
                let consumed = task.status == TaskStatus::Waiting(crate::WaitState::Signal(key));
                if consumed {
                    task.status = TaskStatus::Ready;
                    task.delivery = Some(crate::SignalDelivery {
                        key,
                        value: value.clone(),
                    });
                }
                signals.insert(receipt_key, crate::SignalReceipt { value, consumed });
            }
            TaskChange::Suspend {
                id,
                attempt,
                revision,
                checkpoint,
                condition,
            } => {
                let task = next.get_mut(&id).ok_or(Rejection::NotFound)?;
                authorized(task, attempt, now)?;
                ready(task)?;
                if task.revision != revision {
                    return Err(Rejection::Revision);
                }
                task.confirm(crate::model::Transition::Checkpoint(checkpoint))?;
                task.delivery = None;
                task.status = match condition {
                    crate::WaitFor::Delay { milliseconds } => {
                        TaskStatus::Waiting(crate::WaitState::Until(
                            now.checked_add(milliseconds).ok_or(Rejection::Exhausted)?,
                        ))
                    }
                    crate::WaitFor::Signal(key) => {
                        let (signals, receipt_key) = (&mut receipts, (id, key));
                        if let Some(receipt) = signals.get_mut(&receipt_key) {
                            if receipt.consumed {
                                return Err(Rejection::Invalid);
                            }
                            receipt.consumed = true;
                            task.delivery = Some(crate::SignalDelivery {
                                key,
                                value: receipt.value.clone(),
                            });
                            TaskStatus::Ready
                        } else {
                            TaskStatus::Waiting(crate::WaitState::Signal(key))
                        }
                    }
                };
            }
            TaskChange::Renew {
                id,
                attempt,
                lease_ms,
            } => {
                let task = next.get_mut(&id).ok_or(Rejection::NotFound)?;
                authorized(task, attempt, now)?;
                task.assignment
                    .as_mut()
                    .ok_or(Rejection::StaleAttempt)?
                    .expires_at = now
                    .checked_add(lease_ms.get())
                    .ok_or(Rejection::Exhausted)?;
            }
            TaskChange::Release { id, attempt } => {
                let task = next.get_mut(&id).ok_or(Rejection::NotFound)?;
                // Physical settlement may occur after expiry; never release a successor.
                if task.assignment.as_ref().map(|a| a.attempt) != Some(attempt) {
                    return Err(Rejection::StaleAttempt);
                }
                task.assignment = None;
            }
            progress => {
                let (id, attempt, revision) = match &progress {
                    TaskChange::Checkpoint {
                        id,
                        attempt,
                        revision,
                        ..
                    }
                    | TaskChange::Complete {
                        id,
                        attempt,
                        revision,
                        ..
                    }
                    | TaskChange::Fail {
                        id,
                        attempt,
                        revision,
                        ..
                    } => (*id, *attempt, *revision),
                    _ => unreachable!(),
                };
                let task = next.get_mut(&id).ok_or(Rejection::NotFound)?;
                authorized(task, attempt, now)?;
                ready(task)?;
                if task.revision != revision {
                    return Err(Rejection::Revision);
                }
                let transition = match progress {
                    TaskChange::Checkpoint { checkpoint, .. } => {
                        crate::model::Transition::Checkpoint(checkpoint)
                    }
                    TaskChange::Complete { result, .. } => {
                        crate::model::Transition::Complete(result)
                    }
                    TaskChange::Fail { error, .. } => crate::model::Transition::Fail(error),
                    _ => unreachable!(),
                };
                task.confirm(transition)?;
            }
        }
    }
    Ok(PreparedTasks {
        records: next,
        sequence,
        signals: receipts,
    })
}

fn ready(task: &TaskRecord) -> Result<(), Rejection> {
    if task.status == TaskStatus::Ready {
        Ok(())
    } else {
        Err(Rejection::Terminal)
    }
}

fn authorized(task: &TaskRecord, attempt: AttemptId, now: u64) -> Result<(), Rejection> {
    if task
        .assignment
        .as_ref()
        .is_some_and(|a| a.attempt == attempt && a.expires_at > now)
    {
        Ok(())
    } else {
        Err(Rejection::StaleAttempt)
    }
}
