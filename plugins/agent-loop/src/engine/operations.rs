use super::Agent;
use crate::{
    FailureReason, ModelRequest, ModelResponse, OperationId, Phase, Recovery, Request, State,
    ToolCall, ToolDecision, ToolExecution, ToolPreparation, ToolResult,
    failure::DriveError,
    hooks::{ModelInput, ToolInput, ToolOutput, TurnOutcome},
    model::invalid,
};
use tembra::{Code, Context, Error};

fn recovery_required(message: impl ToString) -> Error {
    Error::new(
        Code::Unavailable,
        FailureReason::RecoveryRequired.as_str(),
        message.to_string(),
    )
}

impl Agent {
    /// Reconcile a pending request before a provider can receive it again.
    pub(super) async fn run_model(
        &self,
        context: &Context,
        operation: OperationId,
        request: &ModelRequest,
        recover: bool,
    ) -> Result<ModelResponse, DriveError> {
        if recover {
            match self
                .model
                .recover(context, operation, request)
                .await
                .map_err(DriveError::Provider)?
            {
                Recovery::Completed(response) => return Ok(response),
                Recovery::SafeToRetry => context.check()?,
                Recovery::Unknown => {
                    return Err(recovery_required("model outcome is unknown").into());
                }
            }
        }
        self.complete_model(context, operation, request)
            .await
            .map_err(DriveError::Provider)
    }

    /// Recovery retains provider identity and checks permission before a retry.
    pub(super) async fn run_tool(
        &self,
        context: &Context,
        state: &State,
        execution: ToolExecution<'_>,
        recover: bool,
    ) -> Result<ToolResult, DriveError> {
        if recover {
            match self
                .tools
                .recover(context, execution)
                .await
                .map_err(DriveError::Provider)?
            {
                Recovery::Completed(result) => return Ok(result),
                Recovery::SafeToRetry => {
                    let prepared = self
                        .prepare_tool(context, state, execution.call, Some(execution))
                        .await
                        .map_err(DriveError::Provider)?;
                    if prepared.decision != ToolDecision::Allow
                        || prepared.provider.as_ref() != execution.provider
                    {
                        return Err(
                            recovery_required("pending tool is no longer authorized").into()
                        );
                    }
                    context.check()?;
                }
                Recovery::Unknown => {
                    return Err(recovery_required("tool outcome is unknown").into());
                }
            }
        }
        self.tools
            .execute(context, execution)
            .await
            .map_err(DriveError::Provider)
    }

    /// Apply initial call policy before provider selection and intent confirmation.
    pub(super) async fn prepare_call(
        &self,
        context: &Context,
        state: &State,
        call: &ToolCall,
    ) -> tembra::Result<(ToolCall, ToolPreparation)> {
        let mut input = ToolInput {
            turn: state.turn,
            call: call.clone(),
            decision: ToolDecision::Allow,
        };
        if let Some(hooks) = &self.hooks {
            input = hooks.before_tool_call.run(context, input).await?;
            if input.turn != state.turn || input.call.id != call.id || input.call.name != call.name
            {
                return Err(invalid("tool hook changed call identity or turn"));
            }
        }
        let prepared = match input.decision {
            ToolDecision::Allow => self.prepare_tool(context, state, &input.call, None).await?,
            blocked => ToolPreparation {
                decision: blocked,
                provider: None,
            },
        };
        Ok((input.call, prepared))
    }

    async fn complete_model(
        &self,
        context: &Context,
        operation: OperationId,
        request: &ModelRequest,
    ) -> tembra::Result<crate::ModelResponse> {
        match &self.progress {
            Some(progress) => {
                self.model
                    .stream(context, operation, request, progress)
                    .await
            }
            None => self.model.complete(context, operation, request).await,
        }
    }

    pub(super) async fn prepare(
        &self,
        context: &Context,
        state: &State,
        turn: u32,
    ) -> tembra::Result<ModelRequest> {
        let mut input = ModelInput {
            turn,
            request: ModelRequest {
                context: state.request.context.clone(),
                tools: state.request.tools.clone(),
            },
        };
        if let Some(hooks) = &self.hooks {
            if turn == 1 {
                input = hooks.before_turn_agent.run(context, input).await?;
            }
            input = hooks.before_turn.run(context, input).await?;
            input.request.context = hooks
                .prepare_context
                .run(context, input.request.context)
                .await?;
            if input.turn != turn
                || input
                    .request
                    .tools
                    .iter()
                    .any(|t| !state.request.tools.contains(t))
            {
                return Err(invalid("model hook widened tools or changed the turn"));
            }
            State::start(Request {
                context: input.request.context.clone(),
                tools: input.request.tools.clone(),
                max_turns: state.request.max_turns,
            })?;
        }
        Ok(input.request)
    }

    pub(super) async fn transform_result(
        &self,
        context: &Context,
        turn: u32,
        call: &ToolCall,
        result: ToolResult,
    ) -> tembra::Result<ToolResult> {
        let Some(hooks) = &self.hooks else {
            return Ok(result);
        };
        let output = hooks
            .after_tool_call
            .run(
                context,
                ToolOutput {
                    turn,
                    call: call.clone(),
                    result,
                },
            )
            .await?;
        if output.turn != turn || output.call != *call {
            return Err(invalid("result hook changed the confirmed call or turn"));
        }
        Ok(output.result)
    }

    pub(super) async fn after_turn(&self, context: &Context, state: &State) {
        if let Some(hooks) = &self.hooks {
            let reason = match state.phase {
                Phase::Finished { reason } => Some(reason),
                Phase::PrepareTurn => None,
                _ => return,
            };
            hooks
                .turn(
                    context,
                    TurnOutcome {
                        turn: state.turn,
                        usage: state.usage,
                        reason,
                    },
                )
                .await;
        }
    }

    async fn prepare_tool(
        &self,
        context: &Context,
        state: &State,
        call: &ToolCall,
        retry: Option<ToolExecution<'_>>,
    ) -> tembra::Result<ToolPreparation> {
        context.check()?;
        if !state.available_tools.contains(&call.name) {
            return Ok(ToolPreparation {
                decision: ToolDecision::Block(
                    "tool is not available in this prepared request".into(),
                ),
                provider: None,
            });
        }
        let Some(spec) = state
            .request
            .tools
            .iter()
            .find(|tool| tool.name == call.name)
        else {
            return Ok(ToolPreparation {
                decision: ToolDecision::Block("tool is not available in this request".into()),
                provider: None,
            });
        };
        let prepared = if let Some(execution) = retry {
            self.tools.prepare_retry(context, spec, execution).await?
        } else {
            self.tools.prepare(context, spec, call).await?
        };
        if let Some(provider) = &prepared.provider {
            provider.validate()?;
        }
        Ok(prepared)
    }
}
