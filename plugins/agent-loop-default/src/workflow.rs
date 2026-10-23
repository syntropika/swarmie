use std::{cell::RefCell, collections::BTreeSet, rc::Rc, time::Duration};

use swarmie_agent_contracts::*;
use swarmie_model_contracts::{FinishReason, GenerateResult};
use swarmie_session_contracts::{SessionStore, SessionStoreInput, SessionStoreRef};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, DurableTasksRef, TaskDefinition, TaskInvocation, TaskLookup,
    TaskOutcome, TaskRecord, TaskStatus, TaskStorage, TaskStorageInput, TaskTransition,
    task_definitions,
};
use swarmie_task_session_contracts::{TaskSessionBindings, TaskSessionBindingsInput};
use tembra::{core, prelude::*};
use tembra_hooks::{
    HookManager, HookManagerOptions, PreparedBail, PreparedMiddleware, PreparedWaterfall,
};

use crate::{
    TURN_KIND,
    admission::Admission,
    journal::Journal,
    observers::{Observation, ObserverDispatcher},
    state::{Checkpoint, decode, encode, invalid, now_ms},
};

#[tembra::contract(local, name = "swarmie.agent-loop-default.turn-handler", version = 1)]
trait TurnHandler {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

pub(crate) fn declare(
    scope: &core::FacetScope,
    setup: &mut tembra::FacetDeclaration<'_, '_>,
    driver: Rc<dyn core::Driver>,
    observers: Rc<ObserverDispatcher>,
) -> Result<()> {
    let tools = scope.registry(&tool_definitions()?)?;
    let terminals = scope.registry(&crate::model::terminals()?)?;
    let hooks = HookManager::new(scope.clone(), driver.clone(), HookManagerOptions::default())?;
    let terminal_driver = driver.clone();
    let terminal_observers = observers.clone();
    setup.serve_with_options(
        TurnHandler,
        move |_, _, declaration| {
            declaration.core_mut().register_method(
                &task_definitions()?,
                TURN_KIND,
                encode(&TaskDefinition {
                    kind: TURN_KIND.into(),
                    version: 3,
                })?,
                "step",
            )?;
            let sessions = declaration.get(SessionStore, SessionStoreInput {})?;
            let admission = Admission::new(
                declaration.get(TaskSessionBindings, TaskSessionBindingsInput {})?,
                declaration.get(TaskStorage, TaskStorageInput {})?,
                sessions.clone(),
            );
            Ok(Engine {
                sessions,
                admission,
                tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                select_context: hooks.declare_waterfall(
                    declaration.core_mut(),
                    "select-context",
                    select_context,
                )?,
                prepare_context: hooks.declare_waterfall(
                    declaration.core_mut(),
                    "prepare-context",
                    prepare_context,
                )?,
                before_model: hooks.declare_waterfall(
                    declaration.core_mut(),
                    "before-model",
                    before_model_call,
                )?,
                wrap_model: hooks.declare_middleware(
                    declaration.core_mut(),
                    "wrap-model",
                    wrap_model_call,
                )?,
                before_tool: hooks.declare_waterfall(
                    declaration.core_mut(),
                    "before-tool",
                    before_tool_call,
                )?,
                authorize_tool: hooks.declare_bail(
                    declaration.core_mut(),
                    "authorize-tool",
                    authorize_tool_call,
                )?,
                observers: observers.clone(),
                _monitor: declaration.get(AgentObservations, AgentObservationsInput {})?,
                tools: tools.clone(),
                terminals: terminals.clone(),
                driver: driver.clone(),
                active: RefCell::new(BTreeSet::new()),
            })
        },
        core::ServeOptions {
            lifetime: core::Lifetime::Host,
            ..Default::default()
        },
    )?;
    crate::model::declare(scope, setup, terminal_driver, terminal_observers)
}

pub(crate) struct Engine {
    pub(crate) sessions: SessionStoreRef,
    pub(crate) admission: Admission,
    pub(crate) tasks: DurableTasksRef,
    pub(crate) select_context: PreparedWaterfall<ContextSelection>,
    pub(crate) prepare_context: PreparedWaterfall<PreparedContext>,
    pub(crate) before_model: PreparedWaterfall<ModelCall>,
    pub(crate) wrap_model: PreparedMiddleware<ModelCall, GenerateResult>,
    pub(crate) before_tool: PreparedWaterfall<ToolCall>,
    pub(crate) authorize_tool: PreparedBail<ToolCall, ToolDenial>,
    pub(crate) observers: Rc<ObserverDispatcher>,
    _monitor: AgentObservationsRef,
    pub(crate) tools: core::RegistryView,
    pub(crate) terminals: core::RegistryView,
    pub(crate) driver: Rc<dyn core::Driver>,
    active: RefCell<BTreeSet<String>>,
}

/// Latest fully recorded internal substep; speculative hook work never advances this view.
struct Bookkeeping {
    checkpoint: Checkpoint,
    phase: String,
}

struct PhaseGuard<'a> {
    active: &'a RefCell<BTreeSet<String>>,
    id: String,
}

impl Drop for PhaseGuard<'_> {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.id);
    }
}

impl TurnHandlerProvider for Engine {
    async fn step(&self, invocation: TaskInvocation, context: Context) -> Result<TaskTransition> {
        let task = match invocation {
            TaskInvocation::Migrate {
                task,
                target_version,
            } => {
                return if target_version == 3 && task.definition_version == 3 {
                    let request: StartTurn = decode(task.input.clone())?;
                    let checkpoint: Checkpoint = decode(task.checkpoint.clone())?;
                    checkpoint.validate_identity(&request)?;
                    self.admission
                        .confirm(&checkpoint.turn(&request), &context)
                        .await?;
                    Ok(TaskTransition::Migrate {
                        checkpoint: task.checkpoint,
                        phase: task.phase,
                    })
                } else {
                    Ok(TaskTransition::RejectMigration {
                        reason: "unsupported agent checkpoint format; definition and checkpoint version 3 are required".into(),
                    })
                };
            }
            TaskInvocation::Run { task, .. } => task,
        };
        if !self.active.borrow_mut().insert(task.id.clone()) {
            return Err(RuntimeError::unavailable(
                "agent_turn_busy",
                "this turn phase is already physically active",
            ));
        }
        let _active = PhaseGuard {
            active: &self.active,
            id: task.id.clone(),
        };
        if task.kind != TURN_KIND
            || task.definition_version != 3
            || task.status != TaskStatus::Running
            || task.cancellation_requested
            || !matches!(task.phase.as_str(), "start" | "model" | "tool" | "finish")
        {
            return Err(invalid(
                "turn invocation differs from the admitted durable task",
            ));
        }
        let request: StartTurn = decode(task.input.clone())?;
        request.limits.validate()?;
        if task.id != crate::state::task_id(&request) {
            return Err(invalid("task identity differs from the turn"));
        }
        let checkpoint: Checkpoint = decode(task.checkpoint.clone())?;
        checkpoint.validate_identity(&request)?;
        // This phase root confirms its routing once; its Session reads and appends use the proof.
        // Confirmation, which can activate matching Pending routing, precedes the comparison
        // with the durable task below, so its failures are reported before that mismatch.
        let admitted = self
            .admission
            .confirm(&checkpoint.turn(&request), &context)
            .await?;
        // One durable read after the confirmation's awaits binds the invocation to the current
        // record, so a stale or foreign invocation cannot write Session receipts.
        let current = self
            .tasks
            .with_context(&context)
            .get(TaskLookup {
                id: task.id.clone(),
            })
            .await?
            .ok_or_else(|| invalid("turn task is absent"))?;
        if current != task {
            return Err(invalid(
                "turn invocation differs from the admitted durable task",
            ));
        }
        context.check(context.host_id())?;
        let remaining = checkpoint
            .started_at_ms
            .saturating_add(request.limits.max_duration_ms)
            .saturating_sub(now_ms()?);
        if remaining == 0 && task.phase != "finish" {
            if let Some(decision) = self
                .recover_terminal_decision(&task, &request, &checkpoint, &context)
                .await?
            {
                return Ok(decision);
            }
            return self.end_checkpoint(checkpoint, "duration limit reached");
        }
        let (child, guard) = context.child_call(context.principal().clone())?;
        let child = Context::from_core(child);
        let mut journal = Journal::new(admitted);
        let progress = RefCell::new(Bookkeeping {
            checkpoint: checkpoint.clone(),
            phase: task.phase.clone(),
        });
        let result = {
            let work = self.phase(
                &task,
                &request,
                checkpoint.clone(),
                &progress,
                &mut journal,
                &child,
            );
            let timer = self
                .driver
                .sleep(Duration::from_millis(if task.phase == "finish" {
                    1_000
                } else {
                    remaining.max(1)
                }));
            futures::pin_mut!(work, timer);
            match futures::future::select(work, timer).await {
                futures::future::Either::Left((result, _)) => result,
                futures::future::Either::Right(((), work)) => {
                    child.cancellation().cancel(RuntimeError::aborted(
                        "agent_deadline",
                        "turn duration limit reached",
                    ));
                    // Cooperative cancellation still awaits physical invocation ownership.
                    let _ = work.await;
                    if let Some(decision) = self
                        .recover_terminal_decision(&task, &request, &checkpoint, &context)
                        .await?
                    {
                        Ok(decision)
                    } else if task.phase == "finish" {
                        Ok(TaskTransition::Fail {
                            error: checkpoint
                                .ending
                                .clone()
                                .unwrap_or_else(|| "turn finalization deadline reached".into()),
                        })
                    } else {
                        self.end_checkpoint(
                            progress.borrow().checkpoint.clone(),
                            "duration limit reached",
                        )
                    }
                }
            }
        };
        drop(guard);
        if let Err(error) = &result
            && error.reason != crate::terminal_recovery::UNRESOLVED
            && context.check(context.host_id()).is_ok()
        {
            let failed_phase = progress.borrow().phase.clone();
            journal
                .append(
                    &self.sessions,
                    &checkpoint.turn(&request),
                    &format!("error:{}:{}", failed_phase, task.attempts),
                    "swarmie.agent.phase-error",
                    serde_json::json!({"phase":failed_phase,"reason":error.reason}),
                    &context,
                )
                .await?;
        }
        let duration_exhausted =
            now_ms()?.saturating_sub(checkpoint.started_at_ms) >= request.limits.max_duration_ms;
        match result {
            Err(error)
                if error.reason != crate::terminal_recovery::UNRESOLVED
                    && duration_exhausted
                    && context.check(context.host_id()).is_ok() =>
            {
                // A phase's own deadline can settle just before the enclosing timer fires.
                // Its physical work is already joined; do not park an exhausted turn.
                if let Some(decision) = self
                    .recover_terminal_decision(&task, &request, &checkpoint, &context)
                    .await?
                {
                    Ok(decision)
                } else {
                    self.end_checkpoint(
                        progress.borrow().checkpoint.clone(),
                        "duration limit reached",
                    )
                }
            }
            other => other,
        }
    }
}

impl Engine {
    /// Runs bounded deterministic bookkeeping inside one scheduler-owned invocation. Only
    /// the next effect's committed intent, a forced ending, or a terminal outcome escapes.
    async fn phase(
        &self,
        task: &TaskRecord,
        request: &StartTurn,
        mut cp: Checkpoint,
        progress: &RefCell<Bookkeeping>,
        journal: &mut Journal,
        context: &Context,
    ) -> Result<TaskTransition> {
        let mut phase = task.phase.clone();
        for _ in 0..8 {
            context.check(context.host_id())?;
            progress.borrow_mut().phase = phase.clone();
            let result = self
                .subphase(&phase, request, cp.clone(), journal, context)
                .await;
            let transition = match result {
                Err(error)
                    if matches!(phase.as_str(), "start" | "prepare-model" | "prepare-tool")
                        && matches!(
                            error.code,
                            core::ErrorCode::Invalid
                                | core::ErrorCode::Forbidden
                                | core::ErrorCode::ContractMismatch
                        )
                        && context.check(context.host_id()).is_ok() =>
                {
                    self.end_checkpoint(cp.clone(), &format!("phase refused: {}", error.reason))?
                }
                other => other?,
            };
            match transition {
                TaskTransition::Continue {
                    checkpoint,
                    phase: next_phase,
                } => {
                    let next_cp: Checkpoint = decode(checkpoint.clone())?;
                    // A substep returns only after its Session evidence commits. Timeout
                    // finalization may retain these counters and references, never speculative work.
                    progress.borrow_mut().checkpoint = next_cp.clone();
                    if matches!(next_phase.as_str(), "model" | "tool")
                        || (next_phase == "finish" && next_cp.ending.is_some())
                    {
                        return Ok(TaskTransition::Continue {
                            checkpoint,
                            phase: next_phase,
                        });
                    }
                    cp = next_cp;
                    phase = next_phase;
                }
                terminal => {
                    return Ok(terminal);
                }
            }
        }
        Err(invalid("agent bookkeeping exceeded its bounded phase path"))
    }

    async fn subphase(
        &self,
        phase: &str,
        request: &StartTurn,
        mut cp: Checkpoint,
        journal: &mut Journal,
        context: &Context,
    ) -> Result<TaskTransition> {
        context.check(context.host_id())?;
        let turn = cp.turn(request);
        match phase {
            "start" => {
                journal
                    .append(
                        &self.sessions,
                        &turn,
                        "turn-started",
                        "swarmie.agent.turn-started",
                        encode(request)?,
                        context,
                    )
                    .await?;
                self.observers.enqueue(Observation::TurnStart(turn));
                next(cp, "prepare-model")
            }
            "prepare-model" => self.prepare_model(request, cp, journal, context).await,
            "model" => self.model(request, cp, journal, context).await,
            "model-result" => self.model_result(request, cp, journal, context).await,
            "prepare-tool" => self.prepare_tool(request, cp, journal, context).await,
            "tool" => self.tool(request, cp, journal, context).await,
            "tool-result" => self.tool_result(request, cp, journal, context).await,
            "step-end" => {
                let (_, result) = self
                    .model_observation(request, &cp, journal, context)
                    .await?;
                let GenerateResult::Response { response } = result else {
                    return Err(invalid("missing model response"));
                };
                journal.append(&self.sessions,&turn,&format!("step:{}:end",cp.step),"swarmie.agent.step-ended",serde_json::json!({"finish_reason":response.finish_reason,"tool_calls":cp.tool_index,"output_tokens":cp.output_tokens}),context).await?;
                self.observers.enqueue(Observation::Step(StepEnd {
                    turn: turn.clone(),
                    finish_reason: response.finish_reason,
                    tool_calls: cp.tool_index,
                }));
                if cp.tool_index == 0 {
                    if !matches!(
                        response.finish_reason,
                        FinishReason::Stop | FinishReason::StopSequence
                    ) {
                        return self
                            .end_checkpoint(cp, "model did not finish with a complete answer");
                    }
                    next(cp, "finish")
                } else {
                    cp.step += 1;
                    cp.tool_index = 0;
                    cp.tool_event = None;
                    next(cp, "prepare-model")
                }
            }
            "finish" => {
                let outcome = if let Some(error) = &cp.ending {
                    TaskOutcome::Failed {
                        error: error.clone(),
                    }
                } else {
                    TaskOutcome::Completed {
                        value: completion_value(request, &cp),
                    }
                };
                journal
                    .append(
                        &self.sessions,
                        &turn,
                        "turn-decision",
                        "swarmie.agent.turn-ended",
                        encode(&outcome)?,
                        context,
                    )
                    .await?;
                self.observers.enqueue(Observation::Turn(TurnEnd {
                    turn,
                    outcome: outcome.clone(),
                }));
                match outcome {
                    TaskOutcome::Completed { value } => Ok(TaskTransition::Complete { value }),
                    TaskOutcome::Failed { error } => Ok(TaskTransition::Fail { error }),
                    _ => unreachable!(),
                }
            }
            _ => Err(invalid("unsupported agent phase")),
        }
    }

    pub(crate) fn end_checkpoint(&self, mut cp: Checkpoint, error: &str) -> Result<TaskTransition> {
        cp.ending = Some(error.into());
        next(cp, "finish")
    }
}

pub(crate) fn next(cp: Checkpoint, phase: &str) -> Result<TaskTransition> {
    Ok(TaskTransition::Continue {
        checkpoint: encode(&cp)?,
        phase: phase.into(),
    })
}

/// Exact successful turn payload shared by normal completion and evidence recovery.
pub(crate) fn completion_value(request: &StartTurn, cp: &Checkpoint) -> serde_json::Value {
    serde_json::json!({"session_id":request.session_id,"conversation_id":request.conversation_id,"turn_id":request.turn_id,"response_event_id":cp.response_event,"steps":cp.step+1,"tool_calls":cp.tool_calls,"output_tokens":cp.output_tokens})
}
