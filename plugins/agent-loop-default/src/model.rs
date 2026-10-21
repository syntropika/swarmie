//! Managed model terminal. Durable receipts identify uncertain external requests; a model
//! request ID is correlation, not a promise of provider-side idempotency. Hidden reasoning
//! is excluded from Session history while opaque provider continuation remains unchanged.
//!
//! Provider confirmation and the effective middleware outcome use distinct immutable receipts.
//! The terminal commits `model-provider-confirmed` before returning through middleware. The
//! workflow separately commits `model-confirmed` after middleware has transformed or replaced
//! that result. Recovery can replay the first receipt through middleware without another
//! provider request, including a crash between these independent Session commits.

use std::{cell::RefCell, collections::BTreeSet, rc::Rc, time::Duration};

use futures::FutureExt;
use swarmie_agent_contracts::{KnownResult, ModelCall, ModelStreamObservation, StartTurn};
use swarmie_model_contracts::{
    ContentPart, GenerateResult, GenerationEndpoint, GenerationInput, GenerationPhase,
    GenerationReference, GenerationState, ModelEndpoint, ModelEvent, ModelProviderSchemaInput,
    ProviderRegistration, ResponseFormat, Role, Support, model_providers,
};
use swarmie_session_contracts::{SessionStore, SessionStoreInput, SessionStoreRef};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, TaskLookup, TaskStatus, TaskStorage, TaskStorageInput,
};
use swarmie_task_session_contracts::{TaskSessionBindings, TaskSessionBindingsInput};
use tembra::{core, prelude::*};

use crate::{
    TURN_KIND,
    admission::Admission,
    journal,
    observers::{Observation, ObserverDispatcher},
    state::{Checkpoint, decode, encode, invalid, now_ms, stable, task_id},
};

#[tembra::contract(local, name = "swarmie.agent-model-terminal", version = 1)]
pub(crate) trait AgentModelTerminal {
    #[portable]
    async fn invoke(&self, call: ModelCall) -> Result<GenerateResult>;
}

pub(crate) fn terminals() -> Result<core::Registry> {
    core::Registry::new("swarmie.agent-model-terminals", core::ConflictPolicy::Error)
}

pub(crate) fn declare(
    scope: &core::FacetScope,
    setup: &mut tembra::FacetDeclaration<'_, '_>,
    driver: Rc<dyn core::Driver>,
    observers: Rc<ObserverDispatcher>,
) -> Result<()> {
    let scope = scope.clone();
    let registry = terminals()?;
    setup.serve_with_options(
        AgentModelTerminal,
        move |_, _, declaration| {
            declaration.core_mut().register_method(
                &registry,
                "default",
                serde_json::json!({"version":1}),
                "invoke",
            )?;
            let sessions = declaration.get(SessionStore, SessionStoreInput {})?;
            let admission = Admission::new(
                declaration.get(TaskSessionBindings, TaskSessionBindingsInput {})?,
                declaration.get(TaskStorage, TaskStorageInput {})?,
                sessions.clone(),
            );
            Ok(ModelExecutor {
                sessions,
                admission,
                providers: scope.registry(&model_providers()?)?,
                observers: observers.clone(),
                driver: driver.clone(),
                scope: scope.clone(),
                active: Rc::new(RefCell::new(BTreeSet::new())),
            })
        },
        core::ServeOptions {
            lifetime: core::Lifetime::Host,
            ..Default::default()
        },
    )
}

pub(crate) struct ModelExecutor {
    sessions: SessionStoreRef,
    admission: Admission,
    providers: core::RegistryView,
    driver: Rc<dyn core::Driver>,
    observers: Rc<ObserverDispatcher>,
    scope: core::FacetScope,
    active: Rc<RefCell<BTreeSet<String>>>,
}

struct ActiveRequest {
    requests: Rc<RefCell<BTreeSet<String>>>,
    id: String,
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.requests.borrow_mut().remove(&self.id);
    }
}

impl AgentModelTerminalProvider for ModelExecutor {
    async fn invoke(&self, call: ModelCall, context: Context) -> Result<GenerateResult> {
        self.execute(call, context).await
    }
}

impl ModelExecutor {
    /// The terminal independently checks identity and limits after middleware transformation.
    /// A confirmed provider response is replayed through middleware; an interrupted
    /// unconfirmed provider request is never resent automatically.
    pub(crate) async fn execute(
        &self,
        incoming: ModelCall,
        context: Context,
    ) -> Result<GenerateResult> {
        if !self
            .active
            .borrow_mut()
            .insert(incoming.request.request_id.clone())
        {
            return Err(RuntimeError::unavailable(
                "agent_model_busy",
                "this model operation is already physically active",
            ));
        }
        let _active = ActiveRequest {
            requests: self.active.clone(),
            id: incoming.request.request_id.clone(),
        };
        let events = journal::history(&self.sessions, &incoming.turn, &context).await?;
        let start: StartTurn = decode(
            journal::own_event(
                &events,
                &stable(&incoming.turn, "turn-started"),
                &incoming.turn,
                "swarmie.agent.turn-started",
            )?
            .payload
            .clone(),
        )?;
        if incoming.turn.turn_id != start.turn_id
            || incoming.turn.session_id != start.session_id
            || incoming.turn.conversation_id != start.conversation_id
            || incoming.turn.task_id != task_id(&start)
            || incoming.turn.step >= start.limits.max_steps
            || incoming.request.request_id
                != stable(&start, &format!("model:{}", incoming.turn.step))
        {
            return Err(invalid(
                "model terminal causal identity differs from the admitted turn",
            ));
        }
        let suffix = format!("model:{}", incoming.turn.step);
        if let Some(event) = journal::find(
            &events,
            &stable(&start, &format!("{suffix}:provider-confirmed")),
        ) {
            journal::own_event(
                &events,
                &event.event_id,
                &incoming.turn,
                "swarmie.agent.model-provider-confirmed",
            )?;
            if event.payload["operation_id"] != incoming.request.request_id {
                return Err(invalid("confirmed model operation identity differs"));
            }
            let result: GenerateResult = decode(event.payload["result"].clone())?;
            validate_result(&incoming, &result)?;
            return Ok(result);
        }
        let call = match journal::find(&events, &stable(&start, &format!("{suffix}:actual"))) {
            Some(event) => {
                journal::own_event(
                    &events,
                    &event.event_id,
                    &incoming.turn,
                    "swarmie.agent.model-actual",
                )?;
                let saved: ModelCall = decode(event.payload.clone())?;
                if saved.turn != incoming.turn
                    || saved.request.request_id != incoming.request.request_id
                {
                    return Err(invalid("persisted model request identity differs"));
                }
                saved
            }
            None => incoming,
        };
        let tasks = context.get(DurableTasks, DurableTasksInput {}).await?;
        let task = tasks
            .with_context(&context)
            .get(TaskLookup {
                id: call.turn.task_id.clone(),
            })
            .await?
            .ok_or_else(|| invalid("model terminal turn task is absent"))?;
        let checkpoint: Checkpoint = decode(task.checkpoint.clone())?;
        checkpoint.validate_identity(&start)?;
        if checkpoint.turn(&start) != call.turn
            || task.input != encode(&start)?
            || task.cancellation_requested
            || task.status != TaskStatus::Running
            || task.kind != TURN_KIND
            || task.phase != "model"
        {
            return Err(invalid(
                "model terminal turn is no longer admitted for this step",
            ));
        }
        self.admission.require(&call.turn, &context).await?;
        let deadline = checkpoint
            .started_at_ms
            .checked_add(start.limits.max_duration_ms)
            .ok_or_else(|| invalid("turn deadline overflow"))?;
        if now_ms()? >= deadline {
            return Err(RuntimeError::aborted(
                "agent_duration_limit",
                "turn duration exhausted",
            ));
        }
        let remaining = start
            .limits
            .max_output_tokens
            .saturating_sub(checkpoint.output_tokens);
        if call.request.model != start.model
            || call
                .request
                .options
                .max_output_tokens
                .is_none_or(|limit| limit == 0 || u64::from(limit) > remaining)
        {
            return Err(invalid(
                "model terminal request exceeds the admitted model or token budget",
            ));
        }
        if call
            .request
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|part| matches!(part, ContentPart::Reasoning { .. }))
        {
            return Err(invalid(
                "hidden reasoning cannot enter persisted model request history",
            ));
        }
        if let Some(resolution) = journal::find(
            &events,
            &stable(&start, &format!("resolution:{}", call.request.request_id)),
        ) {
            journal::own_event(
                &events,
                &resolution.event_id,
                &call.turn,
                "swarmie.agent.operation-resolution",
            )?;
            let resolution: KnownResult = decode(resolution.payload.clone())?;
            if resolution.operation_id != call.request.request_id {
                return Err(invalid("model resolution operation differs"));
            }
            let evidence = journal::own_event(
                &events,
                &resolution.result_event_id,
                &call.turn,
                "swarmie.agent.model-provider-confirmed",
            )?;
            if evidence.payload["operation_id"] != call.request.request_id {
                return Err(invalid("model result evidence operation differs"));
            }
            let mut result: GenerateResult = decode(evidence.payload["result"].clone())?;
            validate_result(&call, &result)?;
            redact_reasoning(&mut result);
            journal::append(
                &self.sessions,
                &call.turn,
                &format!("{suffix}:provider-confirmed"),
                "swarmie.agent.model-provider-confirmed",
                serde_json::json!({"operation_id":call.request.request_id,"result":result}),
                &context,
            )
            .await?;
            return Ok(result);
        }
        let registration = self.resolve(&call, &context)?;
        let provider = context
            .get(
                ModelEndpoint::new(&registration.provider_contract)?,
                ModelProviderSchemaInput {},
            )
            .await?;
        let capabilities = provider
            .with_context(&context)
            .capabilities(call.request.model.clone())
            .await?;
        if capabilities.streaming == Support::Unsupported
            || (!call.request.tools.is_empty() && capabilities.tools == Support::Unsupported)
            || (call
                .request
                .messages
                .iter()
                .any(|m| m.role == Role::Developer)
                && capabilities.developer_role == Support::Unsupported)
            || (call
                .request
                .messages
                .iter()
                .flat_map(|m| &m.content)
                .any(|p| matches!(p, ContentPart::Image { .. }))
                && capabilities.images == Support::Unsupported)
            || (call.request.options.reasoning_effort.is_some()
                && capabilities.reasoning == Support::Unsupported)
            || matches!(
                (
                    &call.request.options.response_format,
                    capabilities.json,
                    capabilities.json_schema
                ),
                (Some(ResponseFormat::Json), Support::Unsupported, _)
                    | (
                        Some(ResponseFormat::JsonSchema { .. }),
                        _,
                        Support::Unsupported
                    )
            )
        {
            return Err(RuntimeError::invalid(
                "agent_model_capability",
                "model endpoint does not support the effective request",
            ));
        }
        journal::append(
            &self.sessions,
            &call.turn,
            &format!("{suffix}:actual"),
            "swarmie.agent.model-actual",
            encode(&call)?,
            &context,
        )
        .await?;
        if journal::find(&events, &stable(&start, &format!("{suffix}:started"))).is_some() {
            return Err(RuntimeError::unavailable(
                "agent_model_uncertain",
                "interrupted model request requires confirmed result reconciliation",
            ));
        }
        journal::append(
            &self.sessions,
            &call.turn,
            &format!("{suffix}:started"),
            "swarmie.agent.model-started",
            serde_json::json!({"operation_id":call.request.request_id}),
            &context,
        )
        .await?;
        let latest = tasks
            .with_context(&context)
            .get(TaskLookup {
                id: call.turn.task_id.clone(),
            })
            .await?
            .ok_or_else(|| invalid("model task disappeared before generation admission"))?;
        if latest.cancellation_requested
            || latest.status != TaskStatus::Running
            || latest.phase != "model"
            || latest.checkpoint != task.checkpoint
        {
            return Err(RuntimeError::aborted(
                "agent_model_admission",
                "turn changed before generation admission",
            ));
        }
        let generation = context
            .get(
                GenerationEndpoint::new(&registration.generation_contract)?,
                GenerationInput {
                    request: call.request.clone(),
                },
            )
            .await?;
        let consumed = self
            .consume(&generation, &call, deadline, &context)
            .boxed_local();
        let timer = self
            .driver
            .sleep(Duration::from_millis(deadline.saturating_sub(now_ms()?)));
        let consumed = match futures::future::select(consumed, timer).await {
            futures::future::Either::Left((result, _)) => result,
            futures::future::Either::Right(((), running)) => {
                let error = RuntimeError::aborted(
                    "agent_duration_limit",
                    "turn duration exhausted during model invocation",
                );
                context.cancel(error.clone());
                // Cancellation signals even while an event hook is awaiting. Joining keeps
                // physical work owned; an uncooperative handler can still delay settlement.
                let _ = running.await;
                Err(error)
            }
        };
        let result = match consumed {
            Ok(result) => result,
            Err(error) => {
                self.cancel_and_settle(generation, call.clone()).await?;
                return Err(error);
            }
        };
        match persist_provider_result(&self.sessions, &call, result, &context).await {
            Ok(result) => Ok(result),
            Err(error) => {
                // The caller may have expired between stream completion and publication.
                // Retain this live completion and retry the same durable receipt through
                // cleanup authority instead of losing it or submitting another request.
                self.cancel_and_settle(generation, call).await?;
                Err(error)
            }
        }
    }

    fn resolve(&self, call: &ModelCall, context: &Context) -> Result<ProviderRegistration> {
        let table = self.providers.table(Some(context.core()))?;
        let mut selected = None;
        for (_, entry) in table.entries() {
            let registration: ProviderRegistration = decode(entry.data().clone())?;
            if registration.version != 1 {
                continue;
            }
            if registration
                .connections
                .contains(&call.request.model.connection)
            {
                if selected.is_some() {
                    return Err(invalid("multiple model providers claim the connection"));
                }
                selected = Some(registration);
            }
        }
        selected.ok_or_else(|| {
            RuntimeError::unavailable(
                "agent_model_missing",
                "no model provider claims the requested connection",
            )
        })
    }

    async fn require_generation(&self, call: &ModelCall, context: &Context) -> Result<()> {
        self.admission.require(&call.turn, context).await?;
        let tasks = context.get(DurableTasks, DurableTasksInput {}).await?;
        let task = tasks
            .with_context(context)
            .get(TaskLookup {
                id: call.turn.task_id.clone(),
            })
            .await?
            .ok_or_else(|| invalid("model turn task is absent at execution admission"))?;
        let request: StartTurn = decode(task.input.clone())?;
        let checkpoint: Checkpoint = decode(task.checkpoint.clone())?;
        checkpoint.validate_identity(&request)?;
        let remaining = request
            .limits
            .max_output_tokens
            .saturating_sub(checkpoint.output_tokens);
        if task.kind != TURN_KIND
            || task.definition_version != 2
            || task.status != TaskStatus::Running
            || task.cancellation_requested
            || task.phase != "model"
            || checkpoint.turn(&request) != call.turn
            || task.id != task_id(&request)
            || call.request.model != request.model
            || call.request.request_id != stable(&request, &format!("model:{}", checkpoint.step))
            || call
                .request
                .options
                .max_output_tokens
                .is_none_or(|cap| cap == 0 || u64::from(cap) > remaining)
            || now_ms()?
                >= checkpoint
                    .started_at_ms
                    .saturating_add(request.limits.max_duration_ms)
        {
            return Err(invalid(
                "model turn changed before effective generation start",
            ));
        }
        context.check(context.host_id())?;
        Ok(())
    }

    async fn consume(
        &self,
        generation: &GenerationReference,
        call: &ModelCall,
        deadline: u64,
        context: &Context,
    ) -> Result<GenerateResult> {
        self.require_generation(call, context).await?;
        // start is attempted exactly once. A failed reactive publication can follow
        // physical completion; recovery reads this same live object and never submits I/O.
        if let Err(error) = generation.with_context(context).start().await {
            let recovered = generation.with_context(context).recover().await?;
            if recovered.physically_settled {
                return terminal_result(call, recovered);
            }
            return Err(error);
        }
        let mut through = 0;
        loop {
            context.check(context.host_id())?;
            if now_ms()? >= deadline {
                return Err(RuntimeError::aborted(
                    "agent_duration_limit",
                    "turn duration exhausted during generation",
                ));
            }
            // A valid Running projection can remain stale forever after a rejected terminal
            // candidate. Consult the retained completion channel even when State itself reads.
            let state = match generation.progress().value() {
                Ok(state) if state.physically_settled => state,
                _ => generation.with_context(context).recover().await?,
            };
            if state.request_id != call.request.request_id
                || state.acknowledged > through
                || state.acknowledged > state.last_sequence
            {
                return Err(invalid(
                    "generation identity or single-consumer cursor differs",
                ));
            }
            for event in state
                .events
                .iter()
                .filter(|event| event.sequence > through)
                .cloned()
                .collect::<Vec<_>>()
            {
                if through.checked_add(1) != Some(event.sequence) {
                    return Err(invalid("generation stream lost an unacknowledged event"));
                }
                if !matches!(&event.event, ModelEvent::ReasoningDelta { .. }) {
                    self.observers
                        .enqueue(Observation::Stream(ModelStreamObservation {
                            turn: call.turn.clone(),
                            request_id: call.request.request_id.clone(),
                            event: event.clone(),
                        }));
                }
                through = event.sequence;
            }
            if through > state.acknowledged
                && let Err(error) = generation.with_context(context).acknowledge(through).await
            {
                let recovered = generation.with_context(context).recover().await?;
                if recovered.physically_settled {
                    return terminal_result(call, recovered);
                }
                return Err(error);
            }
            if state.physically_settled {
                if through != state.last_sequence {
                    return Err(invalid(
                        "settled generation has an incomplete stream window",
                    ));
                }
                return terminal_result(call, state);
            }
            self.driver.sleep(Duration::from_millis(2)).await;
        }
    }

    async fn cancel_and_settle(
        &self,
        generation: GenerationReference,
        call: ModelCall,
    ) -> Result<()> {
        let driver = self.driver.clone();
        let sessions = self.sessions.clone();
        // Cancellation expires the caller's authority. The owning Facet admits a fresh
        // cleanup root, retaining the generation claim through local physical settlement
        // and its durable provider receipt. Failed State publication cannot prevent cleanup.
        self.scope
            .root(move |cleanup| {
                async move {
                    let cleanup = Context::from_core(cleanup);
                    if generation.core().status() == core::RefStatus::Gone {
                        // Host or Facet retirement invalidates callable claims before joining
                        // provider Resources. That owner now closes physical work; a dead live
                        // object is not recoverable evidence and must never fabricate a receipt.
                        return Err(generation.core().error().unwrap_or_else(|| {
                            RuntimeError::unavailable(
                                "agent_generation_gone",
                                "generation lifetime has ended",
                            )
                        }));
                    }
                    // An already completed result wins over cancellation. recover() is also
                    // necessary when cancel's own terminal State mutation is rejected.
                    let initial = generation.with_context(&cleanup).recover().await;
                    let result = match initial {
                        Ok(recovered) if recovered.physically_settled => {
                            terminal_result(&call, recovered)?
                        }
                        _ => {
                            let _ = generation.with_context(&cleanup).cancel().await;
                            loop {
                                if generation.core().status() == core::RefStatus::Gone {
                                    return Err(generation.core().error().unwrap_or_else(|| {
                                        RuntimeError::unavailable(
                                            "agent_generation_gone",
                                            "generation lifetime has ended",
                                        )
                                    }));
                                }
                                // The recovery method is independent of terminal State adoption.
                                // A transient delivery failure is not proof of physical shutdown,
                                // so keep the claim and retry observation rather than inference.
                                let recovered =
                                    match generation.with_context(&cleanup).recover().await {
                                        Ok(state) => Some(state),
                                        Err(_) => generation.progress().value().ok(),
                                    };
                                if let Some(recovered) = recovered
                                    && recovered.physically_settled
                                {
                                    break terminal_result(&call, recovered)?;
                                }
                                driver.sleep(Duration::from_millis(2)).await;
                            }
                        }
                    };
                    persist_provider_result(&sessions, &call, result, &cleanup).await?;
                    // Local settlement is confirmed before releasing this claim. A refused
                    // receipt append returns an error and cannot advance the task checkpoint;
                    // the existing started receipt keeps later retries from resubmitting I/O.
                    // No local result says anything about remote rollback or billing.
                    Ok(())
                }
                .boxed_local()
            })
            .await
    }
}

fn validate_result(call: &ModelCall, result: &GenerateResult) -> Result<()> {
    if let GenerateResult::Response { response } = result
        && (response.request_id != call.request.request_id
            || response.model != call.request.model
            || response.message.role != Role::Assistant)
    {
        return Err(invalid(
            "model response does not belong to the admitted request",
        ));
    }
    Ok(())
}

pub(crate) fn redact_reasoning(result: &mut GenerateResult) {
    if let GenerateResult::Response { response } = result {
        response
            .message
            .content
            .retain(|part| !matches!(part, ContentPart::Reasoning { .. }));
    }
}

fn terminal_result(call: &ModelCall, state: GenerationState) -> Result<GenerateResult> {
    if state.request_id != call.request.request_id
        || !state.physically_settled
        || state.phase != GenerationPhase::Settled
    {
        return Err(invalid(
            "recovered generation identity or physical settlement differs",
        ));
    }
    match (state.response, state.error) {
        (Some(response), None) => Ok(GenerateResult::Response {
            response: Box::new(response),
        }),
        (None, Some(error)) => Ok(GenerateResult::Error { error }),
        _ => Err(invalid("settled generation has no unique terminal result")),
    }
}

async fn persist_provider_result(
    sessions: &SessionStoreRef,
    call: &ModelCall,
    mut result: GenerateResult,
    context: &Context,
) -> Result<GenerateResult> {
    validate_result(call, &result)?;
    redact_reasoning(&mut result);
    journal::append(
        sessions,
        &call.turn,
        &format!("model:{}:provider-confirmed", call.turn.step),
        "swarmie.agent.model-provider-confirmed",
        serde_json::json!({"operation_id":call.request.request_id,"result":result}),
        context,
    )
    .await?;
    Ok(result)
}
