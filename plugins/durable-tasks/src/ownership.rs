use std::collections::{BTreeMap, BTreeSet};

use swarmie_task_contracts::{TaskOutcome, TaskRecord, TaskStatus};

/// Only direct foreground children can be awaited. Independent work is submitted as a root.
pub(crate) fn children<'a>(records: &'a [TaskRecord], parent: &str) -> Vec<&'a TaskRecord> {
    records
        .iter()
        .filter(|record| record.parent.as_deref() == Some(parent))
        .collect()
}

pub(crate) fn unsettled_children(records: &[TaskRecord], parent: &str) -> bool {
    records
        .iter()
        .any(|record| record.parent.as_deref() == Some(parent) && !record.is_terminal())
}

pub(crate) fn child_failed(record: &TaskRecord) -> bool {
    matches!(record.status, TaskStatus::Completing | TaskStatus::Terminal)
        && !matches!(record.outcome, Some(TaskOutcome::Completed { .. }))
}

/// Iterative traversal remains bounded by actual persisted records, without recursive stacks.
pub(crate) fn descendants(records: &[TaskRecord], root: &str) -> BTreeSet<String> {
    let mut edges = BTreeMap::<&str, Vec<&str>>::new();
    for record in records {
        if let Some(parent) = &record.parent {
            edges.entry(parent).or_default().push(&record.id);
        }
    }
    let mut result = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if result.insert(id.to_owned()) {
            pending.extend(edges.get(id).into_iter().flatten().copied());
        }
    }
    result
}

pub(crate) fn cancellations(records: &[TaskRecord]) -> BTreeSet<String> {
    let mut cancelled = BTreeSet::new();
    for record in records {
        if record.cancellation_requested {
            cancelled.extend(descendants(records, &record.id));
        } else if matches!(
            record.outcome,
            Some(
                TaskOutcome::Failed { .. }
                    | TaskOutcome::Aborted { .. }
                    | TaskOutcome::Faulted { .. }
            )
        ) {
            for child in children(records, &record.id) {
                cancelled.extend(descendants(records, &child.id));
            }
        }
    }
    cancelled
}

pub(crate) fn settle(record: &mut TaskRecord, records: &[TaskRecord], outcome: TaskOutcome) {
    record.status = if unsettled_children(records, &record.id) {
        TaskStatus::Completing
    } else {
        TaskStatus::Terminal
    };
    record.outcome = Some(outcome);
    record.wait_for.clear();
    record.wake_at_ms = None;
    record.blocking_reason = None;
}
