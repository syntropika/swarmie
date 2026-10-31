use futures::{FutureExt, future::LocalBoxFuture};
use tembra::{Code, Context, Declaration, Orb, Service};

use crate::{
    IntoToolOutput, OperationId, Recovery, Tool, ToolCall, ToolContext, ToolDecision,
    ToolExecution, ToolExecutionError, ToolMethod, ToolResult, ToolSpec, Tools, invalid, tool_spec,
    validate,
};

/// An author instance and its immutable metadata. Live ownership belongs to the
/// Tembra service environment; this adapter creates no registry or task runtime.
pub struct ToolHandler<T> {
    tool: T,
    spec: ToolSpec,
    context: ToolContext,
    policy: Option<crate::ToolPolicy>,
    hooks: Option<crate::hooks::Bound>,
}

impl<T: Tool> ToolHandler<T> {
    pub fn new(tool: T) -> tembra::Result<Self> {
        Ok(Self {
            spec: tool_spec(&tool)?,
            tool,
            context: ToolContext::new(),
            policy: None,
            hooks: None,
        })
    }

    pub fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    pub fn with_policy(mut self, policy: crate::ToolPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Bind the descriptors declared by this provider's Orb. Custom services
    /// must pass their entered invocation context when calling this adapter.
    pub fn bind_hooks(mut self, declaration: &Declaration) -> tembra::Result<Self> {
        self.hooks = Some(crate::hooks::Bound::bind(declaration)?);
        Ok(self)
    }

    pub fn observation_failure(&self) -> Option<crate::hooks::Failure> {
        self.hooks.as_ref().and_then(crate::hooks::Bound::failure)
    }

    /// Supply host-owned inbound data, never model-controlled authorization.
    /// Each invocation receives its own snapshot.
    pub fn with_context(mut self, context: ToolContext) -> Self {
        self.context = context.for_dispatch();
        self
    }

    /// Execute with caller-owned result metadata. A dropped execution future
    /// never publishes partial context mutations. It cannot undo external I/O.
    pub async fn execute_with(
        &self,
        context: &Context,
        execution: ToolExecution<'_>,
        data: &mut ToolContext,
    ) -> tembra::Result<ToolResult> {
        let mut dispatched = data.for_dispatch();
        dispatched.enter(context.clone(), execution.operation, execution.call.clone());
        let outcome = async {
            check_direct(context, execution)?;
            let call = execution.call;
            if let ToolDecision::Block(reason) = self
                .admit(
                    context,
                    &self.spec,
                    call,
                    crate::hooks::Phase::Execution,
                    Some(execution.operation),
                )
                .await?
            {
                return Ok(ToolResult::Blocked(reason));
            }
            let args = serde_json::from_value(call.arguments.clone()).map_err(invalid)?;
            check_active(context)?;
            execution_result(
                self.tool
                    .call(&mut dispatched, args)
                    .await
                    .map_err(|error| self.tool.map_error(error))
                    .and_then(IntoToolOutput::into_tool_output),
            )
        }
        .await;
        let metadata = dispatched.result_context();
        data.accept_dispatch_result(dispatched);
        if let Some(hooks) = &self.hooks {
            let event = match &outcome {
                Ok(result) => crate::hooks::Outcome::Executed(result.clone()),
                Err(error) => crate::hooks::Outcome::Unconfirmed(error.into()),
            };
            hooks
                .finished(
                    context,
                    execution,
                    crate::hooks::Phase::Execution,
                    event,
                    metadata,
                )
                .await;
        }
        outcome
    }

    async fn admit(
        &self,
        context: &Context,
        spec: &ToolSpec,
        call: &ToolCall,
        phase: crate::hooks::Phase,
        operation: Option<OperationId>,
    ) -> tembra::Result<ToolDecision> {
        if let Some(reason) = check_call(context, &self.spec, spec, call)? {
            return Ok(ToolDecision::Block(reason));
        }
        let args = match serde_json::from_value(call.arguments.clone()) {
            Ok(args) => args,
            Err(_) => {
                return Ok(ToolDecision::Block(
                    "tool arguments do not match their declared type".into(),
                ));
            }
        };
        let decision = self.policy_decision(context, call).await?;
        let decision = match decision {
            ToolDecision::Block(_) => decision,
            ToolDecision::Allow => self.tool.validate(context, &args).await?,
        };
        self.hook_decision(context, call, phase, operation, decision)
            .await
    }

    async fn policy_decision(
        &self,
        context: &Context,
        call: &ToolCall,
    ) -> tembra::Result<ToolDecision> {
        check_active(context)?;
        match &self.policy {
            Some(policy) => policy.check(context, &self.spec, call).await,
            None => Ok(ToolDecision::Allow),
        }
    }

    async fn hook_decision(
        &self,
        context: &Context,
        call: &ToolCall,
        phase: crate::hooks::Phase,
        operation: Option<OperationId>,
        decision: ToolDecision,
    ) -> tembra::Result<ToolDecision> {
        check_active(context)?;
        let decision = match &self.hooks {
            Some(hooks) => {
                hooks
                    .admit(
                        context,
                        crate::hooks::Admission {
                            phase,
                            operation,
                            spec: self.spec.clone(),
                            call: call.clone(),
                            decision,
                        },
                    )
                    .await?
            }
            None => decision,
        };
        check_active(context)?;
        Ok(decision)
    }
}

impl<T: Tool> Tools for ToolHandler<T> {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            self.admit(context, spec, call, crate::hooks::Phase::Preparation, None)
                .await
        }
        .boxed_local()
    }

    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            self.execute_with(context, execution, &mut self.context.for_dispatch())
                .await
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ToolResult>>> {
        async move {
            let outcome = async {
                check_direct(context, execution)?;
                if execution.call.name != self.spec.name {
                    return Err(invalid("recovery belongs to a different tool"));
                }
                let decision = self.policy_decision(context, execution.call).await?;
                let decision = self
                    .hook_decision(
                        context,
                        execution.call,
                        crate::hooks::Phase::Recovery,
                        Some(execution.operation),
                        decision,
                    )
                    .await?;
                if let ToolDecision::Block(reason) = decision {
                    return Err(tembra::Error::new(
                        Code::Forbidden,
                        "swarmie.tools.recovery_denied",
                        reason,
                    ));
                }
                // Saved arguments may predate the current schema. The author
                // reconciles original receipt identity and domain authority.
                self.tool
                    .recover(context, execution.operation, execution.call)
                    .await
            }
            .await;
            if let Some(hooks) = &self.hooks {
                let event = match &outcome {
                    Ok(recovered) => crate::hooks::Outcome::Recovered(recovered.clone()),
                    Err(error) => crate::hooks::Outcome::Unconfirmed(error.into()),
                };
                hooks
                    .finished(
                        context,
                        execution,
                        crate::hooks::Phase::Recovery,
                        event,
                        Default::default(),
                    )
                    .await;
            }
            outcome
        }
        .boxed_local()
    }
}

fn check_active(context: &Context) -> tembra::Result<()> {
    context.check()?;
    if context.signal().is_cancelled() {
        return Err(tembra::Error::new(
            Code::Cancelled,
            "swarmie.tools.cancelled",
            "tool invocation was cancelled",
        ));
    }
    Ok(())
}

fn check_direct(context: &Context, execution: ToolExecution<'_>) -> tembra::Result<()> {
    check_active(context)?;
    if execution.provider.is_some() {
        return Err(invalid(
            "author instances accept direct provider invocations only",
        ));
    }
    Ok(())
}

fn check_call(
    context: &Context,
    owned: &ToolSpec,
    offered: &ToolSpec,
    call: &ToolCall,
) -> tembra::Result<Option<String>> {
    check_active(context)?;
    if owned != offered || call.name != owned.name {
        return Ok(Some("tool definition or name changed".into()));
    }
    // This structural subset is feedback, never an authority decision.
    if validate::validate(&owned.input_schema, &call.arguments).is_err() {
        return Ok(Some(
            "tool arguments do not match their declared schema".into(),
        ));
    }
    Ok(None)
}

fn execution_result(
    outcome: Result<crate::ToolOutput, ToolExecutionError>,
) -> tembra::Result<ToolResult> {
    match outcome {
        Ok(output) => Ok(ToolResult::Success(output.into_value())),
        Err(error) if error.is_refusal() => Ok(ToolResult::Blocked(error.model_feedback().into())),
        Err(error) if error.is_confirmed() => Ok(ToolResult::Failed(error.model_feedback().into())),
        Err(error) => Err(tembra::Error::new(
            Code::Unavailable,
            "swarmie.tools.unknown",
            error.model_feedback(),
        )),
    }
}

/// Mount an author adapter as a normal Tembra provider service. The factory runs
/// under the acquiring principal and may reject it before publishing metadata.
/// Resource-owning authors declare their resources through `declaration`.
pub fn tool_orb<T: Tool>(
    service: impl Into<String>,
    create: impl Fn(&Declaration) -> tembra::Result<ToolHandler<T>> + 'static,
) -> Orb {
    let name = service.into();
    let service = Service::new(name.clone(), move |declaration| {
        create(&declaration)?.bind_hooks(&declaration)
    })
    .method(
        ToolMethod::Validate.as_str(),
        |tool, context, (spec, call): (ToolSpec, ToolCall)| async move {
            tool.validate(&context, &spec, &call).await
        },
    )
    .method(
        ToolMethod::Execute.as_str(),
        |tool, context, (operation, call): (OperationId, ToolCall)| async move {
            tool.execute(&context, ToolExecution::new(operation, &call))
                .await
        },
    )
    .method(
        ToolMethod::Recover.as_str(),
        |tool, context, (operation, call): (OperationId, ToolCall)| async move {
            tool.recover(&context, ToolExecution::new(operation, &call))
                .await
        },
    );
    let service = service.method(
        crate::hooks::FAILURE_METHOD,
        |tool, _ctx, (): ()| async move { Ok(tool.observation_failure()) },
    );
    crate::hooks::Hooks::default().declare(Orb::new(name, 1).service(service))
}
