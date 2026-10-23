//! A distinct fixture-only AgentLoop implementation that completes confirmed final answers.
//! It shares only public Contracts and explicitly admits version-three finalization checkpoints
//! or a model-phase checkpoint with a complete correlated provider receipt. It does not invoke
//! models, tools or lifecycle contributions; unsupported or uncertain phases remain refused.

use swarmie_agent_contracts::*;
use swarmie_model_contracts::{ContentPart, FinishReason, GenerateResult, Role};
use swarmie_session_contracts::{
    EventQuery, SessionLookup, SessionStore, SessionStoreInput, SessionStoreRef,
};
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
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
                    route: Route {
                        storage: declaration.get(TaskStorage, TaskStorageInput {})?,
                        bindings: declaration
                            .get(TaskSessionBindings, TaskSessionBindingsInput {})?,
                        sessions: declaration.get(SessionStore, SessionStoreInput {})?,
                    },
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
                    serde_json::json!({"kind":KIND,"version":3}),
                    "step",
                )?;
                Ok(Finalizer {
                    tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                    route: Route {
                        storage: declaration.get(TaskStorage, TaskStorageInput {})?,
                        bindings: declaration
                            .get(TaskSessionBindings, TaskSessionBindingsInput {})?,
                        sessions: declaration.get(SessionStore, SessionStoreInput {})?,
                    },
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

struct Route {
    storage: TaskStorageRef,
    bindings: TaskSessionBindingsRef,
    sessions: SessionStoreRef,
}

impl Route {
    async fn historical(&self, task: &TaskRecord, context: &Context) -> Result<TaskSessionBinding> {
        let key = BindingKey {
            task_store_id: self
                .storage
                .with_context(context)
                .outbox_status()
                .await?
                .store_id,
            task_id: task.id.clone(),
        };
        let Some(BindingDisposition::Routed { binding }) = self
            .bindings
            .with_context(context)
            .lookup(key.clone())
            .await?
        else {
            return Err(RuntimeError::invalid(
                "fixture_replacement_binding",
                "task has no routed association",
            ));
        };
        if binding.key != key
            || binding.task.input != task.input
            || binding.task.kind != task.kind
            || binding.task.definition_version != task.definition_version
            || binding.parent_task_id != task.parent
            || binding.destination.session_id != task.input["session_id"]
            || binding.destination.conversation_id != task.input["conversation_id"]
            || binding.destination.turn_id != task.input["turn_id"]
        {
            return Err(RuntimeError::invalid(
                "fixture_replacement_binding",
                "association differs from saved task",
            ));
        }
        Ok(*binding)
    }

    async fn require(&self, task: &TaskRecord, context: &Context) -> Result<()> {
        let historical = self.historical(task, context).await?;
        let binding = self
            .bindings
            .with_context(context)
            .require_confirmed_binding(historical.key.clone())
            .await?;
        let session = self
            .sessions
            .with_context(context)
            .open(SessionLookup {
                session_id: binding.destination.session_id.clone(),
            })
            .await?
            .ok_or_else(|| {
                RuntimeError::invalid("fixture_replacement_binding", "bound session is absent")
            })?;
        if binding.admission != BindingAdmission::Active
            || binding.destination != historical.destination
            || session.store_id != binding.destination.session_store_id
            || self
                .storage
                .with_context(context)
                .outbox_status()
                .await?
                .store_id
                != binding.key.task_store_id
        {
            return Err(RuntimeError::invalid(
                "fixture_replacement_binding",
                "selected stores differ from association",
            ));
        }
        context.check(context.host_id())?;
        Ok(())
    }
}

struct Control {
    tasks: DurableTasksRef,
    route: Route,
}

impl Control {
    async fn find(&self, lookup: &TurnLookup, context: &Context) -> Result<Option<TurnRecord>> {
        let mut query = TaskQuery::default();
        loop {
            let page = self
                .tasks
                .with_context(context)
                .graph(query.clone())
                .await?;
            for task in page.records {
                if task.kind == KIND
                    && task.input["turn_id"] == lookup.turn_id
                    && task.input["session_id"] == lookup.session_id
                    && task.input["conversation_id"] == lookup.conversation_id
                {
                    let request: StartTurn =
                        serde_json::from_value(task.input.clone()).map_err(|error| {
                            RuntimeError::invalid("fixture_replacement_input", error.to_string())
                        })?;
                    self.route.historical(&task, context).await?;
                    return Ok(Some(TurnRecord {
                        turn_id: lookup.turn_id.clone(),
                        session_id: request.session_id,
                        conversation_id: request.conversation_id,
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
            .find(
                &TurnLookup {
                    session_id: request.session_id.clone(),
                    conversation_id: request.conversation_id.clone(),
                    turn_id: request.turn_id.clone(),
                },
                &context,
            )
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
        self.route.require(&record.task, &context).await?;
        Ok(record)
    }

    async fn inspect(&self, request: TurnLookup, context: Context) -> Result<Option<TurnRecord>> {
        let record = self.find(&request, &context).await?;
        if let Some(record) = &record {
            self.route.require(&record.task, &context).await?;
        }
        Ok(record)
    }

    async fn cancel(&self, request: TurnLookup, context: Context) -> Result<TurnRecord> {
        let record = self
            .find(
                &TurnLookup {
                    session_id: request.session_id.clone(),
                    conversation_id: request.conversation_id.clone(),
                    turn_id: request.turn_id.clone(),
                },
                &context,
            )
            .await?
            .ok_or_else(|| {
                RuntimeError::invalid("fixture_replacement_missing", "turn is absent")
            })?;
        self.tasks
            .with_context(&context)
            .cancel(TaskLookup { id: record.task.id })
            .await?;
        self.find(
            &TurnLookup {
                session_id: request.session_id.clone(),
                conversation_id: request.conversation_id.clone(),
                turn_id: request.turn_id.clone(),
            },
            &context,
        )
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
        let record = self
            .find(
                &TurnLookup {
                    session_id: request.session_id.clone(),
                    conversation_id: request.conversation_id.clone(),
                    turn_id: request.turn_id.clone(),
                },
                &context,
            )
            .await?
            .ok_or_else(|| {
                RuntimeError::invalid("fixture_replacement_missing", "turn is absent")
            })?;
        self.route.require(&record.task, &context).await?;
        self.tasks.with_context(&context).resume().await?;
        self.find(
            &TurnLookup {
                session_id: request.session_id.clone(),
                conversation_id: request.conversation_id.clone(),
                turn_id: request.turn_id.clone(),
            },
            &context,
        )
        .await?
        .ok_or_else(|| RuntimeError::invalid("fixture_replacement_missing", "turn is absent"))
    }
}

struct Finalizer {
    tasks: DurableTasksRef,
    route: Route,
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
                return if target_version == 3 && task.definition_version == 3 {
                    Ok(TaskTransition::Migrate {
                        checkpoint: task.checkpoint,
                        phase: task.phase,
                    })
                } else {
                    Ok(TaskTransition::RejectMigration {
                        reason: "replacement supports only checkpoint version three".into(),
                    })
                };
            }
        };
        let admitted = self
            .tasks
            .with_context(&context)
            .get(TaskLookup {
                id: task.id.clone(),
            })
            .await?
            .ok_or_else(|| RuntimeError::invalid("fixture_replacement_task", "task is absent"))?;
        if admitted != task
            || task.definition_version != 3
            || task.kind != KIND
            || task.status != TaskStatus::Running
            || task.cancellation_requested
        {
            return Err(RuntimeError::invalid(
                "fixture_replacement_task",
                "invocation differs from its admitted task",
            ));
        }
        self.route.require(&task, &context).await?;
        if !matches!(task.phase.as_str(), "model" | "finish") {
            return Err(RuntimeError::invalid(
                "fixture_replacement_scope",
                "checkpoint is outside confirmed final-answer scope",
            ));
        }
        let request: StartTurn = serde_json::from_value(task.input.clone()).map_err(|error| {
            RuntimeError::invalid("fixture_replacement_input", error.to_string())
        })?;
        if task.checkpoint["version"] != 3
            || task.checkpoint["session_id"] != request.session_id
            || task.checkpoint["conversation_id"] != request.conversation_id
            || task.checkpoint["turn_id"] != request.turn_id
        {
            return Err(RuntimeError::invalid(
                "fixture_replacement_checkpoint",
                "checkpoint identity or version differs",
            ));
        }
        let request_id = task.checkpoint["request_event"].as_str().ok_or_else(|| {
            RuntimeError::invalid(
                "fixture_replacement_checkpoint",
                "request reference is absent",
            )
        })?;
        let effective_id = if task.phase == "model" {
            None
        } else {
            Some(task.checkpoint["response_event"].as_str().ok_or_else(|| {
                RuntimeError::invalid(
                    "fixture_replacement_checkpoint",
                    "confirmed response reference is absent",
                )
            })?)
        };
        let kind = if effective_id.is_some() {
            "swarmie.agent.model-confirmed"
        } else {
            "swarmie.agent.model-provider-confirmed"
        };
        let mut query = EventQuery::new(&request.session_id);
        query.conversation_id = Some(request.conversation_id.clone());
        query.turn_id = Some(request.turn_id.clone());
        let mut planned: Option<ModelCall> = None;
        loop {
            let page = self
                .sessions
                .with_context(&context)
                .events(query.clone())
                .await?;
            for entry in &page.events {
                if entry.event.event_id == request_id {
                    if entry.event.version != 2 || entry.event.kind != "swarmie.agent.model-request"
                    {
                        return Err(RuntimeError::invalid(
                            "fixture_replacement_request",
                            "request event shape differs",
                        ));
                    }
                    let call: ModelCall = serde_json::from_value(entry.event.payload.clone())
                        .map_err(|error| {
                            RuntimeError::invalid("fixture_replacement_request", error.to_string())
                        })?;
                    if call.turn.session_id != request.session_id
                        || call.turn.conversation_id != request.conversation_id
                        || call.turn.turn_id != request.turn_id
                        || call.turn.task_id != task.id
                        || Some(u64::from(call.turn.step)) != task.checkpoint["step"].as_u64()
                        || call.request.model != request.model
                    {
                        return Err(RuntimeError::invalid(
                            "fixture_replacement_request",
                            "request causal identity differs",
                        ));
                    }
                    planned = Some(call);
                }
                let Some(call) = &planned else {
                    continue;
                };
                let selected = if let Some(id) = effective_id {
                    entry.event.event_id == id
                } else {
                    entry.event.kind == kind
                        && entry.event.payload["operation_id"] == call.request.request_id
                };
                if !selected {
                    continue;
                }
                if entry.event.version != 2
                    || entry.event.kind != kind
                    || entry.session_id != request.session_id
                    || entry.event.correlation.conversation_id.as_deref()
                        != Some(&request.conversation_id)
                    || entry.event.correlation.turn_id.as_deref() != Some(&request.turn_id)
                    || entry.event.correlation.task_id.as_deref() != Some(&task.id)
                    || entry.event.payload["operation_id"] != call.request.request_id
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
                        "replacement requires a successful final response",
                    ));
                };
                if response.request_id != call.request.request_id
                    || response.model != call.request.model
                    || response.message.role != Role::Assistant
                    || !matches!(
                        response.finish_reason,
                        FinishReason::Stop | FinishReason::StopSequence
                    )
                    || response.message.content.iter().any(|part| {
                        matches!(
                            part,
                            ContentPart::ToolCall { .. } | ContentPart::Reasoning { .. }
                        )
                    })
                {
                    return Err(RuntimeError::invalid(
                        "fixture_replacement_scope",
                        "replacement requires a correlated final answer without tools or private reasoning",
                    ));
                }
                self.route.require(&task, &context).await?;
                return Ok(TaskTransition::Complete {
                    value: serde_json::json!({"session_id":request.session_id,"conversation_id":request.conversation_id,"turn_id":request.turn_id,"response_event_id":entry.event.event_id,"implementation":"fixture.replacement-agent-loop"}),
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
