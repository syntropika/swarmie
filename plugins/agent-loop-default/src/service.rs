use std::rc::Rc;

use swarmie_agent_contracts::{
    AgentLoop, AgentLoopProvider, ContinueTurn, StartTurn, TurnEnd, TurnLookup, TurnRecord,
    on_turn_end,
};
use swarmie_session_contracts::{MESSAGE, SessionStore, SessionStoreInput, SessionStoreRef};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, DurableTasksRef, TaskLookup, TaskSpec, TaskStatus,
};
use tembra::{core, prelude::*};
use tembra_hooks::{HookManager, HookManagerOptions};

use crate::{
    TURN_KIND, journal,
    state::{Checkpoint, decode, encode, invalid, now_ms, task_id},
};

/// Installs one compiled implementation of the Agent Contract and its registered turn handler.
/// The Host selects providers; the loop neither owns storage nor schedules a second executor.
///
/// # Errors
/// Returns Tembra declaration errors. Missing dependencies fail actual installation readiness.
pub fn facet(driver: Rc<dyn core::Driver>) -> Result<NativeFacet> {
    NativeFacet::new("swarmie.agent-loop-default", vec![], move |scope, setup| {
        let hooks = HookManager::new(scope.clone(), driver.clone(), HookManagerOptions::default())?;
        setup.serve_with_options(
            AgentLoop,
            move |_, _, declaration| {
                Ok(Service {
                    hooks: hooks.clone(),
                    tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                    sessions: declaration.get(SessionStore, SessionStoreInput {})?,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        crate::workflow::declare(scope, setup, driver.clone())
    })
}

struct Service {
    tasks: DurableTasksRef,
    sessions: SessionStoreRef,
    hooks: HookManager,
}

impl Service {
    async fn require(&self, turn_id: &str, context: &Context) -> Result<TurnRecord> {
        let task = self
            .tasks
            .with_context(context)
            .get(TaskLookup {
                id: task_id(turn_id),
            })
            .await?
            .ok_or_else(|| invalid("turn does not exist"))?;
        if task.kind != TURN_KIND {
            return Err(invalid("task is not an agent turn"));
        }
        let request: StartTurn = decode(task.input.clone())?;
        if request.turn_id != turn_id {
            return Err(invalid("turn identity differs"));
        }
        Ok(TurnRecord {
            turn_id: turn_id.into(),
            session_id: request.session_id,
            task,
        })
    }

    async fn reconcile_terminal(&self, record: &TurnRecord, context: &Context) -> Result<()> {
        if record.task.is_terminal() {
            let request: StartTurn = decode(record.task.input.clone())?;
            let checkpoint: Checkpoint = decode(record.task.checkpoint.clone())?;
            let turn = checkpoint.turn(&request);
            let events = journal::history(&self.sessions, &request.session_id, context).await?;
            if journal::find(
                &events,
                &crate::state::stable(&request.turn_id, "turn-decision"),
            )
            .is_none()
            {
                let outcome = record
                    .task
                    .outcome
                    .clone()
                    .ok_or_else(|| invalid("terminal task lacks outcome"))?;
                self.hooks
                    .run_series(
                        &on_turn_end()?,
                        TurnEnd {
                            turn: turn.clone(),
                            outcome: outcome.clone(),
                        },
                        context.core(),
                    )
                    .await?;
                journal::append(
                    &self.sessions,
                    &turn,
                    "turn-decision",
                    "swarmie.agent.turn-ended",
                    encode(&outcome)?,
                    context,
                )
                .await?;
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
        let lookup = TaskLookup {
            id: task_id(&request.turn_id),
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
            return self.require(&request.turn_id, &context).await;
        }
        let events = journal::history(&self.sessions, &request.session_id, &context).await?;
        let input = events
            .iter()
            .find(|event| event.event.event_id == request.input_event_id)
            .filter(|event| event.event.kind == MESSAGE && event.event.version == 1)
            .ok_or_else(|| {
                invalid("input must reference a committed version-one session message")
            })?;
        let message: swarmie_session_contracts::Message = decode(input.event.payload.clone())?;
        if message.role != "user" {
            return Err(invalid("turn input must be a user message"));
        }
        let checkpoint = Checkpoint {
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
                definition_version: 1,
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
                    id: task_id(&request.turn_id),
                })
                .await?
            else {
                return Err(error);
            };
            if task.kind != TURN_KIND || task.input != encode(&request)? {
                return Err(error);
            }
        }
        self.require(&request.turn_id, &context).await
    }

    async fn inspect(&self, request: TurnLookup, context: Context) -> Result<Option<TurnRecord>> {
        if self
            .tasks
            .with_context(&context)
            .get(TaskLookup {
                id: task_id(&request.turn_id),
            })
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let record = self.require(&request.turn_id, &context).await?;
        self.reconcile_terminal(&record, &context).await?;
        Ok(Some(record))
    }

    async fn cancel(&self, request: TurnLookup, context: Context) -> Result<TurnRecord> {
        let before = self.require(&request.turn_id, &context).await?;
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
        let record = self.require(&request.turn_id, &context).await?;
        self.reconcile_terminal(&record, &context).await?;
        Ok(record)
    }

    async fn continue_turn(&self, request: ContinueTurn, context: Context) -> Result<TurnRecord> {
        let record = self.require(&request.turn_id, &context).await?;
        let start: StartTurn = decode(record.task.input.clone())?;
        let checkpoint: Checkpoint = decode(record.task.checkpoint.clone())?;
        if let Some(resolution) = request.resolution {
            if record.task.status != TaskStatus::Parked {
                return Err(invalid("reconciliation requires a parked turn"));
            }
            let turn = checkpoint.turn(&start);
            let (operation, kind) = match record.task.phase.as_str() {
                "tool" => (
                    crate::state::stable(
                        &start.turn_id,
                        &format!("tool:{}:{}", checkpoint.step, checkpoint.tool_index),
                    ),
                    "swarmie.agent.tool-confirmed",
                ),
                "model" => (
                    crate::state::stable(&start.turn_id, &format!("model:{}", checkpoint.step)),
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
            let events = journal::history(&self.sessions, &start.session_id, &context).await?;
            let result = journal::find(&events, &resolution.result_event_id)
                .ok_or_else(|| invalid("resolution event is absent"))?;
            if result.version != 1
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
        let record = self.require(&request.turn_id, &context).await?;
        self.reconcile_terminal(&record, &context).await?;
        Ok(record)
    }
}
