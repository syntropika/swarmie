use swarmie_task_contracts::{BlockingReason, TaskOutcome, TaskRecord, TaskStatus, WaitPolicy};
use tembra::{Context, Result, RuntimeError};

use crate::{
    ownership, registry,
    scheduler::{Engine, now_ms},
};

impl Engine {
    /// Reconciles only persisted scheduling semantics. New executions are admitted separately
    /// by the managed pump and invoke Tembra Registry operations under fresh roots.
    pub async fn reconcile(&self, context: &Context) -> Result<Vec<TaskRecord>> {
        let records = self.records(context).await?;
        let cancellations = ownership::cancellations(&records);
        let now = now_ms()?;
        let mut runnable = Vec::new();
        for record in &records {
            if record.is_terminal() {
                continue;
            }
            let mut next = record.clone();
            let children = ownership::children(&records, &record.id);
            // Compute transitive intent before selecting candidates, including descendants
            // beyond one atomic cancellation batch. A newly cancelled ancestor cannot leave
            // a pending grandchild eligible merely because its parent is later in scan order.
            let cancelled = cancellations.contains(&record.id);
            if self.inflight.borrow().contains(&record.id) {
                if cancelled && !record.cancellation_requested {
                    next.cancellation_requested = true;
                    let committed = self.update(context, next).await?;
                    let token = self.active.borrow().get(&record.id).cloned();
                    if committed && let Some(token) = token {
                        token.cancel(RuntimeError::aborted(
                            "task_cancelled",
                            "owned task cancellation requested",
                        ));
                    }
                }
                continue;
            }
            if cancelled {
                next.cancellation_requested = true;
                if record.status == TaskStatus::Completing {
                    if children.iter().all(|child| child.is_terminal()) {
                        next.status = TaskStatus::Terminal;
                    }
                } else {
                    ownership::settle(
                        &mut next,
                        &records,
                        TaskOutcome::Aborted {
                            reason: "cancellation requested".into(),
                        },
                    );
                }
            } else {
                match record.status {
                    TaskStatus::Running => {
                        next.restarts = next.restarts.checked_add(1).ok_or_else(|| {
                            RuntimeError::invalid(
                                "durable_tasks_restarts",
                                "restart count exhausted",
                            )
                        })?;
                        next.status = if next.restarts >= self.configuration.restart_limit {
                            next.blocking_reason = Some(BlockingReason::RestartLimit);
                            TaskStatus::Parked
                        } else {
                            TaskStatus::Pending
                        };
                    }
                    TaskStatus::Completing => {
                        if children.iter().all(|child| child.is_terminal()) {
                            next.status = TaskStatus::Terminal;
                        }
                    }
                    TaskStatus::Waiting => {
                        let selected: Vec<_> = children
                            .iter()
                            .filter(|child| record.wait_for.contains(&child.id))
                            .collect();
                        if record.wait_policy == WaitPolicy::FailFast
                            && selected.iter().any(|child| ownership::child_failed(child))
                        {
                            ownership::settle(
                                &mut next,
                                &records,
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
                        match registry::definition(&self.registry, record, context.core()) {
                            Ok(definition) => {
                                let rejected_version = matches!(
                                    record.blocking_reason,
                                    Some(BlockingReason::MigrationRejected { target_version, .. })
                                        if target_version == definition.metadata.version
                                );
                                let rejected = if rejected_version {
                                    let mut failures = self.rejected_migrations.borrow_mut();
                                    match failures.get(&record.id) {
                                        Some(previous) => {
                                            previous.same_target(&definition.operation)
                                        }
                                        None => {
                                            failures.insert(
                                                record.id.clone(),
                                                definition.operation.clone(),
                                            );
                                            true
                                        }
                                    }
                                } else {
                                    false
                                };
                                if !rejected {
                                    self.rejected_migrations.borrow_mut().remove(&record.id);
                                    next.status = TaskStatus::Pending;
                                    next.blocking_reason = None;
                                }
                            }
                            Err(reason) => next.blocking_reason = Some(reason),
                        }
                    }
                    TaskStatus::Pending => {
                        runnable.push(record.clone());
                    }
                    TaskStatus::Parked | TaskStatus::Terminal => {}
                }
            }
            if next != *record {
                self.update(context, next).await?;
            }
        }
        Ok(runnable)
    }
}
