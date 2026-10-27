use crate::hooks::{
    Cause, ExecutionOutcome, ExecutionSource, ExecutionStatus, Lifecycle, ObservationFailure,
};
use crate::{ExecutionFailure, ExecutionResult, failure::DriveError};
use crate::{
    FinishReason, Message, Model, OperationId, Output, Phase, Request, State, ToolCall,
    ToolDecision, ToolExecution, ToolResult, Tools, model::invalid,
};
use futures::{FutureExt, future::LocalBoxFuture};
use std::{collections::BTreeSet, rc::Rc};
use tembra::Context;

mod operations;

/// Reusable configuration; each invocation owns its own progress.
#[derive(Clone)]
pub struct Agent {
    model: Rc<dyn Model>,
    tools: Rc<dyn Tools>,
    hooks: Option<Rc<Lifecycle>>,
    progress: Option<swarmie_providers::Progress>,
}

/// Internal awaited confirmation seam, not a plugin hook or an external effect.
pub(crate) trait Confirmation {
    fn confirm<'a>(
        &'a self,
        context: &'a Context,
        next: &'a State,
    ) -> LocalBoxFuture<'a, tembra::Result<()>>;
}

struct Volatile;

impl Confirmation for Volatile {
    fn confirm<'a>(
        &'a self,
        context: &'a Context,
        _next: &'a State,
    ) -> LocalBoxFuture<'a, tembra::Result<()>> {
        async { context.check() }.boxed_local()
    }
}

impl Agent {
    /// Configure model and tool providers. Each execution owns its progress.
    pub fn new(model: Rc<dyn Model>, tools: Rc<dyn Tools>) -> Self {
        Self {
            model,
            tools,
            hooks: None,
            progress: None,
        }
    }

    /// Opt into provider streaming. Previews are transient and never pass through
    /// confirmation, history or tool dispatch. The settled response uses the same
    /// confirmation path as ordinary completion.
    ///
    /// ```
    /// use std::rc::Rc;
    /// use swarmie_agent_loop::{Agent, Model, Tools};
    /// use swarmie_providers::Progress;
    ///
    /// fn streamed_agent(
    ///     declaration: &tembra::Declaration,
    ///     model: Rc<dyn Model>,
    ///     tools: Rc<dyn Tools>,
    /// ) -> tembra::Result<Agent> {
    ///     Ok(Agent::new(model, tools).with_progress(Progress::bind(declaration)?))
    /// }
    /// ```
    pub fn with_progress(mut self, progress: swarmie_providers::Progress) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Bind the declared hooks to this object's lifecycle.
    /// Use one entered Tembra invocation per drive. A root-only Context is invalid.
    /// Unbound Agents do not dispatch contributions.
    pub fn bind_hooks(mut self, declaration: &tembra::Declaration) -> tembra::Result<Self> {
        self.hooks = Some(Lifecycle::bind(declaration)?);
        Ok(self)
    }

    /// Return the last observation error, shared by clones of this bound Agent.
    /// Success does not clear it. It does not identify an invocation or replace its result.
    /// Supplied Orb methods do not expose this diagnostic slot.
    pub fn observation_failure(&self) -> Option<ObservationFailure> {
        self.hooks.as_ref().and_then(|hooks| hooks.last_failure())
    }

    /// Runs without task storage. The caller owns the invocation's physical future.
    /// Use the orb's managed method when dropping a waiter must not abandon work.
    /// Preserve the inner result when embedding execution in a Tembra method:
    ///
    /// ```
    /// use swarmie_agent_loop::{Agent, ExecutionResult, Request};
    /// use tembra::Context;
    ///
    /// async fn execute_agent(
    ///     agent: &Agent,
    ///     context: &Context,
    ///     request: Request,
    /// ) -> tembra::Result<ExecutionResult> {
    ///     Ok(agent.execute(context, request).await)
    /// }
    /// ```
    pub async fn execute(&self, context: &Context, request: Request) -> ExecutionResult {
        let state =
            State::start(request).map_err(|error| ExecutionFailure { error, state: None })?;
        self.resume(context, state).await
    }

    /// Continues caller-owned progress. Pending effects always go through provider recovery.
    /// Rejected initial state returns no progress and never reaches providers.
    pub async fn resume(&self, context: &Context, mut state: State) -> ExecutionResult {
        state
            .validate()
            .map_err(|error| ExecutionFailure { error, state: None })?;
        self.drive(context, &mut state, &Volatile)
            .await
            .map_err(|error| ExecutionFailure {
                error: error.original(),
                state: Some(Box::new(state)),
            })
    }

    /// Callers validate initial progress before entering the shared engine.
    pub(crate) async fn drive(
        &self,
        context: &Context,
        state: &mut State,
        confirmation: &dyn Confirmation,
    ) -> Result<Output, DriveError> {
        let source = self
            .hooks
            .as_ref()
            .map(|_| context.invocation())
            .transpose()?;
        let result = self.drive_inner(context, state, confirmation).await;
        if let (Some(hooks), Some(source)) = (&self.hooks, source) {
            let status = match &result {
                Ok(output) => ExecutionStatus::Completed(output.reason),
                Err(error) => ExecutionStatus::Interrupted(Cause::from(error.cause())),
            };
            hooks
                .finished(
                    context,
                    ExecutionOutcome {
                        source: ExecutionSource {
                            invocation: source.id,
                            root: source.root,
                        },
                        turn: state.turn,
                        usage: state.usage,
                        status,
                    },
                )
                .await;
        }
        result
    }

    async fn drive_inner(
        &self,
        context: &Context,
        state: &mut State,
        confirmation: &dyn Confirmation,
    ) -> Result<Output, DriveError> {
        let mut recover = true;
        loop {
            if !matches!(state.phase, Phase::Finished { .. }) {
                context.check()?;
            }
            match state.phase.clone() {
                Phase::PrepareTurn => {
                    let mut next = state.clone();
                    next.turn += 1;
                    let request = self.prepare(context, state, next.turn).await?;
                    next.available_tools =
                        request.tools.iter().map(|tool| tool.name.clone()).collect();
                    next.phase = Phase::ModelPending {
                        operation: OperationId::new(),
                        request,
                    };
                    adopt(context, state, next, confirmation).await?;
                    recover = false;
                }
                Phase::ModelPending { operation, request } => {
                    let response = self
                        .run_model(context, operation, &request, recover)
                        .await?;
                    context.check()?;
                    let mut ids = BTreeSet::new();
                    for message in &state.request.context.messages {
                        if let Message::Assistant(message) = message {
                            ids.extend(message.tool_calls.iter().map(|call| call.id.clone()));
                        }
                    }
                    for call in &response.message.tool_calls {
                        if !ids.insert(call.id.clone()) {
                            return Err(invalid("duplicate tool call identity").into());
                        }
                    }
                    let mut next = state.clone();
                    next.usage = next
                        .usage
                        .checked_add(response.usage)
                        .ok_or_else(|| invalid("token count overflow"))?;
                    next.phase = if response.message.tool_calls.is_empty() {
                        Phase::Finished {
                            reason: FinishReason::Complete,
                        }
                    } else {
                        Phase::PrepareTools {
                            calls: response.message.tool_calls.clone(),
                            next: 0,
                        }
                    };
                    next.request
                        .context
                        .messages
                        .push(Message::Assistant(response.message));
                    adopt(context, state, next, confirmation).await?;
                    self.after_turn(context, state).await;
                    recover = false;
                }
                Phase::PrepareTools { calls, next: index } => {
                    let (call, prepared) = self.prepare_call(context, state, &calls[index]).await?;
                    let mut next = state.clone();
                    match prepared.decision {
                        ToolDecision::Allow => {
                            next.phase = Phase::ToolPending {
                                operation: OperationId::new(),
                                call,
                                provider: prepared.provider,
                                calls,
                                next: index,
                            };
                        }
                        ToolDecision::Block(message) => {
                            let result = self
                                .transform_result(
                                    context,
                                    state.turn,
                                    &call,
                                    ToolResult::Blocked(message),
                                )
                                .await?;
                            finish_tool(&mut next, calls, index, result)
                        }
                    }
                    adopt(context, state, next, confirmation).await?;
                    self.after_turn(context, state).await;
                    recover = false;
                }
                Phase::ToolPending {
                    operation,
                    call,
                    provider,
                    calls,
                    next: index,
                } => {
                    context.check()?;
                    let result = self
                        .run_tool(
                            context,
                            state,
                            ToolExecution {
                                operation,
                                call: &call,
                                provider: provider.as_ref(),
                            },
                            recover,
                        )
                        .await?;
                    let result = self
                        .transform_result(context, state.turn, &call, result)
                        .await
                        .map_err(DriveError::Provider)?;
                    let mut next = state.clone();
                    finish_tool(&mut next, calls, index, result);
                    adopt(context, state, next, confirmation).await?;
                    self.after_turn(context, state).await;
                    recover = false;
                }
                Phase::Finished { reason } => return Ok(state.output(reason)),
            }
        }
    }
}

fn finish_tool(state: &mut State, calls: Vec<ToolCall>, index: usize, result: ToolResult) {
    state.request.context.messages.push(Message::Tool {
        call: calls[index].id.clone(),
        result,
    });
    state.phase = if index + 1 < calls.len() {
        Phase::PrepareTools {
            calls,
            next: index + 1,
        }
    } else if state.turn >= state.request.max_turns.get() {
        Phase::Finished {
            reason: FinishReason::TurnLimit,
        }
    } else {
        Phase::PrepareTurn
    };
}

async fn adopt(
    context: &Context,
    state: &mut State,
    next: State,
    confirmation: &dyn Confirmation,
) -> tembra::Result<()> {
    context.check()?;
    confirmation.confirm(context, &next).await?;
    *state = next;
    Ok(())
}
