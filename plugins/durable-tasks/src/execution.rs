use std::collections::BTreeSet;
use std::rc::Rc;

use swarmie_task_contracts::{
    BlockingReason, CommitResult, TaskInvocation, TaskLifecycleEvent, TaskLifecycleOrigin,
    TaskLifecycleTask, TaskOutcome, TaskPhaseKind, TaskPhaseOutcome, TaskRecord, TaskStatus,
    TaskTransition, TaskWrite, task_wire_bytes, validate_task_payload_size,
};
use tembra::{Context, Result, RuntimeError};

use crate::{
    mutation::{Change, Mutation},
    ownership,
    reads::{TaskFamily, is_contention},
    registry,
    scheduler::Engine,
};

#[derive(Clone)]
enum Decision {
    Transition(TaskTransition),
    Park(String),
    Fault(String),
    RejectMigration(u32, String),
}

fn diagnostic(mut detail: String) -> String {
    while task_wire_bytes(&detail).is_ok_and(|bytes| bytes > 4096) {
        let mut end = detail.len() / 2;
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
    }
    detail
}

fn same_phase(left: &TaskRecord, right: &TaskRecord) -> bool {
    left.revision == right.revision
        && left.kind == right.kind
        && left.definition_version == right.definition_version
        && left.phase == right.phase
        && left.input == right.input
        && left.checkpoint == right.checkpoint
}

fn invalid_decision(error: &RuntimeError) -> bool {
    error.reason.starts_with("task_storage_invalid")
        || error.reason == "task_storage_too_large"
        || error.reason == "durable_tasks_transition"
}

impl Engine {
    async fn contention_pause(&self, context: &Context) -> Result<()> {
        context.check(context.host_id())?;
        let timer = self.driver.sleep(std::time::Duration::from_millis(
            self.configuration.poll_interval_ms.max(1),
        ));
        let stopped = context.cancellation().cancelled();
        futures::pin_mut!(timer, stopped);
        futures::future::select(timer, stopped).await;
        context.check(context.host_id())
    }

    // A settled handler decision stays in its original managed execution while relation
    // reads contend. Retrying these reads cannot invoke the external phase again.
    async fn settled_family(&self, context: &Context, id: &str) -> Result<TaskFamily> {
        loop {
            match self.family(context, id).await {
                Err(error) if is_contention(&error) => self.contention_pause(context).await?,
                result => return result,
            }
        }
    }

    /// Reserves and runs one phase of a reconciled candidate.
    ///
    /// Reconciliation returns a root only while it is Pending without cancellation intent, so its
    /// record is the reservation's compare-and-swap base: any later write, including cancellation
    /// intent, changes the revision and makes the reservation conflict and reschedule. An owned
    /// task still reads its coherent family first, because ancestor cancellation does not change
    /// the task's own revision.
    pub async fn execute(self: Rc<Self>, candidate: TaskRecord, context: Context) -> Result<()> {
        let id = candidate.id.clone();
        let (mut record, cancelled) = if candidate.parent.is_none() {
            let cancelled = candidate.cancellation_requested;
            (candidate, cancelled)
        } else {
            let family = match self.family(&context, &id).await {
                Err(error) if is_contention(&error) => {
                    self.schedule(&id);
                    return Ok(());
                }
                result => result?,
            };
            let cancelled = self.cancelled(&family);
            (family.task, cancelled)
        };
        if record.status != TaskStatus::Pending {
            return Ok(());
        }
        // Cancellation is judged on a coherent family before reserving execution.
        if cancelled {
            self.schedule(&id);
            return Ok(());
        }
        let definition = match registry::definition(&self.registry, &record, context.core()) {
            Ok(definition) => definition,
            Err(reason) => {
                let change = Change::from_record(&record, TaskLifecycleOrigin::Reconciliation);
                record.status = TaskStatus::Blocked;
                record.blocking_reason = Some(reason);
                if matches!(
                    self.update(&context, record, change).await?,
                    CommitResult::Conflict { .. }
                ) {
                    self.schedule(&id);
                }
                return Ok(());
            }
        };
        let migrating = record.definition_version != definition.metadata.version;
        if migrating {
            self.rejected_migrations
                .borrow_mut()
                .insert(id.clone(), definition.operation.clone());
        }
        let Some(attempts) = record.attempts.checked_add(1) else {
            return self
                .finish_decision(
                    &context,
                    &record,
                    Decision::Fault("attempt count exhausted before handler admission".into()),
                    None,
                )
                .await;
        };
        let change = Change::from_record(&record, TaskLifecycleOrigin::Reservation);
        record.status = TaskStatus::Running;
        record.blocking_reason = None;
        record.attempts = attempts;
        self.reservations.borrow_mut().insert(id.clone());
        match self.update(&context, record, change).await {
            Ok(CommitResult::Committed { .. }) => {}
            Ok(CommitResult::Conflict { .. }) => {
                self.reservations.borrow_mut().remove(&id);
                self.schedule(&id);
                return Ok(());
            }
            Err(error) if error.reason == "task_storage_outbox_full" => {
                // Capacity rejection is definite: no Running reservation was committed.
                self.reservations.borrow_mut().remove(&id);
                self.schedule(&id);
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        let (handler_context, handler_guard) = context.child_call(context.principal().clone())?;
        self.active
            .borrow_mut()
            .insert(id.clone(), handler_context.cancellation().clone());
        let admission = self.settled_family(&context, &id).await?;
        let admitted = admission.task.clone();
        if admitted.status != TaskStatus::Running {
            return Ok(());
        }
        if self.cancelled(&admission) {
            return self
                .finish_decision(
                    &context,
                    &admitted,
                    Decision::Park("cancellation before handler admission".into()),
                    migrating.then_some(definition.metadata.version),
                )
                .await;
        }
        let invocation = if migrating {
            TaskInvocation::Migrate {
                task: admitted.clone(),
                target_version: definition.metadata.version,
            }
        } else {
            TaskInvocation::Run {
                task: admitted.clone(),
                children: admission.children,
            }
        };
        if let Err(error) = validate_task_payload_size(&invocation) {
            return self
                .finish_decision(
                    &context,
                    &admitted,
                    Decision::Fault(error.to_string()),
                    None,
                )
                .await;
        }
        let call = definition.operation.call_json_tracked(
            &handler_context,
            Some(serde_json::to_value(invocation).map_err(|error| {
                RuntimeError::invalid("durable_tasks_invocation", error.to_string())
            })?),
        );
        let receipt = call.receipt();
        let result = call.await;
        // Physical execution ownership remains valid until the actual Tembra receipt settles.
        receipt.settled().await;
        self.observations.emit(TaskLifecycleEvent::PhaseSettled {
            task: TaskLifecycleTask::from(&admitted),
            invocation: if migrating {
                TaskPhaseKind::Migrate
            } else {
                TaskPhaseKind::Run
            },
            outcome: match &result {
                Ok(_) => TaskPhaseOutcome::Returned,
                Err(error) if error.code == tembra::core::ErrorCode::Aborted => {
                    TaskPhaseOutcome::Cancelled
                }
                Err(_) => TaskPhaseOutcome::Rejected,
            },
        });
        drop(handler_guard);
        self.active.borrow_mut().remove(&id);
        if context.check(context.host_id()).is_err() || self.failure.borrow().is_some() {
            return Ok(());
        }
        let result = result.and_then(|value| {
            tembra::contracts::from_portable_value::<TaskTransition>(&value.ok_or_else(|| {
                RuntimeError::invalid(
                    "durable_tasks_transition",
                    "handler returned no durable transition",
                )
            })?)
            .map_err(|error| RuntimeError::invalid("durable_tasks_transition", error.to_string()))
        });
        let decision = match result {
            Ok(transition) => Decision::Transition(transition),
            Err(error) if migrating => {
                Decision::RejectMigration(definition.metadata.version, error.to_string())
            }
            Err(error)
                if matches!(error.reason.as_str(), "result" | "durable_tasks_transition") =>
            {
                Decision::Fault(error.to_string())
            }
            Err(error) => Decision::Park(error.to_string()),
        };
        self.finish_decision(
            &context,
            &admitted,
            decision,
            migrating.then_some(definition.metadata.version),
        )
        .await
    }

    async fn finish_decision(
        &self,
        context: &Context,
        admitted: &TaskRecord,
        mut decision: Decision,
        migration: Option<u32>,
    ) -> Result<()> {
        loop {
            if context.check(context.host_id()).is_err() || self.failure.borrow().is_some() {
                return Ok(());
            }
            let family = self.settled_family(context, &admitted.id).await?;
            let mut latest = family.task.clone();
            let mut change = Change::from_record(&latest, TaskLifecycleOrigin::Handler);
            if matches!(latest.status, TaskStatus::Terminal | TaskStatus::Completing) {
                return Ok(());
            }
            let cancelled = self.cancelled(&family);
            if !cancelled
                && !same_phase(&latest, admitted)
                && !matches!(decision, Decision::Park(_))
            {
                decision = Decision::Park("settled phase conflicts with a newer committed record; explicit retry required; external effect may have occurred".into());
            }
            let outcome = if cancelled {
                change.origin = TaskLifecycleOrigin::Cancellation;
                latest.cancellation_requested = true;
                ownership::settle(
                    &mut latest,
                    &family.children,
                    TaskOutcome::Aborted {
                        reason: "cancellation requested".into(),
                    },
                );
                self.update(context, latest, change).await
            } else {
                match decision.clone() {
                    Decision::Park(detail) => {
                        latest.status = TaskStatus::Parked;
                        latest.wake_at_ms = None;
                        latest.wait_for.clear();
                        latest.blocking_reason = Some(BlockingReason::HandlerFailure {
                            detail: diagnostic(detail),
                        });
                        self.update(context, latest, change).await
                    }
                    Decision::Fault(detail) => {
                        ownership::settle(
                            &mut latest,
                            &family.children,
                            TaskOutcome::Faulted {
                                error: diagnostic(detail),
                            },
                        );
                        self.update(context, latest, change).await
                    }
                    Decision::RejectMigration(target_version, detail) => {
                        latest.status = TaskStatus::Blocked;
                        latest.blocking_reason = Some(BlockingReason::MigrationRejected {
                            target_version,
                            detail: diagnostic(detail),
                        });
                        self.update(context, latest, change).await
                    }
                    Decision::Transition(transition) => {
                        if let Some(target_version) = migration {
                            match transition {
                                TaskTransition::Migrate { checkpoint, phase } => {
                                    change.checkpoint_committed = true;
                                    latest.definition_version = target_version;
                                    latest.checkpoint = checkpoint;
                                    latest.phase = phase;
                                    latest.status = TaskStatus::Pending;
                                    latest.restarts = 0;
                                    self.update(context, latest, change).await
                                }
                                TaskTransition::RejectMigration { reason } => {
                                    decision = Decision::RejectMigration(target_version, reason);
                                    continue;
                                }
                                _ => {
                                    decision = Decision::RejectMigration(
                                        target_version,
                                        "migration must explicitly accept or reject the checkpoint"
                                            .into(),
                                    );
                                    continue;
                                }
                            }
                        } else {
                            self.apply_transition(context, latest, transition, &family.children)
                                .await
                        }
                    }
                }
            };
            match outcome {
                Ok(CommitResult::Committed { .. }) => return Ok(()),
                Ok(CommitResult::Conflict { .. }) => {
                    // A known atomic refusal cannot justify replaying an external effect.
                    decision = Decision::Park("settled phase transaction conflicted; checkpoint preserved; explicit retry required; external effect may have occurred".into());
                    self.contention_pause(context).await?;
                }
                Err(error) if error.reason == "task_storage_outbox_full" => {
                    // Capacity refusal committed nothing. Retain this physically settled
                    // decision while consumers acknowledge/compact; never replay its effect.
                    self.contention_pause(context).await?;
                }
                Err(error) if invalid_decision(&error) => {
                    if error.reason == "task_storage_invalid" {
                        let refreshed = self.settled_family(context, &admitted.id).await?;
                        // A child can be inserted without changing the parent's revision.
                        // A refused terminal write then needs fresh ownership settlement,
                        // retaining the actual handler decision rather than replaying it.
                        if same_phase(&refreshed.task, &family.task)
                            && refreshed.children != family.children
                        {
                            self.contention_pause(context).await?;
                            continue;
                        }
                    }
                    decision = if let Some(version) = migration {
                        Decision::RejectMigration(version, error.to_string())
                    } else {
                        Decision::Fault(error.to_string())
                    };
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn apply_transition(
        &self,
        context: &Context,
        mut task: TaskRecord,
        transition: TaskTransition,
        children: &[TaskRecord],
    ) -> Result<CommitResult> {
        let mut created = Vec::new();
        let mut change = Change::from_record(&task, TaskLifecycleOrigin::Handler);
        change.checkpoint_committed = matches!(
            &transition,
            TaskTransition::Checkpoint { .. }
                | TaskTransition::Sleep { .. }
                | TaskTransition::Wait { .. }
        );
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
                    created.push(Mutation::create(TaskRecord::new(
                        child,
                        Some(task.id.clone()),
                    )));
                }
                if task.wait_for.is_empty() {
                    return Err(RuntimeError::invalid(
                        "durable_tasks_transition",
                        "waiting requires at least one owned child",
                    ));
                }
            }
            TaskTransition::Complete { value } => {
                ownership::settle(&mut task, children, TaskOutcome::Completed { value })
            }
            TaskTransition::Fail { error } => {
                ownership::settle(&mut task, children, TaskOutcome::Failed { error })
            }
            TaskTransition::Abort { reason } => {
                ownership::settle(&mut task, children, TaskOutcome::Aborted { reason })
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
        created.push(Mutation {
            write: TaskWrite {
                provenance: None,
                expected_revision: Some(revision),
                record: task,
            },
            change,
        });
        self.commit(context, created).await
    }
}
