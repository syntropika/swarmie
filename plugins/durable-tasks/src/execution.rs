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
    registry::{self, Definition},
    scheduler::Engine,
};

#[derive(Clone)]
enum Decision {
    Transition(TaskTransition),
    Park(String),
    Fault(String),
    RejectMigration(u32, String),
}

/// What a task's managed execution does after one phase settles.
pub(crate) enum Next {
    /// The phase decision was committed or deliberately dropped; the execution ends.
    Done,
    /// The handler asked to continue in place. The settled decision is committed from a fresh
    /// root Context, whose code epoch also selects the definition for the next phase.
    Continue(Box<Continuation>),
}

/// A physically settled continuation decision that is not committed yet.
pub(crate) struct Continuation {
    admitted: TaskRecord,
    checkpoint: serde_json::Value,
    phase: String,
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

fn without_progress() -> RuntimeError {
    RuntimeError::invalid(
        "durable_tasks_transition",
        "checkpoint transition must describe durable progress",
    )
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

    /// A capacity refusal committed nothing, and only outbox consumers can free capacity. The
    /// task is queued again after the contention pause rather than for the next pass: that pass
    /// would be refused again at once, and a scheduling loop that stays ready starves the
    /// Host's timers while nothing can progress.
    async fn retry_after_capacity(&self, context: &Context, id: &str) -> Result<Next> {
        self.contention_pause(context).await?;
        self.schedule(id);
        Ok(Next::Done)
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
    pub async fn execute(self: Rc<Self>, candidate: TaskRecord, context: Context) -> Result<Next> {
        let id = candidate.id.clone();
        let (mut record, cancelled) = if candidate.parent.is_none() {
            let cancelled = candidate.cancellation_requested;
            (candidate, cancelled)
        } else {
            let family = match self.family(&context, &id).await {
                Err(error) if is_contention(&error) => {
                    self.schedule(&id);
                    return Ok(Next::Done);
                }
                result => result?,
            };
            let cancelled = self.cancelled(&family);
            (family.task, cancelled)
        };
        if record.status != TaskStatus::Pending {
            return Ok(Next::Done);
        }
        // Cancellation is judged on a coherent family before reserving execution.
        if cancelled {
            self.schedule(&id);
            return Ok(Next::Done);
        }
        let definition = match registry::definition(&self.registry, &record, context.core()) {
            Ok(definition) => definition,
            Err(reason) => {
                let change = Change::from_record(&record, TaskLifecycleOrigin::Reconciliation);
                record.status = TaskStatus::Blocked;
                record.blocking_reason = Some(reason);
                return match self.update(&context, record, change).await {
                    Ok(CommitResult::Committed { .. }) => Ok(Next::Done),
                    Ok(CommitResult::Conflict { .. }) => {
                        self.schedule(&id);
                        Ok(Next::Done)
                    }
                    Err(error) if error.reason == "task_storage_outbox_full" => {
                        self.retry_after_capacity(&context, &id).await
                    }
                    Err(error) => Err(error),
                };
            }
        };
        let migrating = record.definition_version != definition.metadata.version;
        if migrating {
            self.rejected_migrations
                .borrow_mut()
                .insert(id.clone(), definition.operation.clone());
        }
        let Some(attempts) = record.attempts.checked_add(1) else {
            self.finish_decision(
                &context,
                &record,
                Decision::Fault("attempt count exhausted before handler admission".into()),
                None,
            )
            .await?;
            return Ok(Next::Done);
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
                return Ok(Next::Done);
            }
            Err(error) if error.reason == "task_storage_outbox_full" => {
                // Capacity rejection is definite: no Running reservation was committed.
                self.reservations.borrow_mut().remove(&id);
                return self.retry_after_capacity(&context, &id).await;
            }
            Err(error) => return Err(error),
        }
        self.invoke(&context, &id, definition, migrating).await
    }

    /// Commits a settled continuation decision from this fresh root Context. When the commit
    /// also reserved the next phase, the same managed execution invokes the handler for it.
    pub async fn continue_phase(
        self: Rc<Self>,
        continuation: Box<Continuation>,
        context: Context,
    ) -> Result<Next> {
        let Continuation {
            admitted,
            checkpoint,
            phase,
        } = *continuation;
        let decision = Decision::Transition(TaskTransition::Continue { checkpoint, phase });
        match self
            .finish_decision(&context, &admitted, decision, None)
            .await?
        {
            Some((reserved, definition)) => {
                self.invoke(&context, &reserved.id, definition, false).await
            }
            None => Ok(Next::Done),
        }
    }

    /// Admits and invokes the handler for this owner's committed Running reservation, then
    /// commits the settled decision or hands a continuation back to the managed execution.
    async fn invoke(
        &self,
        context: &Context,
        id: &str,
        definition: Definition,
        migrating: bool,
    ) -> Result<Next> {
        let (handler_context, handler_guard) = context.child_call(context.principal().clone())?;
        self.active
            .borrow_mut()
            .insert(id.to_owned(), handler_context.cancellation().clone());
        let admission = self.settled_family(context, id).await?;
        let admitted = admission.task.clone();
        if admitted.status != TaskStatus::Running {
            return Ok(Next::Done);
        }
        if self.cancelled(&admission) {
            self.finish_decision(
                context,
                &admitted,
                Decision::Park("cancellation before handler admission".into()),
                migrating.then_some(definition.metadata.version),
            )
            .await?;
            return Ok(Next::Done);
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
            self.finish_decision(context, &admitted, Decision::Fault(error.to_string()), None)
                .await?;
            return Ok(Next::Done);
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
        self.active.borrow_mut().remove(id);
        if context.check(context.host_id()).is_err() || self.failure.borrow().is_some() {
            return Ok(Next::Done);
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
            // Only a run invocation can continue. A migration must accept or reject explicitly.
            Ok(TaskTransition::Continue { checkpoint, phase }) if !migrating => {
                return Ok(Next::Continue(Box::new(Continuation {
                    admitted,
                    checkpoint,
                    phase,
                })));
            }
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
            context,
            &admitted,
            decision,
            migrating.then_some(definition.metadata.version),
        )
        .await?;
        Ok(Next::Done)
    }

    /// Selects the definition and attempt for reserving a continuation's next phase in place.
    ///
    /// Returns None, meaning an ordinary Pending checkpoint, when the attempt counter is
    /// exhausted, when another runnable task waits while every execution slot is occupied, or
    /// when the definition resolved at this fresh Context is unavailable or has another version.
    /// Blocking and migration then run through ordinary admission before the next phase.
    fn continuation(&self, context: &Context, task: &TaskRecord) -> Option<(Definition, u32)> {
        let attempts = task.attempts.checked_add(1)?;
        {
            let inflight = self.inflight.borrow();
            if inflight.len() >= self.configuration.max_parallel as usize
                && self.work.borrow().runnable_waiting(&inflight)
            {
                return None;
            }
        }
        let definition = registry::definition(&self.registry, task, context.core()).ok()?;
        (definition.metadata.version == task.definition_version).then_some((definition, attempts))
    }

    /// Commits a continuation decision and the next phase's reservation as one Running write.
    /// Its provenance is a reservation that also accepted a checkpoint, so the revision reports
    /// a phase start followed by a checkpoint.
    async fn reserve_continuation(
        &self,
        context: &Context,
        mut task: TaskRecord,
        checkpoint: serde_json::Value,
        phase: String,
        attempts: u32,
    ) -> Result<CommitResult> {
        if checkpoint == task.checkpoint && phase == task.phase {
            return Err(without_progress());
        }
        let mut change = Change::from_record(&task, TaskLifecycleOrigin::Reservation);
        change.checkpoint_committed = true;
        task.checkpoint = checkpoint;
        task.phase = phase;
        task.status = TaskStatus::Running;
        task.attempts = attempts;
        task.restarts = 0;
        task.blocking_reason = None;
        self.update(context, task, change).await
    }

    /// Commits a physically settled decision without invoking its handler again. Returns the
    /// committed Running record and its definition only when a continuation reserved its next
    /// phase in place; every other outcome ends the managed execution.
    async fn finish_decision(
        &self,
        context: &Context,
        admitted: &TaskRecord,
        mut decision: Decision,
        migration: Option<u32>,
    ) -> Result<Option<(TaskRecord, Definition)>> {
        loop {
            if context.check(context.host_id()).is_err() || self.failure.borrow().is_some() {
                return Ok(None);
            }
            let family = self.settled_family(context, &admitted.id).await?;
            let mut latest = family.task.clone();
            let mut change = Change::from_record(&latest, TaskLifecycleOrigin::Handler);
            if matches!(latest.status, TaskStatus::Terminal | TaskStatus::Completing) {
                return Ok(None);
            }
            let cancelled = self.cancelled(&family);
            if !cancelled
                && !same_phase(&latest, admitted)
                && !matches!(decision, Decision::Park(_))
            {
                decision = Decision::Park("settled phase conflicts with a newer committed record; explicit retry required; external effect may have occurred".into());
            }
            let mut continued = None;
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
                        } else if let TaskTransition::Continue { checkpoint, phase } = transition {
                            if let Some((definition, attempts)) =
                                self.continuation(context, &latest)
                            {
                                continued = Some(definition);
                                self.reserve_continuation(
                                    context, latest, checkpoint, phase, attempts,
                                )
                                .await
                            } else {
                                self.apply_transition(
                                    context,
                                    latest,
                                    TaskTransition::Continue { checkpoint, phase },
                                    &family.children,
                                )
                                .await
                            }
                        } else {
                            self.apply_transition(context, latest, transition, &family.children)
                                .await
                        }
                    }
                }
            };
            match outcome {
                Ok(CommitResult::Committed { records, .. }) => {
                    let Some(definition) = continued else {
                        return Ok(None);
                    };
                    let reserved = records
                        .into_iter()
                        .find(|record| record.id == admitted.id)
                        .ok_or_else(|| {
                            RuntimeError::invalid(
                                "durable_tasks_storage_receipt",
                                "storage commit did not return the continued task",
                            )
                        })?;
                    return Ok(Some((reserved, definition)));
                }
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
                | TaskTransition::Continue { .. }
                | TaskTransition::Sleep { .. }
                | TaskTransition::Wait { .. }
        );
        task.restarts = 0;
        match transition {
            // A continuation that cannot reserve its next phase in place is an ordinary
            // checkpoint: the next phase is admitted by a later reconciliation pass.
            TaskTransition::Checkpoint { checkpoint, phase }
            | TaskTransition::Continue { checkpoint, phase } => {
                if checkpoint == task.checkpoint && phase == task.phase {
                    return Err(without_progress());
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
