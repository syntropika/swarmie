use std::rc::Rc;

use futures::FutureExt;
use swarmie_agent_contracts::{
    AgentLoop, AgentLoopProvider, AgentObservations, ContinueTurn, StartTurn, TurnEnd, TurnLookup,
    TurnRecord,
};
use swarmie_session_contracts::{MESSAGE, SessionStore, SessionStoreInput, SessionStoreRef};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, DurableTasksRef, TaskLookup, TaskSpec, TaskStatus,
};
use tembra::{core, prelude::*};

use crate::{
    TURN_KIND, journal,
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
                let activity = monitor.clone();
                declaration
                    .core_mut()
                    .task(move |scope| activity.clone().run(scope).boxed_local())?;
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
                Ok(Service {
                    observers: service_observers.clone(),
                    // Keep the diagnostics service's managed observation activity live.
                    _monitor: declaration.get(
                        AgentObservations,
                        swarmie_agent_contracts::AgentObservationsInput {},
                    )?,
                    tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                    sessions: declaration.get(SessionStore, SessionStoreInput {})?,
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
    observers: Rc<ObserverDispatcher>,
    _monitor: swarmie_agent_contracts::AgentObservationsRef,
}

impl Service {
    async fn require(&self, identity: &impl TurnIdentity, context: &Context) -> Result<TurnRecord> {
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

    async fn reconcile_terminal(&self, record: &TurnRecord, context: &Context) -> Result<()> {
        if record.task.is_terminal() {
            let request: StartTurn = decode(record.task.input.clone())?;
            let checkpoint: Checkpoint = decode(record.task.checkpoint.clone())?;
            let turn = checkpoint.turn(&request);
            let events = journal::history(&self.sessions, &request, context).await?;
            if journal::find(&events, &crate::state::stable(&request, "turn-decision")).is_none() {
                let outcome = record
                    .task
                    .outcome
                    .clone()
                    .ok_or_else(|| invalid("terminal task lacks outcome"))?;
                journal::append(
                    &self.sessions,
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
            return self.require(&request, &context).await;
        }
        let input = journal::input(&self.sessions, &request, &request.input_event_id, &context)
            .await?
            .filter(|event| event.event.kind == MESSAGE && event.event.version == 1)
            .ok_or_else(|| {
                invalid("input must reference a committed message in this session and conversation")
            })?;
        let message: swarmie_session_contracts::Message = decode(input.event.payload.clone())?;
        if message.role != "user" {
            return Err(invalid("turn input must be a user message"));
        }
        let checkpoint = Checkpoint {
            version: 2,
            session_id: request.session_id.clone(),
            conversation_id: request.conversation_id.clone(),
            turn_id: request.turn_id.clone(),
            started_at_ms: now_ms()?,
            input_sequence: input.sequence,
            ..Default::default()
        };
        let created = self
            .tasks
            .with_context(&context)
            .create(TaskSpec {
                id: lookup.id,
                kind: TURN_KIND.into(),
                definition_version: 2,
                input: encode(&request)?,
                checkpoint: encode(&checkpoint)?,
                phase: "start".into(),
            })
            .await;
        if let Err(error) = created {
            // A concurrent exact submission or lost create acknowledgement is reconciled by ID.
            let Some(task) = self
                .tasks
                .with_context(&context)
                .get(TaskLookup {
                    id: task_id(&request),
                })
                .await?
            else {
                return Err(error);
            };
            if task.kind != TURN_KIND || task.input != encode(&request)? {
                return Err(error);
            }
        }
        self.require(&request, &context).await
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
        let record = self.require(&request, &context).await?;
        self.reconcile_terminal(&record, &context).await?;
        Ok(Some(record))
    }

    async fn cancel(&self, request: TurnLookup, context: Context) -> Result<TurnRecord> {
        let before = self.require(&request, &context).await?;
        let start: StartTurn = decode(before.task.input.clone())?;
        let mut checkpoint: Checkpoint = decode(before.task.checkpoint.clone())?;
        checkpoint.step = 0; // Cancellation batch identity cannot depend on task progress.
        journal::append(
            &self.sessions,
            &checkpoint.turn(&start),
            "cancel-requested",
            "swarmie.agent.cancellation-requested",
            serde_json::json!({}),
            &context,
        )
        .await?;
        self.tasks
            .with_context(&context)
            .cancel(TaskLookup { id: before.task.id })
            .await?;
        let record = self.require(&request, &context).await?;
        self.reconcile_terminal(&record, &context).await?;
        Ok(record)
    }

    async fn continue_turn(&self, request: ContinueTurn, context: Context) -> Result<TurnRecord> {
        let record = self.require(&request, &context).await?;
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
            let events = journal::history(&self.sessions, &start, &context).await?;
            let result = journal::find(&events, &resolution.result_event_id)
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
        let record = self.require(&request, &context).await?;
        self.reconcile_terminal(&record, &context).await?;
        Ok(record)
    }
}
