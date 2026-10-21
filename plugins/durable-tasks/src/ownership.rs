use swarmie_task_contracts::{TaskOutcome, TaskRecord, TaskStatus};

pub(crate) fn child_failed(record: &TaskRecord) -> bool {
    matches!(record.status, TaskStatus::Completing | TaskStatus::Terminal)
        && !matches!(record.outcome, Some(TaskOutcome::Completed { .. }))
}

/// The caller supplies a sequence-fenced direct-child family, including terminal outcomes.
/// Independent roots and unrelated terminal history cannot affect this ownership decision.
pub(crate) fn settle(record: &mut TaskRecord, children: &[TaskRecord], outcome: TaskOutcome) {
    record.status = if children.iter().any(|child| !child.is_terminal()) {
        TaskStatus::Completing
    } else {
        TaskStatus::Terminal
    };
    record.outcome = Some(outcome);
    record.wait_for.clear();
    record.wake_at_ms = None;
    record.blocking_reason = None;
}
