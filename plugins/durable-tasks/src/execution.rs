use std::collections::BTreeSet;
use std::rc::Rc;

use swarmie_task_contracts::{
    BlockingReason, TaskInvocation, TaskOutcome, TaskRecord, TaskStatus, TaskTransition, TaskWrite,
};
use tembra::{Context, Result, RuntimeError};

use crate::{ownership, registry, scheduler::Engine};

impl Engine {
    pub async fn execute(self: Rc<Self>, id: String, context: Context) -> Result<()> {
        let Some(mut record) = self.get(&context, &id).await? else {
            return Ok(());
        };
        if record.status != TaskStatus::Pending || record.cancellation_requested {
            return Ok(());
        }
        let definition = match registry::definition(&self.registry, &record, context.core()) {
            Ok(definition) => definition,
            Err(reason) => {
                record.status = TaskStatus::Blocked;
                record.blocking_reason = Some(reason);
                self.update(&context, record).await?;
                return Ok(());
            }
        };
        let migrating = record.definition_version != definition.metadata.version;
        let records = self.records(&context).await?;
        if ownership::cancellations(&records).contains(&id) {
            record.cancellation_requested = true;
            self.update(&context, record).await?;
            return Ok(());
        }
        if migrating {
            self.rejected_migrations
                .borrow_mut()
                .insert(id.clone(), definition.operation.clone());
        }
        record.status = TaskStatus::Running;
        record.blocking_reason = None;
        record.attempts = record.attempts.checked_add(1).ok_or_else(|| {
            RuntimeError::invalid("durable_tasks_attempts", "attempt count exhausted")
        })?;
        self.reservations.borrow_mut().insert(id.clone());
        if !self.update(&context, record).await? {
            self.reservations.borrow_mut().remove(&id);
            return Ok(());
        }
        let (handler_context, handler_guard) = context.child_call(context.principal().clone())?;
        self.active
            .borrow_mut()
            .insert(id.clone(), handler_context.cancellation().clone());
        // Reconcile cancellation that committed between reservation and handler admission.
        let admission = self.records(&context).await?;
        let current = self.require(&context, &id).await?;
        if current.cancellation_requested || ownership::cancellations(&admission).contains(&id) {
            let mut cancelled = current;
            cancelled.cancellation_requested = true;
            self.update(&context, cancelled).await?;
            return Ok(());
        }
        let invocation = if migrating {
            TaskInvocation::Migrate {
                task: current,
                target_version: definition.metadata.version,
            }
        } else {
            TaskInvocation::Run {
                task: current,
                children: ownership::children(&records, &id)
                    .into_iter()
                    .cloned()
                    .collect(),
            }
        };
        let call = definition.operation.call_json_tracked(
            &handler_context,
            Some(serde_json::to_value(invocation).map_err(|error| {
                RuntimeError::invalid("durable_tasks_invocation", error.to_string())
            })?),
        );
        let receipt = call.receipt();
        let result = call.await;
        // Detached backend work may retain physical invocation ownership after the public
        // result returns. Never admit another phase until that actual entry has settled.
        receipt.settled().await;
        drop(handler_guard);
        self.active.borrow_mut().remove(&id);
        if context.check(context.host_id()).is_err() || self.failure.borrow().is_some() {
            // Scheduler shutdown expires authority. Preserve Running and checkpoint for
            // process/owner recovery rather than claiming a final write under a dead root
            // or after a fatal scheduling failure with uncertain storage outcome.
            return Ok(());
        }
        let mut latest = self.require(&context, &id).await?;
        if latest.status != TaskStatus::Running {
            return Ok(());
        }
        let records = self.records(&context).await?;
        if latest.cancellation_requested || ownership::cancellations(&records).contains(&id) {
            latest.cancellation_requested = true;
            ownership::settle(
                &mut latest,
                &records,
                TaskOutcome::Aborted {
                    reason: "cancellation requested".into(),
                },
            );
            self.update(&context, latest).await?;
            return Ok(());
        }
        let transition = match result.and_then(|value| {
            tembra::contracts::from_portable_value::<TaskTransition>(&value.ok_or_else(|| {
                RuntimeError::invalid(
                    "durable_tasks_transition",
                    "handler returned no durable transition",
                )
            })?)
            .map_err(|error| RuntimeError::invalid("durable_tasks_transition", error.to_string()))
        }) {
            Ok(transition) => transition,
            Err(error) => {
                if migrating {
                    self.reject_migration(
                        &context,
                        latest,
                        definition.metadata.version,
                        error.to_string(),
                    )
                    .await?;
                } else if matches!(error.reason.as_str(), "result" | "durable_tasks_transition") {
                    self.fault(&context, latest, error.to_string()).await?;
                } else {
                    self.park(&context, latest, error.to_string()).await?;
                }
                return Ok(());
            }
        };
        if migrating {
            let original = latest.clone();
            match transition {
                TaskTransition::Migrate { checkpoint, phase } => {
                    latest.definition_version = definition.metadata.version;
                    latest.checkpoint = checkpoint;
                    latest.phase = phase;
                    latest.status = TaskStatus::Pending;
                    latest.restarts = 0;
                }
                TaskTransition::RejectMigration { reason } => {
                    latest.status = TaskStatus::Blocked;
                    latest.blocking_reason = Some(BlockingReason::MigrationRejected {
                        target_version: definition.metadata.version,
                        detail: reason,
                    });
                }
                _ => {
                    self.reject_migration(
                        &context,
                        latest,
                        definition.metadata.version,
                        "migration must explicitly accept or reject the checkpoint".into(),
                    )
                    .await?;
                    return Ok(());
                }
            }
            if let Err(error) = self.update(&context, latest).await {
                if error.reason == "task_storage_invalid" {
                    self.reject_migration(
                        &context,
                        original,
                        definition.metadata.version,
                        error.to_string(),
                    )
                    .await?;
                } else {
                    return Err(error);
                }
            }
            return Ok(());
        }
        match self
            .apply_transition(&context, latest.clone(), transition)
            .await
        {
            Ok(()) => Ok(()),
            Err(error)
                if error.reason.starts_with("task_storage_invalid")
                    || error.reason == "durable_tasks_transition" =>
            {
                // A protocol fault retains the original checkpoint and decided Faulted
                // outcome while owned children drain. Storage failures propagate unchanged.
                // Storage failures with uncertain outcome must propagate for explicit recovery.
                self.fault(&context, latest, error.to_string()).await
            }
            Err(error) => Err(error),
        }
    }

    async fn park(&self, context: &Context, mut task: TaskRecord, detail: String) -> Result<()> {
        task.status = TaskStatus::Parked;
        task.blocking_reason = Some(BlockingReason::HandlerFailure { detail });
        self.update(context, task).await?;
        Ok(())
    }

    async fn fault(&self, context: &Context, mut task: TaskRecord, detail: String) -> Result<()> {
        let records = self.records(context).await?;
        ownership::settle(&mut task, &records, TaskOutcome::Faulted { error: detail });
        self.update(context, task).await?;
        Ok(())
    }

    async fn reject_migration(
        &self,
        context: &Context,
        mut task: TaskRecord,
        target_version: u32,
        detail: String,
    ) -> Result<()> {
        task.status = TaskStatus::Blocked;
        task.blocking_reason = Some(BlockingReason::MigrationRejected {
            target_version,
            detail,
        });
        self.update(context, task).await?;
        Ok(())
    }

    async fn apply_transition(
        &self,
        context: &Context,
        mut task: TaskRecord,
        transition: TaskTransition,
    ) -> Result<()> {
        let records = self.records(context).await?;
        let mut created = Vec::new();
        task.restarts = 0;
        match transition {
            TaskTransition::Checkpoint { checkpoint, phase } => {
                if checkpoint == task.checkpoint && phase == task.phase {
                    return Err(RuntimeError::invalid(
                        "durable_tasks_transition",
                        "checkpoint transition must describe durable progress",
                    ));
                }
                task.checkpoint = checkpoint;
                task.phase = phase;
                task.status = TaskStatus::Pending;
            }
            TaskTransition::Sleep {
                checkpoint,
                phase,
                wake_at_ms,
            } => {
                task.checkpoint = checkpoint;
                task.phase = phase;
                task.status = TaskStatus::Sleeping;
                task.wake_at_ms = Some(wake_at_ms);
            }
            TaskTransition::Wait {
                checkpoint,
                phase,
                children,
                task_ids,
                policy,
            } => {
                task.checkpoint = checkpoint;
                task.phase = phase;
                task.status = TaskStatus::Waiting;
                task.wait_policy = policy;
                task.wait_for = task_ids;
                let mut selected: BTreeSet<_> = task.wait_for.iter().cloned().collect();
                if selected.len() != task.wait_for.len() {
                    return Err(RuntimeError::invalid(
                        "durable_tasks_transition",
                        "waiting task identities must be unique",
                    ));
                }
                for child in children {
                    if !selected.insert(child.id.clone()) {
                        return Err(RuntimeError::invalid(
                            "durable_tasks_transition",
                            "child identity appears twice in waiting transition",
                        ));
                    }
                    task.wait_for.push(child.id.clone());
                    created.push(TaskWrite {
                        expected_revision: None,
                        record: TaskRecord::new(child, Some(task.id.clone())),
                    });
                }
                if task.wait_for.is_empty() {
                    return Err(RuntimeError::invalid(
                        "durable_tasks_transition",
                        "waiting requires at least one owned child",
                    ));
                }
            }
            TaskTransition::Complete { value } => {
                ownership::settle(&mut task, &records, TaskOutcome::Completed { value })
            }
            TaskTransition::Fail { error } => {
                ownership::settle(&mut task, &records, TaskOutcome::Failed { error })
            }
            TaskTransition::Abort { reason } => {
                ownership::settle(&mut task, &records, TaskOutcome::Aborted { reason })
            }
            TaskTransition::Migrate { .. } | TaskTransition::RejectMigration { .. } => {
                return Err(RuntimeError::invalid(
                    "durable_tasks_transition",
                    "run invocation cannot return a migration response",
                ));
            }
        }
        let revision = task.revision;
        task.revision = revision.checked_add(1).ok_or_else(|| {
            RuntimeError::invalid("durable_tasks_revision", "task revision exhausted")
        })?;
        created.push(TaskWrite {
            expected_revision: Some(revision),
            record: task,
        });
        self.commit(context, created).await?;
        Ok(())
    }
}
