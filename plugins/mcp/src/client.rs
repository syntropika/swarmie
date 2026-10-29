use crate::{
    Configuration, Protocol, SERVICE,
    discovery::{self, DiscoveredTool},
    error::{projection, unknown},
    invalid,
    projection::model_result,
    transport::{Session, open},
    unavailable,
};
use futures::{
    FutureExt,
    future::{LocalBoxFuture, Shared},
};
use rmcp::{
    RoleClient,
    model::{CallToolRequestParams, CallToolResponse, CallToolResult},
    service::{Peer, ServiceError},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use swarmie_tools::{
    OperationId, Recovery, ToolCall, ToolDecision, ToolDefinition, ToolExecution, ToolName,
    ToolProvider, ToolResult, ToolSpec, Tools,
};
use tembra::Context;

/// Observed negotiated identity and full remote tool metadata, without credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub status: ConnectionStatus,
    pub protocol: Option<Protocol>,
    pub server: Option<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Disabled,
    Unavailable,
    Closed,
}

struct Attempt {
    call: ToolCall,
    result: Option<ToolResult>,
    response: Option<CallToolResult>,
}

type Closing = Shared<LocalBoxFuture<'static, tembra::Result<()>>>;

/// One host-selected principal binding and one physical MCP connection.
///
/// Connection does not install software, create a runtime or grant authority to a
/// discovered tool. The host must keep Tokio running through [`Self::close`].
/// Receipts are in memory only; after restart an unrecognized outcome is Unknown.
pub struct Connection {
    principal: String,
    provider: ToolProvider,
    configuration: Configuration,
    observation: Observation,
    tools: BTreeMap<ToolName, DiscoveredTool>,
    peer: Peer<RoleClient>,
    running: RefCell<Option<Session>>,
    closing: RefCell<Option<Closing>>,
    status: Cell<ConnectionStatus>,
    attempts: RefCell<BTreeMap<OperationId, Attempt>>,
}

impl Connection {
    /// Connect deliberately, then discover a complete bounded tool catalog.
    /// No OAuth interaction, installation or alternate-version retry is performed.
    /// Cancel the supplied context and keep polling to await physical cleanup.
    /// Dropping this future independently requests child termination via Drop but
    /// does not acknowledge process reaping; managed calls retain physical ownership.
    pub async fn connect(configuration: Configuration, context: &Context) -> tembra::Result<Self> {
        let provider = ToolProvider {
            service: SERVICE.into(),
            key: vec![configuration.id.as_str().into()],
        };
        Self::connect_bound(configuration, context, provider).await
    }

    pub(crate) async fn connect_bound(
        configuration: Configuration,
        context: &Context,
        provider: ToolProvider,
    ) -> tembra::Result<Self> {
        context.check()?;
        let principal = context.principal()?;
        let signal = context.signal();
        configuration.validate()?;
        let deadline = tokio::time::Instant::now()
            .checked_add(configuration.limits.connect_timeout)
            .ok_or_else(|| invalid("MCP connection deadline exceeds the clock range"))?;
        if principal.is_empty() {
            return Err(invalid("MCP connection requires its owning principal"));
        }
        tokio::runtime::Handle::try_current()
            .map_err(|_| unavailable("MCP connection requires a host-provided Tokio runtime"))?;
        let mut session = open(&configuration, &signal, deadline).await?;
        let discovered = async {
            let server = session
                .running
                .peer_info()
                .ok_or_else(|| invalid("MCP server identity is missing"))?;
            if server.protocol_version != configuration.protocol.version() {
                return Err(invalid(
                    "MCP server selected an unsupported protocol revision",
                ));
            }
            let tools = if server.capabilities.tools.is_some() {
                discovery::discover(session.running.peer(), &configuration, &signal, deadline)
                    .await?
            } else {
                BTreeMap::new()
            };
            signal.check()?;
            let observation = Observation {
                status: ConnectionStatus::Connected,
                protocol: Some(configuration.protocol),
                server: Some(serde_json::to_value(server.as_ref()).map_err(projection)?),
                tools: tools
                    .values()
                    .map(|tool| serde_json::to_value(&tool.remote).map_err(projection))
                    .collect::<tembra::Result<_>>()?,
            };
            Ok((observation, tools))
        }
        .await;
        let (observation, tools) = match discovered {
            Ok(value) => value,
            Err(error) => {
                let _cleanup = session.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            principal,
            provider,
            observation,
            configuration,
            tools,
            peer: session.running.peer().clone(),
            running: RefCell::new(Some(session)),
            closing: RefCell::new(None),
            status: Cell::new(ConnectionStatus::Connected),
            attempts: RefCell::new(BTreeMap::new()),
        })
    }

    pub fn observation(&self) -> Observation {
        let mut observation = self.observation.clone();
        observation.status = if self.status.get() == ConnectionStatus::Connected
            && self.peer.is_transport_closed()
        {
            ConnectionStatus::Unavailable
        } else {
            self.status.get()
        };
        observation
    }

    pub(crate) fn enable(&self) -> tembra::Result<()> {
        if !matches!(
            self.status.get(),
            ConnectionStatus::Connected | ConnectionStatus::Disabled
        ) || self.peer.is_transport_closed()
        {
            return Err(unavailable(
                "MCP connection is unavailable; explicit replacement is required",
            ));
        }
        self.status.set(ConnectionStatus::Connected);
        Ok(())
    }

    pub fn provider(&self) -> ToolProvider {
        self.provider.clone()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| ToolDefinition {
                spec: tool.spec.clone(),
                provider: self.provider(),
            })
            .collect()
    }

    /// Stops new admission. Already dispatched operations retain their outcomes.
    pub fn disable(&self) {
        if self.status.get() == ConnectionStatus::Connected {
            self.status.set(ConnectionStatus::Disabled);
        }
    }

    /// Await SDK transport cleanup. Cancellation cannot reverse remote effects.
    /// Dropping this future does not acknowledge cleanup; a later call resumes it.
    pub async fn close(&self) -> tembra::Result<()> {
        self.status.set(ConnectionStatus::Closed);
        let closing = {
            let mut closing = self.closing.borrow_mut();
            closing
                .get_or_insert_with(|| {
                    let running = self.running.borrow_mut().take();
                    async move {
                        if let Some(mut running) = running {
                            running
                                .close()
                                .await
                                .map_err(|_| unavailable("MCP transport cleanup failed"))?;
                        }
                        Ok(())
                    }
                    .boxed_local()
                    .shared()
                })
                .clone()
        };
        closing.await
    }

    /// Read the original MCP result under its original principal and intent.
    /// Private `_meta` is retained here and excluded from model-facing ToolResult.
    pub fn result(
        &self,
        context: &Context,
        operation: OperationId,
        call: &ToolCall,
    ) -> tembra::Result<Option<serde_json::Value>> {
        self.authority(context, None)?;
        let attempts = self.attempts.borrow();
        let Some(attempt) = attempts.get(&operation) else {
            return Ok(None);
        };
        if &attempt.call != call {
            return Err(invalid(
                "MCP result intent differs from the original operation",
            ));
        }
        attempt
            .response
            .as_ref()
            .map(|response| serde_json::to_value(response).map_err(projection))
            .transpose()
    }

    fn authority(
        &self,
        context: &Context,
        execution: Option<ToolExecution<'_>>,
    ) -> tembra::Result<()> {
        context.check()?;
        if context.principal()? != self.principal
            || execution
                .and_then(|execution| execution.provider)
                .is_some_and(|provider| provider != &self.provider())
        {
            return Err(invalid(
                "MCP operation belongs to another principal or provider",
            ));
        }
        Ok(())
    }

    fn mark_unavailable(&self) {
        if self.status.get() != ConnectionStatus::Closed {
            self.status.set(ConnectionStatus::Unavailable);
        }
    }

    fn admitted(&self, call: &ToolCall) -> bool {
        self.status.get() == ConnectionStatus::Connected
            && !self.peer.is_transport_closed()
            && self.tools.contains_key(&call.name)
            && call.arguments.is_object()
    }
}

impl Tools for Connection {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            self.authority(context, None)?;
            Ok(
                if self.admitted(call)
                    && self
                        .tools
                        .get(&call.name)
                        .is_some_and(|tool| &tool.spec == spec)
                {
                    ToolDecision::Allow
                } else {
                    ToolDecision::Block(
                        "MCP tool definition, arguments or connection is unavailable".into(),
                    )
                },
            )
        }
        .boxed_local()
    }

    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            self.authority(context, Some(execution))?;
            let call = execution.call;
            if let Some(previous) = self.attempts.borrow().get(&execution.operation) {
                if &previous.call != call {
                    return Err(invalid(
                        "MCP operation identity was reused with different intent",
                    ));
                }
                return if let Some(response) = &previous.response {
                    model_result(response)
                } else {
                    previous.result.clone().ok_or_else(|| {
                        unknown("MCP outcome is unknown; automatic replay is forbidden")
                    })
                };
            }
            if !self.admitted(call) {
                return Ok(ToolResult::Blocked(
                    "MCP connection does not admit this tool call".into(),
                ));
            }
            if self.attempts.borrow().len() >= self.configuration.limits.max_operations {
                return Ok(ToolResult::Blocked(
                    "MCP operation receipt limit reached".into(),
                ));
            }
            let tool = &self.tools[&call.name];
            let params = CallToolRequestParams::new(tool.remote.name.clone()).with_arguments(
                call.arguments
                    .as_object()
                    .cloned()
                    .ok_or_else(|| invalid("MCP arguments must be an object"))?,
            );
            self.attempts.borrow_mut().insert(
                execution.operation,
                Attempt {
                    call: call.clone(),
                    result: None,
                    response: None,
                },
            );
            // Logical cancellation does not abandon the dispatched physical call.
            // Retain its correlated reply for recovery until the request deadline.
            let response = match tokio::time::timeout(
                self.configuration.limits.request_timeout,
                self.peer.call_tool_once(params),
            )
            .await
            {
                Ok(response) => response,
                Err(_) => {
                    if context.check().is_ok() || self.peer.is_transport_closed() {
                        self.mark_unavailable();
                    }
                    context.check()?;
                    return Err(unknown("MCP reply timed out; outcome is unknown"));
                }
            };
            let result = match response {
                Ok(CallToolResponse::Complete(result)) => {
                    self.attempts
                        .borrow_mut()
                        .get_mut(&execution.operation)
                        .ok_or_else(|| invalid("MCP operation receipt is missing"))?
                        .response = Some(result.clone());
                    let projected = model_result(&result);
                    if projected.is_err() {
                        context.check()?;
                    }
                    projected?
                }
                Err(ServiceError::McpError(error)) => ToolResult::Failed(
                    serde_json::json!({"code":error.code,"message":error.message}).to_string(),
                ),
                Err(ServiceError::Cancelled { .. }) => {
                    context.check()?;
                    return Err(unknown("MCP operation was cancelled; outcome is unknown"));
                }
                Err(_) => {
                    self.mark_unavailable();
                    context.check()?;
                    return Err(unknown(
                        "MCP returned no supported final outcome; reconciliation is required",
                    ));
                }
                Ok(_) => {
                    context.check()?;
                    return Err(unknown(
                        "MCP returned an unsupported continuation; outcome is retained as unknown",
                    ));
                }
            };
            self.attempts
                .borrow_mut()
                .get_mut(&execution.operation)
                .ok_or_else(|| invalid("MCP operation receipt is missing"))?
                .result = Some(result.clone());
            context.check()?;
            Ok(result)
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ToolResult>>> {
        async move {
            self.authority(context, Some(execution))?;
            let attempts = self.attempts.borrow();
            let Some(attempt) = attempts.get(&execution.operation) else {
                return Ok(Recovery::Unknown);
            };
            if &attempt.call != execution.call {
                return Err(invalid(
                    "MCP recovery intent differs from the original operation",
                ));
            }
            if let Some(response) = &attempt.response {
                return model_result(response).map(Recovery::Completed);
            }
            Ok(attempt
                .result
                .clone()
                .map(Recovery::Completed)
                .unwrap_or(Recovery::Unknown))
        }
        .boxed_local()
    }
}
