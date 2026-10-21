//! A distinct fixture-only AgentLoop implementation that completes confirmed final answers.
//! It shares only public Contracts and explicitly admits the default version-one finalization
//! checkpoints. Other phases are refused rather than guessed or silently migrated.

use swarmie_agent_contracts::*;
use swarmie_model_contracts::{ContentPart, GenerateResult};
use swarmie_session_contracts::{EventQuery, SessionStore, SessionStoreInput, SessionStoreRef};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};

const KIND: &str = "swarmie.agent-turn";

#[tembra::contract(
    local,
    name = "swarmie.acceptance-replacement-agent-handler",
    version = 1
)]
trait ReplacementHandler {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

pub fn facet() -> Result<NativeFacet> {
    NativeFacet::new("fixture.replacement-agent-loop", vec![], |_, setup| {
        setup.serve_with_options(
            AgentLoop,
            |_, _, declaration| {
                Ok(Control {
                    tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        setup.serve_with_options(
            ReplacementHandler,
            |_, _, declaration| {
                declaration.core_mut().register_method(
                    &task_definitions()?,
                    KIND,
                    serde_json::json!({"kind":KIND,"version":1}),
                    "step",
                )?;
                Ok(Finalizer {
                    sessions: declaration.get(SessionStore, SessionStoreInput {})?,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Control {
    tasks: DurableTasksRef,
}

impl Control {
    async fn find(&self, turn_id: &str, context: &Context) -> Result<Option<TurnRecord>> {
        let mut query = TaskQuery::default();
        loop {
            let page = self
                .tasks
                .with_context(context)
                .graph(query.clone())
                .await?;
            for task in page.records {
                if task.kind == KIND && task.input["turn_id"] == turn_id {
                    let request: StartTurn =
                        serde_json::from_value(task.input.clone()).map_err(|error| {
                            RuntimeError::invalid("fixture_replacement_input", error.to_string())
                        })?;
                    return Ok(Some(TurnRecord {
                        turn_id: turn_id.into(),
                        session_id: request.session_id,
                        task,
                    }));
                }
            }
            let Some(cursor) = page.next else {
                break;
            };
            query.cursor = Some(cursor);
        }
        Ok(None)
    }
}

impl AgentLoopProvider for Control {
    async fn start(&self, request: StartTurn, context: Context) -> Result<TurnRecord> {
        let record = self
            .find(&request.turn_id, &context)
            .await?
            .ok_or_else(|| {
                RuntimeError::invalid(
                    "fixture_replacement_scope",
                    "this recovery implementation admits existing confirmed turns",
                )
            })?;
        if record.task.input
            != serde_json::to_value(request).map_err(|error| {
                RuntimeError::invalid("fixture_replacement_input", error.to_string())
            })?
        {
            return Err(RuntimeError::invalid(
                "fixture_replacement_identity",
                "submission differs from the saved turn",
            ));
        }
        Ok(record)
    }

    async fn inspect(&self, request: TurnLookup, context: Context) -> Result<Option<TurnRecord>> {
        self.find(&request.turn_id, &context).await
    }

    async fn cancel(&self, request: TurnLookup, context: Context) -> Result<TurnRecord> {
        let record = self
            .find(&request.turn_id, &context)
            .await?
            .ok_or_else(|| {
                RuntimeError::invalid("fixture_replacement_missing", "turn is absent")
            })?;
        self.tasks
            .with_context(&context)
            .cancel(TaskLookup { id: record.task.id })
            .await?;
        self.find(&request.turn_id, &context)
            .await?
            .ok_or_else(|| RuntimeError::invalid("fixture_replacement_missing", "turn is absent"))
    }

    async fn continue_turn(&self, request: ContinueTurn, context: Context) -> Result<TurnRecord> {
        if request.resolution.is_some() {
            return Err(RuntimeError::invalid(
                "fixture_replacement_scope",
                "this finalizer does not reconcile external effects",
            ));
        }
        self.tasks.with_context(&context).resume().await?;
        self.find(&request.turn_id, &context)
            .await?
            .ok_or_else(|| RuntimeError::invalid("fixture_replacement_missing", "turn is absent"))
    }
}

struct Finalizer {
    sessions: SessionStoreRef,
}

impl ReplacementHandlerProvider for Finalizer {
    async fn step(&self, invocation: TaskInvocation, context: Context) -> Result<TaskTransition> {
        let task = match invocation {
            TaskInvocation::Run { task, .. } => task,
            TaskInvocation::Migrate {
                task,
                target_version,
            } => {
                return if target_version == 1 && task.definition_version == 1 {
                    Ok(TaskTransition::Migrate {
                        checkpoint: task.checkpoint,
                        phase: task.phase,
                    })
                } else {
                    Ok(TaskTransition::RejectMigration {
                        reason: "replacement supports only checkpoint version one".into(),
                    })
                };
            }
        };
        if !matches!(task.phase.as_str(), "model-result" | "step-end" | "finish") {
            return Err(RuntimeError::invalid(
                "fixture_replacement_scope",
                "checkpoint is outside finalization scope",
            ));
        }
        let request: StartTurn = serde_json::from_value(task.input).map_err(|error| {
            RuntimeError::invalid("fixture_replacement_input", error.to_string())
        })?;
        let response_id = task.checkpoint["response_event"].as_str().ok_or_else(|| {
            RuntimeError::invalid(
                "fixture_replacement_checkpoint",
                "confirmed response reference is absent",
            )
        })?;
        let mut query = EventQuery::new(&request.session_id);
        loop {
            let page = self
                .sessions
                .with_context(&context)
                .events(query.clone())
                .await?;
            if let Some(entry) = page
                .events
                .iter()
                .find(|entry| entry.event.event_id == response_id)
            {
                if entry.event.kind != "swarmie.agent.model-confirmed"
                    || entry.event.correlation.turn_id.as_deref() != Some(&request.turn_id)
                    || entry.event.correlation.task_id.as_deref() != Some(&task.id)
                {
                    return Err(RuntimeError::invalid(
                        "fixture_replacement_receipt",
                        "confirmed receipt correlation differs",
                    ));
                }
                let result: GenerateResult = serde_json::from_value(
                    entry.event.payload["result"].clone(),
                )
                .map_err(|error| {
                    RuntimeError::invalid("fixture_replacement_receipt", error.to_string())
                })?;
                let GenerateResult::Response { response } = result else {
                    return Err(RuntimeError::invalid(
                        "fixture_replacement_scope",
                        "replacement requires a confirmed successful answer",
                    ));
                };
                if response
                    .message
                    .content
                    .iter()
                    .any(|part| matches!(part, ContentPart::ToolCall { .. }))
                {
                    return Err(RuntimeError::invalid(
                        "fixture_replacement_scope",
                        "replacement does not execute tool requests",
                    ));
                }
                return Ok(TaskTransition::Complete {
                    value: serde_json::json!({"session_id":request.session_id,"response_event_id":response_id,"implementation":"fixture.replacement-agent-loop"}),
                });
            }
            let Some(cursor) = page.next else {
                break;
            };
            query.cursor = Some(cursor);
        }
        Err(RuntimeError::invalid(
            "fixture_replacement_receipt",
            "confirmed response is absent",
        ))
    }
}
