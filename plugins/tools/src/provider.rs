use crate::{OperationId, Recovery, ToolCall, ToolResult, ToolSpec, invalid};
use futures::{FutureExt, future::LocalBoxFuture};
use serde::{Deserialize, Serialize};
use tembra::{Context, Declaration, Host, Reference, Resource};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum ToolDecision {
    Allow,
    Block(String),
}

/// Stable provider address, not an authority token or a live reference.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolProvider {
    pub service: String,
    pub key: Vec<String>,
}

impl ToolProvider {
    pub fn new(service: impl Into<String>, key: Vec<String>) -> tembra::Result<Self> {
        let provider = Self {
            service: service.into(),
            key,
        };
        provider.validate()?;
        Ok(provider)
    }

    pub fn validate(&self) -> tembra::Result<()> {
        if self.service.is_empty()
            || self.service.chars().any(char::is_control)
            || self.key.is_empty()
        {
            return Err(invalid(
                "provider service and key must be nonempty; service must contain no controls",
            ));
        }
        Ok(())
    }
}

/// Confirm this address with the final arguments before external dispatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPreparation {
    pub decision: ToolDecision,
    pub provider: Option<ToolProvider>,
}

/// Borrowed execution intent. Recovery must retain both identity and provider.
#[derive(Clone, Copy)]
pub struct ToolExecution<'a> {
    pub operation: OperationId,
    pub call: &'a ToolCall,
    pub provider: Option<&'a ToolProvider>,
}

impl<'a> ToolExecution<'a> {
    pub fn new(operation: OperationId, call: &'a ToolCall) -> Self {
        Self {
            operation,
            call,
            provider: None,
        }
    }
}

pub trait Tools {
    /// Validate final arguments and current permission before external dispatch.
    /// Providers also enforce authority at execute/recover, including receipt access.
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>>;

    /// Direct implementations have no routed address. Catalog clients pin theirs.
    fn prepare<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            Ok(ToolPreparation {
                decision: self.validate(context, spec, call).await?,
                provider: None,
            })
        }
        .boxed_local()
    }

    /// Recheck admission without sending pending arguments to a replacement.
    /// Routers must compare the saved target before invoking validation. The
    /// default supports direct providers only and fails closed for routed intent.
    fn prepare_retry<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            if execution.provider.is_some() {
                return Ok(ToolPreparation {
                    decision: ToolDecision::Block(
                        "provider does not support routed retry admission".into(),
                    ),
                    provider: None,
                });
            }
            self.prepare(context, spec, execution.call).await
        }
        .boxed_local()
    }

    /// Err means no confirmed outcome. Known failures belong in ToolResult::Failed.
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>>;

    /// Unknown outcomes never authorize replay. Only the provider can permit retry.
    fn recover<'a>(
        &'a self,
        _context: &'a Context,
        _execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ToolResult>>> {
        async { Ok(Recovery::Unknown) }.boxed_local()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolMethod {
    Validate,
    Execute,
    Recover,
}

impl ToolMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validate => "validate",
            Self::Execute => "execute",
            Self::Recover => "recover",
        }
    }
}

pub struct ToolService {
    reference: Resource<Reference>,
    provider: ToolProvider,
}

impl ToolService {
    pub fn bind(
        declaration: &Declaration,
        host: Host,
        service: impl Into<String>,
        key: Vec<String>,
    ) -> tembra::Result<Self> {
        let provider = ToolProvider::new(service, key)?;
        Ok(Self {
            reference: bind(declaration, host, &provider)?,
            provider,
        })
    }

    fn check(&self, execution: ToolExecution<'_>) -> tembra::Result<()> {
        if execution
            .provider
            .is_some_and(|provider| provider != &self.provider)
        {
            return Err(invalid("pending tool belongs to another provider"));
        }
        Ok(())
    }
}

impl Tools for ToolService {
    fn prepare_retry<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            self.check(execution)?;
            self.prepare(context, spec, execution.call).await
        }
        .boxed_local()
    }

    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            self.reference
                .value()?
                .call(
                    context,
                    ToolMethod::Validate.as_str(),
                    (spec.clone(), call.clone()),
                )
                .await
        }
        .boxed_local()
    }

    fn prepare<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            Ok(ToolPreparation {
                decision: self.validate(context, spec, call).await?,
                provider: Some(self.provider.clone()),
            })
        }
        .boxed_local()
    }

    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            self.check(execution)?;
            self.reference
                .value()?
                .call(
                    context,
                    ToolMethod::Execute.as_str(),
                    (execution.operation, execution.call.clone()),
                )
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
            self.check(execution)?;
            self.reference
                .value()?
                .call(
                    context,
                    ToolMethod::Recover.as_str(),
                    (execution.operation, execution.call.clone()),
                )
                .await
        }
        .boxed_local()
    }
}

pub(crate) fn bind(
    declaration: &Declaration,
    host: Host,
    provider: &ToolProvider,
) -> tembra::Result<Resource<Reference>> {
    provider.validate()?;
    let principal = declaration.address()?.principal;
    let resource = serde_json::to_string(&("tools.provider", provider)).map_err(invalid)?;
    let provider = provider.clone();
    declaration.resource(
        &resource,
        1,
        move || host.get(&provider.service, provider.key, &principal),
        |reference, _| async move { reference.release() }.boxed_local(),
    )
}
