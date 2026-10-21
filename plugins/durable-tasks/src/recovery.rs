use std::time::Duration;

use swarmie_task_contracts::{
    BlockingReason, CommitResult, TaskLifecycleOrigin, TaskOutcome, TaskQuery, TaskRecord,
    TaskStatus, WaitPolicy,
};
use tembra::{Context, Result, RuntimeError};

use crate::{
    mutation::Change,
    ownership,
    reads::{ACTIVE, busy, is_contention},
    registry,
    scheduler::{Engine, now_ms},
};

impl Engine {
    /// Seed active identities at startup, then use commit hints and bounded sequence probes.
    /// Retained terminal history is read only through explicit graph and family queries.
    pub async fn reconcile(&self, context: &Context) -> Result<Vec<TaskRecord>> {
        let _gate = self.reconcile_gate.lock().await;
        let startup = !self.work.borrow().initialized;
        let probe_due = self.driver.now() >= self.work.borrow().next_probe;
        if startup || self.work.borrow().rescan || probe_due {
            let sequence = self.sequence(context).await?;
            if startup || self.work.borrow().rescan || sequence != self.work.borrow().sequence {
                let (sequence, records) = self
                    .scan_records(
                        context,
                        TaskQuery {
                            limit: 256,
                            statuses: ACTIVE.to_vec(),
                            ..Default::default()
                        },
                    )
                    .await?;
                if !self.work.borrow_mut().discover(&records, sequence, startup) {
                    return Err(busy());
                }
                if sequence > self.committed_sequence.value()? {
                    self.committed_sequence.replace(context, sequence)?;
                }
            }
            self.work.borrow_mut().next_probe =
                self.driver.now().saturating_add(Duration::from_secs(1));
        }
        self.work.borrow_mut().due(now_ms()?);
        let mut runnable = Vec::new();
        for _ in 0..128 {
            let Some(id) = self.work.borrow_mut().pop() else {
                break;
            };
            match self.reconcile_one(context, &id).await {
                Ok(Some(task)) => runnable.push(task),
                Ok(None) => {}
                Err(error) if is_contention(&error) => {
                    self.work.borrow_mut().schedule(id);
                    break;
                }
                Err(error) => return Err(error),
            }
            if runnable.len() >= self.configuration.max_parallel as usize {
                break;
            }
        }
        Ok(runnable)
    }

    async fn reconcile_one(&self, context: &Context, id: &str) -> Result<Option<TaskRecord>> {
        let record = self.require(context, id).await?;
        if record.is_terminal() {
            self.work.borrow_mut().observe(&record);
            return Ok(None);
        }
        if record.cancellation_requested {
            if self.inflight.borrow().contains(id) {
                if let Some(token) = self.active.borrow().get(id).cloned() {
                    token.cancel(RuntimeError::aborted(
                        "task_cancelled",
                        "owned task cancellation requested",
                    ));
                }
            } else {
                // Intent is monotone. The cancellation sweep rereads its own fenced
                // snapshot; an additional full family read would discard the same children.
                self.cancel_owned(context, id).await?;
            }
            return Ok(None);
        }
        // Root candidates need no ancestry. The managed execution still owns coherent
        // checks before reservation and immediately before handler admission. Owned tasks
        // retain reconciliation ancestry checks so fail-fast cancellation can settle them.
        if record.status == TaskStatus::Pending
            && record.parent.is_none()
            && !record.cancellation_requested
        {
            return Ok(Some(record));
        }
        let family = self.family(context, id).await?;
        let now = now_ms()?;
        let record = family.task.clone();
        let mut next = record.clone();
        let mut change = Change::from_record(&record, TaskLifecycleOrigin::Reconciliation);
        let cancelled = self.cancelled(&family);
        if self.inflight.borrow().contains(id) {
            if cancelled {
                if !record.cancellation_requested {
                    change.origin = TaskLifecycleOrigin::Cancellation;
                    next.cancellation_requested = true;
                    if matches!(
                        self.update(context, next, change).await?,
                        CommitResult::Conflict { .. }
                    ) {
                        self.work.borrow_mut().schedule(id.to_owned());
                        return Ok(None);
                    }
                }
                let token = self.active.borrow().get(id).cloned();
                if let Some(token) = token {
                    token.cancel(RuntimeError::aborted(
                        "task_cancelled",
                        "owned task cancellation requested",
                    ));
                }
            }
            return Ok(None);
        }
        if cancelled {
            self.cancel_owned(context, id).await?;
            return Ok(None);
        } else {
            match record.status {
                TaskStatus::Running => {
                    // Only startup records are recovery candidates. Settled conflicts under
                    // a live owner cannot manufacture process crashes or replay effects.
                    if !self.work.borrow().recover.contains(id) {
                        return Ok(None);
                    }
                    change.origin = TaskLifecycleOrigin::Recovery;
                    if next.restarts < self.configuration.restart_limit {
                        next.restarts += 1;
                    }
                    next.status = if next.restarts >= self.configuration.restart_limit {
                        next.blocking_reason = Some(BlockingReason::RestartLimit);
                        TaskStatus::Parked
                    } else {
                        TaskStatus::Pending
                    };
                }
                TaskStatus::Completing => {
                    if family.children.iter().all(TaskRecord::is_terminal) {
                        next.status = TaskStatus::Terminal;
                    }
                }
                TaskStatus::Waiting => {
                    let selected: Vec<_> = family
                        .children
                        .iter()
                        .filter(|child| record.wait_for.contains(&child.id))
                        .collect();
                    if record.wait_policy == WaitPolicy::FailFast
                        && selected.iter().any(|child| ownership::child_failed(child))
                    {
                        ownership::settle(
                            &mut next,
                            &family.children,
                            TaskOutcome::Failed {
                                error: "owned child failed under fail_fast policy".into(),
                            },
                        );
                    } else if selected.len() == record.wait_for.len()
                        && selected.iter().all(|child| child.is_terminal())
                    {
                        next.status = TaskStatus::Pending;
                        next.wait_for.clear();
                    }
                }
                TaskStatus::Sleeping => {
                    if record.wake_at_ms.is_some_and(|deadline| deadline <= now) {
                        next.status = TaskStatus::Pending;
                        next.wake_at_ms = None;
                    }
                }
                TaskStatus::Blocked => {
                    match registry::definition(&self.registry, &record, context.core()) {
                        Ok(definition) => {
                            let rejected_version = matches!(record.blocking_reason,Some(BlockingReason::MigrationRejected {target_version,..}) if target_version==definition.metadata.version);
                            let rejected = if rejected_version {
                                let mut failures = self.rejected_migrations.borrow_mut();
                                match failures.get(id) {
                                    Some(previous) => previous.same_target(&definition.operation),
                                    None => {
                                        failures.insert(id.to_owned(), definition.operation);
                                        true
                                    }
                                }
                            } else {
                                false
                            };
                            if !rejected {
                                self.rejected_migrations.borrow_mut().remove(id);
                                next.status = TaskStatus::Pending;
                                next.blocking_reason = None;
                            }
                        }
                        Err(reason) => next.blocking_reason = Some(reason),
                    }
                }
                TaskStatus::Pending => return Ok(Some(record)),
                TaskStatus::Parked | TaskStatus::Terminal => {}
            }
        }
        if next != record {
            match self.update(context, next, change).await? {
                CommitResult::Committed { .. } => {
                    self.work.borrow_mut().recover.remove(id);
                }
                CommitResult::Conflict { .. } => self.work.borrow_mut().schedule(id.to_owned()),
            }
        }
        Ok(None)
    }
}
