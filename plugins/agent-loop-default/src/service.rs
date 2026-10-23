use std::rc::Rc;

use futures::FutureExt;
use swarmie_agent_contracts::{
    AgentLoop, AgentLoopProvider, AgentObservations, ContinueTurn, StartTurn, TurnEnd, TurnLookup,
    TurnRecord,
};
use swarmie_session_contracts::{
    MESSAGE, SessionLookup, SessionStore, SessionStoreInput, SessionStoreRef,
};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, DurableTasksRef, TaskLookup, TaskSpec, TaskStatus,
    TaskStorage, TaskStorageInput,
};
use swarmie_task_session_contracts::{
    BindingDisposition, BindingKey, PrepareBinding, SessionDestination, TaskSessionBinding,
    TaskSessionBindings, TaskSessionBindingsInput,
};
use tembra::{core, prelude::*};

use crate::{
    TURN_KIND,
    admission::{Admission, Admitted},
    journal,
    observers::{Monitor, Observation, ObserverDispatcher},
    state::{Checkpoint, TurnIdentity, decode, encode, invalid, now_ms, task_id},
};

/// Installs one compiled implementation of the Agent Contract and its registered turn handler.
/// The Host selects providers; the loop neither owns storage nor schedules a second executor.
///
/// # Errors
/// Returns Tembra declaration errors. Missing dependencies fail actual installation readiness.
pub fn facet(driver: Rc<dyn core::Driver>) -> Result<NativeFacet> {
    NativeFacet::new("swarmie.agent-loop-default", vec![], move |scope, setup| {
        let observers = ObserverDispatcher::new(scope.clone(), driver.clone())?;
        let monitor = observers.clone();
        setup.serve_with_options(
            AgentObservations,
            move |_, _, declaration| {
                let pipelines = monitor.declare(declaration.core_mut())?;
                let activity = monitor.clone();
                declaration.core_mut().task(move |scope| {
                    activity.clone().run(scope, pipelines.clone()).boxed_local()
                })?;
                Ok(Monitor(monitor.clone()))
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        let service_observers = observers.clone();
        setup.serve_with_options(
            AgentLoop,
            move |_, _, declaration| {
                let sessions = declaration.get(SessionStore, SessionStoreInput {})?;
                Ok(Service {
                    observers: service_observers.clone(),
                    // Keep the diagnostics service's managed observation activity live.
                    _monitor: declaration.get(
                        AgentObservations,
                        swarmie_agent_contracts::AgentObservationsInput {},
                    )?,
                    tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                    admission: Admission::new(
                        declaration.get(TaskSessionBindings, TaskSessionBindingsInput {})?,
                        declaration.get(TaskStorage, TaskStorageInput {})?,
                        sessions.clone(),
                    ),
                    sessions,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        crate::workflow::declare(scope, setup, driver.clone(), observers)
    })
}

struct Service {
    tasks: DurableTasksRef,
    sessions: SessionStoreRef,
    admission: Admission,
    observers: Rc<ObserverDispatcher>,
    _monitor: swarmie_agent_contracts::AgentObservationsRef,
}

fn routed(disposition: BindingDisposition) -> Result<TaskSessionBinding> {
    match disposition {
        BindingDisposition::Routed { binding } => Ok(*binding),
        BindingDisposition::Excluded { .. } => Err(RuntimeError::new(
            core::ErrorCode::Forbidden,
            "agent_binding_excluded",
            "an excluded task cannot admit an agent turn",
        )),
    }
}

/// Lookup reads historical intent, so its destination and immutable submission must be
/// checked before admit can use it. No retry replaces a retained deadline or initial state.
fn validate_submission(
    binding: &TaskSessionBinding,
    key: &BindingKey,
    destination: &SessionDestination,
    request: &StartTurn,
    input_sequence: u64,
) -> Result<()> {
    let initial: Checkpoint = decode(binding.task.checkpoint.clone())?;
    initial.validate_identity(request)?;
    if &binding.key != key
        || &binding.destination != destination
        || binding.routing_version != 1
        || binding.root_task_id != key.task_id
        || binding.parent_task_id.is_some()
        || binding.task.id != key.task_id
        || binding.task.kind != TURN_KIND
        || binding.task.definition_version != 2
        || binding.task.input != encode(request)?
        || binding.task.phase != "start"
        || initial.input_sequence != input_sequence
        || initial.started_at_ms == 0
        || initial.step != 0
        || initial.tool_index != 0
        || initial.tool_calls != 0
        || initial.output_tokens != 0
        || initial.request_event.is_some()
        || initial.response_event.is_some()
        || initial.tool_event.is_some()
        || initial.ending.is_some()
    {
        return Err(invalid(
            "binding intent differs from the admitted turn submission",
        ));
    }
    Ok(())
}

impl Service {
    async fn require_task(
        &self,
        identity: &impl TurnIdentity,
        context: &Context,
    ) -> Result<TurnRecord> {
        let task = self
            .tasks
            .with_context(context)
            .get(TaskLookup {
                id: task_id(identity),
            })
            .await?
            .ok_or_else(|| invalid("turn does not exist"))?;
        if task.kind != TURN_KIND || task.definition_version != 2 {
            return Err(invalid("task is not an agent turn"));
        }
        let request: StartTurn = decode(task.input.clone())?;
        let checkpoint: Checkpoint = decode(task.checkpoint.clone())?;
        checkpoint.validate_identity(&request)?;
        if request.turn_id != identity.turn_id()
            || request.session_id != identity.session_id()
            || request.conversation_id != identity.conversation_id()
        {
            return Err(invalid("turn identity differs"));
        }
        Ok(TurnRecord {
            turn_id: request.turn_id.clone(),
            conversation_id: request.conversation_id,
            session_id: request.session_id,
            task,
        })
    }

    /// Reads the turn and confirms its routed association once for this control call.
    async fn confirm(
        &self,
        identity: &impl TurnIdentity,
        context: &Context,
    ) -> Result<(TurnRecord, Admitted)> {
        let record = self.require_task(identity, context).await?;
        let request: StartTurn = decode(record.task.input.clone())?;
        let checkpoint: Checkpoint = decode(record.task.checkpoint.clone())?;
        let admitted = self
            .admission
            .confirm(&checkpoint.turn(&request), context)
            .await?;
        Ok((record, admitted))
    }

    /// Records the authoritative terminal outcome through this control call's confirmation.
    /// History is read here, after any transition this call requested.
    async fn reconcile_terminal(
        &self,
        record: &TurnRecord,
        admitted: &Admitted,
        context: &Context,
    ) -> Result<()> {
        if record.task.is_terminal() {
            let request: StartTurn = decode(record.task.input.clone())?;
            let checkpoint: Checkpoint = decode(record.task.checkpoint.clone())?;
            let turn = checkpoint.turn(&request);
            let history = journal::history(&self.sessions, admitted, context).await?;
            if journal::find(
                history.events()?,
                &crate::state::stable(&request, "turn-decision"),
            )
            .is_none()
            {
                let outcome = record
                    .task
                    .outcome
                    .clone()
                    .ok_or_else(|| invalid("terminal task lacks outcome"))?;
                journal::append(
                    &self.sessions,
                    admitted,
                    &turn,
                    "turn-decision",
                    "swarmie.agent.turn-ended",
                    encode(&outcome)?,
                    context,
                )
                .await?;
                self.observers.enqueue(Observation::Turn(TurnEnd {
                    turn: turn.clone(),
                    outcome,
                }));
            }
            journal::append(
                &self.sessions,
                admitted,
                &turn,
                "task-settled",
                "swarmie.agent.task-settled",
                encode(&record.task.outcome)?,
                context,
            )
            .await?;
        }
        Ok(())
    }
}

impl AgentLoopProvider for Service {
    async fn start(&self, request: StartTurn, context: Context) -> Result<TurnRecord> {
        request.limits.validate()?;
        swarmie_session_contracts::identity(&request.turn_id)?;
        swarmie_session_contracts::identity(&request.session_id)?;
        swarmie_session_contracts::identity(&request.conversation_id)?;
        let lookup = TaskLookup {
            id: task_id(&request),
        };
        if let Some(task) = self
            .tasks
            .with_context(&context)
            .get(lookup.clone())
            .await?
        {
            if task.kind != TURN_KIND || task.input != encode(&request)? {
                return Err(invalid("turn identity already has different submission"));
            }
            return Ok(self.confirm(&request, &context).await?.0);
        }
        let session = self
            .sessions
            .with_context(&context)
            .open(SessionLookup {
                session_id: request.session_id.clone(),
            })
            .await?
            .ok_or_else(|| invalid("admission session is absent"))?;
        let input = journal::input(
            &self.sessions,
            &request,
            &request.input_event_id,
            &session.store_id,
            &context,
        )
        .await?
        .filter(|event| event.event.kind == MESSAGE && event.event.version == 1)
        .ok_or_else(|| {
            invalid("input must reference a committed message in this session and conversation")
        })?;
        let message: swarmie_session_contracts::Message = decode(input.event.payload.clone())?;
        if message.role != "user" {
            return Err(invalid("turn input must be a user message"));
        }
        let key = self.admission.key(&lookup.id, &context).await?;
        let destination = SessionDestination {
            session_store_id: session.store_id,
            session_id: request.session_id.clone(),
            conversation_id: request.conversation_id.clone(),
            turn_id: request.turn_id.clone(),
        };
        let checkpoint = Checkpoint {
            version: 2,
            session_id: request.session_id.clone(),
            conversation_id: request.conversation_id.clone(),
            turn_id: request.turn_id.clone(),
            started_at_ms: now_ms()?,
            input_sequence: input.sequence,
            ..Default::default()
        };
        let specification = TaskSpec {
            id: lookup.id.clone(),
            kind: TURN_KIND.into(),
            definition_version: 2,
            input: encode(&request)?,
            checkpoint: encode(&checkpoint)?,
            phase: "start".into(),
        };
        let pending = match self
            .admission
            .bindings
            .with_context(&context)
            .lookup(key.clone())
            .await?
        {
            Some(disposition) => routed(disposition)?,
            None => {
                let prepared = self
                    .admission
                    .bindings
                    .with_context(&context)
                    .prepare(PrepareBinding {
                        task_store_id: key.task_store_id.clone(),
                        task: specification,
                        destination: destination.clone(),
                        routing_version: 1,
                    })
                    .await;
                match prepared {
                    Ok(binding) => binding,
                    Err(error) => {
                        // Concurrent exact StartTurn calls can propose different wall-clock
                        // origins. Reuse the winning immutable intent and its deadline.
                        let Some(disposition) = self
                            .admission
                            .bindings
                            .with_context(&context)
                            .lookup(key.clone())
                            .await?
                        else {
                            return Err(error);
                        };
                        routed(disposition)?
                    }
                }
            }
        };
        validate_submission(&pending, &key, &destination, &request, input.sequence)?;
        // The integration capability owns creation and uncertain-create reconciliation.
        // No runnable task is submitted before its complete durable intent exists.
        if let Err(error) = self
            .admission
            .bindings
            .with_context(&context)
            .admit(key)
            .await
            && self
                .tasks
                .with_context(&context)
                .get(lookup)
                .await?
                .is_none()
        {
            return Err(error);
        }
        // A committed task may already be advancing. confirm() confirms its binding
        // and compares immutable input; mutable phase/checkpoint are not guessed.
        Ok(self.confirm(&request, &context).await?.0)
    }

    async fn inspect(&self, request: TurnLookup, context: Context) -> Result<Option<TurnRecord>> {
        if self
            .tasks
            .with_context(&context)
            .get(TaskLookup {
                id: task_id(&request),
            })
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let (record, admitted) = self.confirm(&request, &context).await?;
        self.reconcile_terminal(&record, &admitted, &context)
            .await?;
        Ok(Some(record))
    }

    async fn cancel(&self, request: TurnLookup, context: Context) -> Result<TurnRecord> {
        let before = self.require_task(&request, &context).await?;
        let start: StartTurn = decode(before.task.input.clone())?;
        let checkpoint: Checkpoint = decode(before.task.checkpoint.clone())?;
        self.admission
            .require_source(&checkpoint.turn(&start), &before.task, &context)
            .await?;
        // Logical source cancellation must not wait for an unavailable destination.
        // Durable Tasks owns the transition, emergency cancellation and its outbox evidence.
        self.tasks
            .with_context(&context)
            .cancel(TaskLookup { id: before.task.id })
            .await?;
        let record = self.require_task(&request, &context).await?;
        // Do not await destination publication here, including unresponsive providers.
        // The integration publisher owns delivery of the authoritative source fact.
        Ok(record)
    }

    async fn continue_turn(&self, request: ContinueTurn, context: Context) -> Result<TurnRecord> {
        let (record, admitted) = self.confirm(&request, &context).await?;
        let start: StartTurn = decode(record.task.input.clone())?;
        let checkpoint: Checkpoint = decode(record.task.checkpoint.clone())?;
        if let Some(resolution) = &request.resolution {
            if record.task.status != TaskStatus::Parked {
                return Err(invalid("reconciliation requires a parked turn"));
            }
            let turn = checkpoint.turn(&start);
            let (operation, kind) = match record.task.phase.as_str() {
                "tool" => (
                    crate::state::stable(
                        &start,
                        &format!("tool:{}:{}", checkpoint.step, checkpoint.tool_index),
                    ),
                    "swarmie.agent.tool-confirmed",
                ),
                "model" => (
                    crate::state::stable(&start, &format!("model:{}", checkpoint.step)),
                    "swarmie.agent.model-provider-confirmed",
                ),
                _ => {
                    return Err(invalid(
                        "this parked phase has no reconcilable external operation",
                    ));
                }
            };
            if resolution.operation_id != operation {
                return Err(invalid("resolution refers to a different operation"));
            }
            let history = journal::history(&self.sessions, &admitted, &context).await?;
            let result = journal::find(history.events()?, &resolution.result_event_id)
                .ok_or_else(|| invalid("resolution event is absent"))?;
            if result.version != 2
                || result.correlation.conversation_id.as_deref() != Some(&start.conversation_id)
                || result.correlation.turn_id.as_deref() != Some(&request.turn_id)
                || result.correlation.task_id.as_deref() != Some(&record.task.id)
                || result.payload["operation_id"] != resolution.operation_id
                || result.kind != kind
            {
                return Err(invalid("resolution does not confirm this operation"));
            }
            let suffix = format!("resolution:{}", resolution.operation_id);
            journal::append(
                &self.sessions,
                &admitted,
                &turn,
                &suffix,
                "swarmie.agent.operation-resolution",
                encode(&resolution)?,
                &context,
            )
            .await?;
        }
        if matches!(record.task.status, TaskStatus::Parked | TaskStatus::Blocked) {
            self.tasks
                .with_context(&context)
                .retry(TaskLookup { id: record.task.id })
                .await?;
        } else {
            self.tasks.with_context(&context).resume().await?;
        }
        // The transition above may advance the turn, so read it again; this call's
        // confirmation still covers the terminal reconciliation of the same turn.
        let record = self.require_task(&request, &context).await?;
        self.reconcile_terminal(&record, &admitted, &context)
            .await?;
        Ok(record)
    }
}
