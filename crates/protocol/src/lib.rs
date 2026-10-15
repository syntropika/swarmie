use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use swarmie::*;
use tokio::sync::RwLock;
mod schema;
pub use schema::protocol_schema;

pub const PROTOCOL_VERSION: &str = "1";
#[derive(Clone)]
pub struct RuntimeService {
    runtime: Runtime,
    stop: tokio::sync::watch::Sender<bool>,
    agents: Arc<RwLock<BTreeMap<String, Agent>>>,
    agent_tools: Option<Arc<dyn AgentToolSetupPort>>,
}
impl RuntimeService {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            stop: tokio::sync::watch::channel(false).0,
            agents: Arc::default(),
            agent_tools: None,
        }
    }
    pub fn with_agent_tools(mut self, setup: Arc<dyn AgentToolSetupPort>) -> Self {
        self.runtime.install_agent_tool_setup(setup.clone());
        self.agent_tools = Some(setup);
        self
    }
    fn agent_tools(&self) -> Result<&Arc<dyn AgentToolSetupPort>> {
        self.agent_tools.as_ref().ok_or_else(|| {
            Error::Unsupported("Agent tool setup is unavailable on this runtime".into())
        })
    }
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    /// Wait for an explicit owner stop request, including requests made before subscription.
    pub async fn wait_for_shutdown(&self) {
        let mut stop = self.stop.subscribe();
        while !*stop.borrow_and_update() {
            if stop.changed().await.is_err() {
                return;
            }
        }
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.runtime.shutdown().await?;
        self.agents.write().await.clear();
        Ok(())
    }
    async fn agent(&self, p: &Value) -> Result<Agent> {
        let id = required(p, "sessionId")?;
        self.runtime
            .agent(id)
            .await
            .ok_or_else(|| Error::Invalid(format!("session is not open: {id}")))
    }
    async fn retain(&self, a: Agent) -> Result<Value> {
        let snapshot = a.snapshot().await;
        self.agents.write().await.insert(snapshot.id.clone(), a);
        Ok(serde_json::to_value(snapshot)?)
    }
    // Keep native reconnection and catalog futures out of the common RPC dispatch frame.
    async fn invoke_mcp(&self, method: &str, p: Value) -> Result<Value> {
        match method {
            "mcp/reopen" => {
                self.retain(self.runtime.mcp_reopen(required(&p, "sessionId")?).await?)
                    .await
            }
            "mcp/list" => {
                let query: McpQuery = serde_json::from_value(p)
                    .map_err(|_| Error::Invalid("Invalid MCP catalog query".into()))?;
                Ok(json!(self.runtime.mcp_catalog(query).await?))
            }
            "mcp/mutate" => {
                let mutation: McpMutation = serde_json::from_value(p)
                    .map_err(|_| Error::Invalid("Invalid MCP operation".into()))?;
                Ok(json!(self.runtime.mcp_mutate(mutation).await?))
            }
            _ => Err(Error::Invalid("Unknown MCP operation".into())),
        }
    }

    pub async fn invoke(&self, method: &str, p: Value) -> Result<Value> {
        match method {
            "mcp/reopen" | "mcp/list" | "mcp/mutate" => Box::pin(self.invoke_mcp(method, p)).await,
            "runtime/shutdown" => {
                self.stop.send_replace(true);
                Ok(Value::Null)
            }
            "directory/get" => {
                let reference: DirectoryRef =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(json!(
                    self.runtime.local_catalog().await?.directory(&reference)?
                ))
            }
            "directory/register" => {
                let args: DirectoryRegistration =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime.register_directory(&args.path).await?,
                )?)
            }
            "project/create" => {
                let args: ProjectCreate =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime
                        .create_project(args.name, args.directory, args.launch)
                        .await?,
                )?)
            }
            "project/list" => Ok(json!(
                self.runtime
                    .local_catalog()
                    .await?
                    .projects
                    .into_values()
                    .collect::<Vec<_>>()
            )),
            "project/get" => Ok(json!(
                self.runtime.project(required(&p, "projectId")?).await?
            )),
            "project/update" => {
                let args: ProjectUpdate =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(json!(self.runtime.update_project(args).await?))
            }
            "settings/directory" => {
                Ok(json!(self.runtime.local_catalog().await?.default_directory))
            }
            "settings/setDirectory" => {
                let args: DefaultDirectoryUpdate =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(json!(
                    self.runtime
                        .set_default_directory(args.expected_revision, args.directory)
                        .await?
                ))
            }
            "session/context" => Ok(json!(
                self.runtime
                    .session_context(required(&p, "sessionId")?)
                    .await?
            )),
            "runtime/initialize" => Ok(
                json!({"name":"swarmie","version":env!("CARGO_PKG_VERSION"),"protocolVersion":PROTOCOL_VERSION,"methods":METHODS,"eventHistoryLimit":4096,"transcriptDurability":self.runtime.transcript_durability(),"runtimeIdentity":self.runtime.identity().await?}),
            ),
            "runtime/schema" => Ok(protocol_schema()),
            "runtime/agentToolsSetup" => {
                self.agent_tools()?
                    .descriptor(required(&p, "harness")?)
                    .await
            }
            "runtime/agentToolsConnect" => self.agent_tools()?.start(p).await,
            "runtime/agentToolsFlow" => self.agent_tools()?.flow(required(&p, "id")?).await,
            "runtime/agentToolsInput" => {
                self.agent_tools()?
                    .input(
                        required(&p, "id")?,
                        p.get("values").cloned().ok_or_else(|| {
                            Error::Invalid("Connection values are required".into())
                        })?,
                    )
                    .await
            }
            "runtime/agentToolsCancel" => self.agent_tools()?.cancel(required(&p, "id")?).await,
            "runtime/agentToolsDisconnect" => {
                self.agent_tools()?
                    .disconnect(required(&p, "harness")?)
                    .await
            }
            "runtime/agentToolsInstall" => {
                self.agent_tools()?.install(required(&p, "harness")?).await
            }
            "runtime/discover" => {
                let mut tools = self.runtime.discover().await?;
                if let Some(setup) = &self.agent_tools {
                    for tool in &mut tools {
                        let descriptor = setup.descriptor(&tool.harness).await?;
                        if descriptor["manual"] != true {
                            tool.available =
                                descriptor["installed"].as_bool().unwrap_or(tool.available);
                        }
                    }
                }
                Ok(json!(tools))
            }
            "runtime/configuration" => {
                let options: SpawnOptions =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                Ok(json!(self.runtime.preview_configuration(options).await?))
            }
            "runtime/probe" => {
                let harness = required(&p, "harness")?;
                let mut probe = self.runtime.probe(harness).await?;
                if let Some(setup) = &self.agent_tools {
                    let descriptor = setup.descriptor(harness).await?;
                    if descriptor["manual"] != true {
                        probe.available =
                            descriptor["installed"].as_bool().unwrap_or(probe.available);
                        probe.auth_state =
                            descriptor["authState"].as_str().unwrap_or("unknown").into();
                        probe.message = descriptor["message"].as_str().map(String::from);
                    }
                }
                Ok(json!(probe))
            }
            "runtime/history" => {
                let query: NativeHistoryQuery =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime.native_history(query).await?,
                )?)
            }
            "runtime/history/read" | "session/import" => {
                let native_id = required(&p, "nativeId")?.to_string();
                let query: NativeHistoryQuery =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                if method == "session/import" {
                    Ok(serde_json::to_value(
                        self.runtime
                            .import_native_history(query, &native_id)
                            .await?,
                    )?)
                } else {
                    Ok(serde_json::to_value(
                        self.runtime.read_native_history(query, &native_id).await?,
                    )?)
                }
            }
            "runtime/quotas" => {
                let query: AccountQuery =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                self.runtime.account_quotas(query).await
            }
            "runtime/quotas/all" => {
                let queries: Vec<AccountQuery> = serde_json::from_value(p["queries"].clone())
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime.account_quotas_all(queries).await?,
                )?)
            }
            "runtime/generate" => {
                let options: SpawnOptions =
                    serde_json::from_value(p.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                let timeout = p
                    .get("timeoutMs")
                    .map(|value| {
                        value
                            .as_u64()
                            .filter(|value| *value > 0 && *value <= 600_000)
                            .ok_or_else(|| {
                                Error::Invalid("timeoutMs must be between 1 and 600000".into())
                            })
                    })
                    .transpose()?
                    .unwrap_or(120_000);
                Ok(Value::String(
                    self.runtime
                        .generate_with_timeout(
                            options,
                            prompt(&p)?,
                            std::time::Duration::from_millis(timeout),
                        )
                        .await?,
                ))
            }
            "extension/list" => Ok(serde_json::to_value(self.runtime.extensions())?),
            "extension/invoke" => {
                self.runtime
                    .invoke_extension(
                        required(&p, "extensionId")?,
                        required(&p, "command")?,
                        p.get("arguments").cloned().unwrap_or_else(|| json!({})),
                    )
                    .await
            }
            "session/tree" => Ok(serde_json::to_value(
                self.runtime
                    .session_tree(required(&p, "sessionId")?)
                    .await?,
            )?),
            "session/list" => Ok(serde_json::to_value(self.runtime.sessions().await?)?),
            "agent/spawn" | "session/create" => {
                let o: SpawnOptions =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                self.retain(self.runtime.spawn(o).await?).await
            }
            "session/resume" => {
                let id = required(&p, "sessionId")?;
                let agent = if let Some(execution) = p.get("execution") {
                    self.runtime
                        .resume_with(
                            id,
                            serde_json::from_value(execution.clone())
                                .map_err(|e| Error::Invalid(e.to_string()))?,
                        )
                        .await?
                } else {
                    self.runtime.resume(id).await?
                };
                self.retain(agent).await
            }
            "session/fork" => {
                let id = required(&p, "sessionId")?;
                let message_id = p
                    .get("messageId")
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| Error::Invalid("messageId must be a string".into()))
                    })
                    .transpose()?;
                let execution = p
                    .get("execution")
                    .map(|value| {
                        serde_json::from_value(value.clone())
                            .map_err(|error| Error::Invalid(error.to_string()))
                    })
                    .transpose()?;
                let position = p
                    .get("position")
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| Error::Invalid("position must be a string".into()))
                    })
                    .transpose()?
                    .unwrap_or("after_assistant");
                if !matches!(position, "after_assistant" | "before_user") {
                    return Err(Error::Invalid("invalid fork position".into()));
                }
                if position == "before_user" {
                    let message_id = message_id
                        .ok_or_else(|| Error::Invalid("before_user requires messageId".into()))?;
                    return self
                        .retain(
                            self.runtime
                                .fork_before_user(id, message_id, execution)
                                .await?,
                        )
                        .await;
                }
                let agent = match (message_id, execution) {
                    (Some(message_id), Some(execution)) => {
                        self.runtime.fork_at_with(id, message_id, execution).await?
                    }
                    (Some(message_id), None) => self.runtime.fork_at(id, message_id).await?,
                    (None, Some(execution)) => self.runtime.fork_with(id, execution).await?,
                    (None, None) => self.runtime.fork(id).await?,
                };
                self.retain(agent).await
            }
            "session/get" => Ok(serde_json::to_value(
                self.runtime.snapshot(required(&p, "sessionId")?).await?,
            )?),
            "session/metadata" => {
                let update: SessionMetadataUpdate =
                    serde_json::from_value(p.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime
                        .update_metadata(required(&p, "sessionId")?, update)
                        .await?,
                )?)
            }
            "session/transcript" => Ok(serde_json::to_value(
                self.runtime
                    .transcript(
                        required(&p, "sessionId")?,
                        p["afterSequence"].as_u64().unwrap_or(0),
                        event_limit(&p)?,
                    )
                    .await?,
            )?),
            "session/queue/list" => Ok(serde_json::to_value(
                self.runtime
                    .snapshot(required(&p, "sessionId")?)
                    .await?
                    .queue,
            )?),
            "session/queue" => Ok(serde_json::to_value(
                self.agent(&p).await?.enqueue(prompt(&p)?).await?,
            )?),
            "session/queue/update" => {
                self.agent(&p)
                    .await?
                    .update_queued(required(&p, "id")?, prompt(&p)?)
                    .await?;
                Ok(Value::Null)
            }
            "session/queue/remove" => {
                self.agent(&p)
                    .await?
                    .remove_queued(required(&p, "id")?)
                    .await?;
                Ok(Value::Null)
            }
            "session/queue/pause" => {
                let paused = p["paused"]
                    .as_bool()
                    .ok_or_else(|| Error::Invalid("paused must be boolean".into()))?;
                self.agent(&p).await?.pause_queue(paused).await?;
                Ok(Value::Null)
            }
            "session/events" => {
                let a = self.agent(&p).await?;
                let after = p["afterSequence"].as_u64().unwrap_or(0);
                let limit = event_limit(&p)?;
                Ok(serde_json::to_value(
                    a.replay(after)
                        .await?
                        .into_iter()
                        .take(limit)
                        .collect::<Vec<_>>(),
                )?)
            }
            "session/prompt" => {
                let receipt = self
                    .runtime
                    .submit(
                        required(&p, "sessionId")?,
                        required(&p, "idempotencyKey")?,
                        prompt(&p)?,
                    )
                    .await?;
                Ok(json!(receipt))
            }
            "session/submission" => Ok(json!(
                self.runtime
                    .submission(required(&p, "sessionId")?, required(&p, "idempotencyKey")?)
                    .await?
            )),
            "session/submission/reconcile" => Ok(json!(
                self.runtime
                    .reconcile_submission(
                        required(&p, "sessionId")?,
                        required(&p, "idempotencyKey")?
                    )
                    .await?
            )),
            "session/steer" => {
                self.agent(&p).await?.steer(prompt(&p)?).await?;
                Ok(json!({"accepted":true}))
            }
            "session/cancel" => {
                let agent = self.agent(&p).await?;
                if let Some(turn) = p.get("turnId") {
                    agent
                        .cancel_turn(
                            turn.as_str()
                                .ok_or_else(|| Error::Invalid("turnId must be a string".into()))?,
                        )
                        .await?;
                } else {
                    agent.cancel().await?;
                }
                Ok(Value::Null)
            }
            "permission/respond" => {
                let r: PermissionResponse =
                    serde_json::from_value(p.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                self.agent(&p).await?.respond_permission(r).await?;
                Ok(Value::Null)
            }
            "agent/configure" => Ok(json!(
                self.agent(&p).await?.configure(p["config"].clone()).await?
            )),
            "agent/models" => Ok(json!(self.agent(&p).await?.models().await?)),
            "agent/configuration" => Ok(json!(self.agent(&p).await?.configuration().await?)),
            "agent/commands" => self.agent(&p).await?.slash_commands().await,
            "session/tasks" => Ok(json!(self.agent(&p).await?.tasks().await?)),
            "session/task/stop" => {
                self.agent(&p)
                    .await?
                    .stop_task(required(&p, "taskId")?)
                    .await?;
                Ok(Value::Null)
            }
            "agent/quotas" => self.agent(&p).await?.quotas().await,
            "session/rollback" => {
                let n = p["turns"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| Error::Invalid("turns must be positive".into()))?;
                let scope = p
                    .get("scope")
                    .map(|value| {
                        serde_json::from_value(value.clone())
                            .map_err(|error| Error::Invalid(error.to_string()))
                    })
                    .transpose()?
                    .unwrap_or_default();
                let expected = p
                    .get("expectedSequence")
                    .map(|value| {
                        value.as_u64().ok_or_else(|| {
                            Error::Invalid("expectedSequence must be an unsigned integer".into())
                        })
                    })
                    .transpose()?;
                self.agent(&p)
                    .await?
                    .rollback_at(n, scope, expected)
                    .await?;
                Ok(Value::Null)
            }
            "session/compact" => {
                self.agent(&p).await?.compact().await?;
                Ok(Value::Null)
            }
            "session/close" | "agent/kill" => {
                let a = self.agent(&p).await?;
                if method == "agent/kill" {
                    a.kill().await?;
                } else {
                    a.close().await?;
                }
                self.agents.write().await.remove(required(&p, "sessionId")?);
                Ok(Value::Null)
            }
            _ => Err(Error::Unsupported(format!("method {method}"))),
        }
    }
    /// JSON-RPC 2.0 requests, notifications and batches share one dispatcher.
    pub async fn handle(&self, request: Value) -> Option<Value> {
        if let Some(batch) = request.as_array() {
            if batch.is_empty() {
                return Some(rpc_error(Value::Null, -32600, "empty batch"));
            }
            let mut responses = vec![];
            for r in batch {
                if let Some(response) = self.single(r.clone()).await {
                    responses.push(response);
                }
            }
            if responses.is_empty() {
                None
            } else {
                Some(Value::Array(responses))
            }
        } else {
            self.single(request).await
        }
    }
    async fn single(&self, r: Value) -> Option<Value> {
        let id = r.get("id").cloned();
        if !r.is_object()
            || r["jsonrpc"] != "2.0"
            || r["method"].as_str().is_none()
            || id
                .as_ref()
                .is_some_and(|v| !v.is_null() && !v.is_string() && !v.is_number())
            || r.get("params")
                .is_some_and(|p| !p.is_object() && !p.is_array())
        {
            return Some(rpc_error(Value::Null, -32600, "invalid JSON-RPC request"));
        }
        // Target checks run at the receiving owner, before any method dispatch.
        if let Some(expected) = r.get("expectedRuntime") {
            let expected = match serde_json::from_value::<RuntimeIdentity>(expected.clone()) {
                Ok(expected) => expected,
                Err(_) => return id.map(|id| rpc_error(id, -32602, "invalid expectedRuntime")),
            };
            match self.runtime.identity().await {
                Ok(actual) if actual == expected => {}
                Ok(_) => {
                    return id.map(|id| {
                        let mut response = rpc_error(
                            id,
                            -32005,
                            "runtime target changed; reconnect before sending commands",
                        );
                        response["error"]["data"] =
                            json!({"code":"runtime_target_mismatch","effectKnowledge":"none"});
                        response
                    });
                }
                Err(error) => return id.map(|id| rpc_error(id, -32000, &error.to_string())),
            }
        }
        let method = r["method"].as_str().unwrap();
        let result = if !METHODS.contains(&method) {
            Err(Error::Unsupported(format!("method {method}")))
        } else if r["params"].is_array() {
            Err(Error::Invalid("named parameters required".into()))
        } else {
            self.invoke(
                method,
                r.get("params").cloned().unwrap_or_else(|| json!({})),
            )
            .await
        };
        id.map(|id| match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(e) => {
                let code = match &e {
                    Error::Unsupported(_) if !METHODS.contains(&method) => -32601,
                    Error::Invalid(_) | Error::Json(_) => -32602,
                    Error::Unsupported(_) => -32001,
                    Error::IdempotencyConflict => -32004,
                    Error::Busy => -32002,
                    Error::Closed => -32003,
                    _ => -32000,
                };
                let mut response = rpc_error(id, code, &e.to_string());
                if method == "session/prompt" {
                    let (failure, effect) = match &e {
                        Error::IdempotencyConflict => ("idempotency_conflict", "none"),
                        Error::Invalid(_) | Error::Json(_) => ("invalid_input", "none"),
                        Error::Busy => ("unavailable", "none"),
                        Error::Unsupported(_) => ("unavailable", "none"),
                        Error::Closed => ("unavailable", "none"),
                        Error::Storage(_) | Error::Io(_) => ("storage_failure", "unknown"),
                        _ => ("native_failure", "unknown"),
                    };
                    response["error"]["data"] = json!({
                        "code":failure,"effectKnowledge":effect,
                        "recovery":{"method":"session/submission","sessionId":r["params"]["sessionId"],"idempotencyKey":r["params"]["idempotencyKey"]}
                    });
                }
                response
            }
        })
    }
}
pub const METHODS: &[&str] = &[
    "mcp/reopen",
    "mcp/list",
    "mcp/mutate",
    "directory/get",
    "directory/register",
    "project/create",
    "project/list",
    "project/get",
    "project/update",
    "settings/directory",
    "settings/setDirectory",
    "session/context",
    "runtime/initialize",
    "runtime/shutdown",
    "runtime/schema",
    "runtime/discover",
    "runtime/agentToolsSetup",
    "runtime/agentToolsConnect",
    "runtime/agentToolsFlow",
    "runtime/agentToolsInput",
    "runtime/agentToolsCancel",
    "runtime/agentToolsDisconnect",
    "runtime/agentToolsInstall",
    "runtime/configuration",
    "runtime/probe",
    "runtime/history",
    "runtime/history/read",
    "session/import",
    "runtime/quotas",
    "runtime/quotas/all",
    "runtime/generate",
    "extension/list",
    "extension/invoke",
    "agent/spawn",
    "session/create",
    "session/list",
    "session/tree",
    "session/resume",
    "session/fork",
    "session/get",
    "session/metadata",
    "session/transcript",
    "session/queue",
    "session/queue/list",
    "session/queue/update",
    "session/queue/remove",
    "session/queue/pause",
    "session/events",
    "session/prompt",
    "session/submission",
    "session/submission/reconcile",
    "session/steer",
    "session/cancel",
    "permission/respond",
    "agent/configure",
    "agent/models",
    "agent/configuration",
    "agent/commands",
    "session/tasks",
    "session/task/stop",
    "agent/quotas",
    "session/rollback",
    "session/compact",
    "session/close",
    "agent/kill",
];
fn event_limit(p: &Value) -> Result<usize> {
    Ok(p.get("limit")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n > 0 && *n <= 4096)
                .ok_or_else(|| Error::Invalid("limit must be between 1 and 4096".into()))
        })
        .transpose()?
        .unwrap_or(4096) as usize)
}
fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .ok_or_else(|| Error::Invalid(format!("missing string parameter {key}")))
}
fn prompt(p: &Value) -> Result<Prompt> {
    if let Some(text) = p["text"].as_str() {
        Ok(text.into())
    } else if let Some(content) = p.get("content") {
        Ok(Prompt {
            content: serde_json::from_value(content.clone())
                .map_err(|e| Error::Invalid(e.to_string()))?,
        })
    } else {
        Err(Error::Invalid("provide text or content".into()))
    }
}
pub fn notification(event: RuntimeEvent) -> Value {
    json!({"jsonrpc":"2.0","method":"session/event","params":{"sessionId":event.session_id,"event":event}})
}
pub fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
