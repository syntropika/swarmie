use super::*;
use async_trait::async_trait;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

pub struct AcpHarness {
    id: String,
    executable: String,
    args: Vec<String>,
}
impl AcpHarness {
    pub fn new(id: impl Into<String>, executable: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            id: id.into(),
            executable: executable.into(),
            args,
        }
    }
}
#[async_trait]
impl Harness for AcpHarness {
    fn id(&self) -> &str {
        &self.id
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(detection(
            &self.id,
            &self.executable,
            Capabilities {
                models: Capability::unknown("Model choices are discovered when a session opens"),
                configure: Capability::unknown("Settings are discovered when a session opens"),
                modes: Capability::unknown("Modes are discovered when a session opens"),
                resume: Capability::unknown("Resume support requires native initialization"),
                images: Capability::unknown("Image support requires native initialization"),
                cancel: true.into(),
                permissions: true.into(),
                operation_approvals: Capability::unknown(
                    "ACP permission requests are optional; operation coverage is provider-specific",
                ),
                ..Default::default()
            },
        ))
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        readiness::probe(self.detect().await?, None).await
    }
    async fn connect(
        &self,
        o: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        if fork {
            return Err(Error::Unsupported("ACP fork".into()));
        }
        if o.execution.tools == ToolPolicy::Disabled {
            return Err(Error::Unsupported(
                "ACP strict tool disabling is not negotiated".into(),
            ));
        }
        if o.execution.ephemeral {
            return Err(Error::Unsupported("ACP ephemeral sessions".into()));
        }
        if o.execution.output_schema.is_some() {
            return Err(Error::Unsupported("ACP structured output schema".into()));
        }
        let environment = execution_env(o, None)?;
        let (exe, args) = launch(o, &self.executable, self.args.clone());
        let (wire, mut frames) =
            spawn_json(o, &exe, &args, WireStyle::JsonRpc, environment).await?;
        let init = wire.request("initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"swarmie","version":"0.1.0"}})).await?;
        if init["protocolVersion"] != 1 {
            return Err(Error::Provider("unsupported ACP protocol version".into()));
        }
        if let Some(method) = o.config["authMethod"].as_str() {
            wire.request("authenticate", json!({"methodId":method}))
                .await?;
        }
        let caps = &init["agentCapabilities"];
        let mut capabilities = Capabilities {
            commands: true.into(),
            resume: Capability::negotiated(caps["loadSession"] == true),
            cancel: true.into(),
            permissions: true.into(),
            operation_approvals: Capability::unknown(
                "ACP permission requests are optional; operation coverage is provider-specific",
            ),
            reasoning: true.into(),
            usage: true.into(),
            images: Capability::negotiated(caps["promptCapabilities"]["image"] == true),
            ..Default::default()
        };
        let mcp_servers = mcp_servers(o, caps)?;
        let mut transports = vec!["stdio"];
        if caps["mcpCapabilities"]["http"] == true {
            transports.push("http");
        }
        if caps["mcpCapabilities"]["sse"] == true {
            transports.push("sse");
        }
        capabilities.metadata = BTreeMap::from([("mcpTransports".into(), json!(transports))]);
        let mut params = json!({"cwd":o.cwd,"mcpServers":mcp_servers});
        let method = if let Some(s) = saved {
            if !capabilities.resume.is_available() {
                return Err(Error::Unsupported("ACP resume".into()));
            }
            params["sessionId"] = json!(s.id);
            "session/load"
        } else {
            "session/new"
        };
        let mut result = wire.request(method, params).await?;
        capabilities.models = Capability::negotiated(
            result["models"]["availableModels"].is_array() || model_option(&result).is_some(),
        );
        capabilities.configure = Capability::negotiated(
            capabilities.models.is_available()
                || result["modes"]["availableModes"].is_array()
                || result["configOptions"].is_array(),
        );
        capabilities.modes = Capability::negotiated(result["modes"]["availableModes"].is_array());
        let session = saved
            .map(|s| s.id.clone())
            .or_else(|| result["sessionId"].as_str().map(String::from))
            .ok_or_else(|| Error::Provider("missing ACP session id".into()))?;
        if let Some(model) = &o.model {
            if !capabilities.models.is_available() {
                return Err(Error::Unsupported("ACP initial model selection".into()));
            }
            if let Some(option) = model_option(&result) {
                let config_id = option["id"]
                    .as_str()
                    .ok_or_else(|| Error::Provider("ACP model option omitted id".into()))?;
                if !config_choices(option)
                    .iter()
                    .any(|choice| choice["id"] == *model)
                {
                    return Err(Error::Invalid("unsupported ACP initial model".into()));
                }
                let selected = wire
                    .request(
                        "session/set_config_option",
                        json!({"sessionId":session,"configId":config_id,"value":model}),
                    )
                    .await?;
                if selected["configOptions"].is_array() {
                    result["configOptions"] = selected["configOptions"].clone();
                }
            } else {
                wire.request(
                    "session/set_model",
                    json!({"sessionId":session,"modelId":model}),
                )
                .await?;
                result["models"]["currentModelId"] = json!(model);
            }
        }
        let requests: Arc<RwLock<BTreeMap<String, Value>>> = Arc::default();
        let configuration = Arc::new(RwLock::new(result.clone()));
        let configuration_task = configuration.clone();
        let req_task = requests.clone();
        let wire_task = wire.clone();
        let session_task = session.clone();
        let active: Arc<RwLock<Option<Value>>> = Arc::default();
        let active_task = active.clone();
        let (tx, events) = mpsc::channel(512);
        let backend_tx = tx.clone();
        let cursor = self.id == "cursor"
            || init["authMethods"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|m| m["id"] == "cursor_login"));
        let grok = self.id == "grok";
        let kiro = self.id == "kiro"
            || init["agentInfo"]["name"]
                .as_str()
                .is_some_and(|n| n.to_lowercase().contains("kiro"));
        tokio::spawn(async move {
            let mut calls = BTreeMap::<String, Value>::new();
            while let Some(f) = frames.recv().await {
                if let Some(diagnostic) = f.get("_runtime_diagnostic") {
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::Diagnostic(diagnostic.clone()),
                            f.clone(),
                        ))
                        .await;
                    continue;
                }
                if terminal_frame(&f, &tx).await {
                    break;
                }
                if f.get("id").is_some() && (f.get("result").is_some() || f.get("error").is_some())
                {
                    let mut current = active_task.write().await;
                    if current.as_ref() == f.get("id") {
                        *current = None;
                        wire_task.set_active(false);
                        if f.get("error").is_some() {
                            let _ = tx
                                .send(BackendEvent::content(
                                    EventKind::Error {
                                        message: f["error"].to_string(),
                                    },
                                    f.clone(),
                                ))
                                .await;
                        }
                        let native_reason = f["result"]["stopReason"].as_str().map(String::from);
                        let outcome = if f.get("error").is_some() {
                            TurnOutcome::Failed
                        } else {
                            outcome::acp(native_reason.as_deref())
                        };
                        if tx
                            .send(BackendEvent::TurnFinished {
                                outcome,
                                native_reason,
                                raw: Some(f),
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                }
                let p = &f["params"];
                let m = f["method"].as_str().unwrap_or("");
                if p["sessionId"].as_str().is_some_and(|s| s != session_task) {
                    let _ = tx.send(BackendEvent::content(EventKind::Raw, f)).await;
                    continue;
                }
                if f.get("id").is_some() && f.get("method").is_some() {
                    if (cursor && matches!(m, "cursor/ask_question" | "cursor/create_plan"))
                        || (grok && matches!(m, "_x.ai/ask_user_question" | "_x.ai/exit_plan_mode"))
                    {
                        match extension_request(&f) {
                            Ok(request) => {
                                if request.kind == "plan" {
                                    let _ = tx
                                        .send(BackendEvent::content(
                                            EventKind::PlanUpdated((extension_plan(p)).into()),
                                            f.clone(),
                                        ))
                                        .await;
                                }
                                req_task.write().await.insert(request.id.clone(), f.clone());
                                if tx
                                    .send(BackendEvent::content(
                                        EventKind::PermissionRequested(canonical_permission(
                                            request,
                                        )),
                                        f,
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(error) => {
                                let _ = wire_task.reject(f["id"].clone(), &error.to_string()).await;
                            }
                        }
                    } else if m == "session/request_permission" {
                        let id = f["id"].to_string();
                        let options = p["options"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|v| PermissionOption {
                                        id: string(v, "optionId"),
                                        label: string(v, "name"),
                                        kind: match v["kind"].as_str() {
                                            Some("allow_once") => "approve_once",
                                            Some("allow_always") => "approve_session",
                                            Some("reject_once") => "deny_once",
                                            Some("reject_always") => "deny_session",
                                            _ => "unknown",
                                        }
                                        .into(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut request = PermissionRequest {
                            questions: Vec::new(),
                            id: id.clone(),
                            kind: p["toolCall"]["kind"].as_str().unwrap_or("tool").into(),
                            title: p["toolCall"]["title"]
                                .as_str()
                                .unwrap_or("Approve tool call")
                                .into(),
                            description: None,
                            command: p["toolCall"].get("rawInput").cloned(),
                            paths: vec![],
                            options,
                            metadata: p.clone(),
                        };
                        request.options.push(option("__cancel", "Cancel", "cancel"));
                        req_task.write().await.insert(id, f.clone());
                        if tx
                            .send(BackendEvent::content(
                                EventKind::PermissionRequested(canonical_permission(request)),
                                f,
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    } else {
                        let _ = wire_task
                            .reject(f["id"].clone(), "client capability is not advertised")
                            .await;
                    }
                    continue;
                }
                if cursor
                    && matches!(
                        m,
                        "cursor/update_todos" | "cursor/task" | "cursor/generate_image"
                    )
                {
                    let kind = match m {
                        "cursor/update_todos" => {
                            let mut state = configuration_task.write().await;
                            let mut todos = if p["merge"] == true {
                                state["cursorTodos"].as_array().cloned().unwrap_or_default()
                            } else {
                                vec![]
                            };
                            if let Some(updates) = p["todos"].as_array() {
                                for todo in updates {
                                    if let Some(old) = todos.iter_mut().find(|old| {
                                        !todo["id"].is_null() && old["id"] == todo["id"]
                                    }) {
                                        *old = todo.clone();
                                    } else {
                                        todos.push(todo.clone());
                                    }
                                }
                            }
                            state["cursorTodos"] = json!(todos);
                            EventKind::PlanUpdated((json!({"entries":todos,"native":p})).into())
                        }
                        "cursor/task" => EventKind::SubagentUpdated((json!({"id":p["agentId"].as_str().or(p["toolCallId"].as_str()),"name":p["description"],"status":"completed","metadata":p})).into()),
                        _ => EventKind::FileChanged(p.clone()),
                    };
                    if tx.send(BackendEvent::content(kind, f)).await.is_err() {
                        break;
                    }
                    continue;
                }
                if kiro && m == "_kiro.dev/commands/available" {
                    if let Some(commands) = p["commands"].as_array() {
                        let commands: Vec<_> = commands.iter().filter_map(|command| Some(json!({"name":command["name"].as_str()?,"description":command["description"],"input":command["meta"]["hint"],"metadata":command}))).collect();
                        configuration_task.write().await["availableCommands"] = json!(commands);
                        if tx
                            .send(BackendEvent::content(
                                EventKind::CommandsUpdated(json!(commands)),
                                f,
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    continue;
                }
                let u = &p["update"];
                if m == "session/update"
                    && matches!(
                        u["sessionUpdate"].as_str(),
                        Some("agent_message_chunk" | "agent_thought_chunk")
                    )
                    && u["_meta"]["usage"].is_object()
                {
                    let mut reported = usage(&u["_meta"]["usage"]);
                    reported.scope = "call".into();
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::UsageUpdated(reported),
                            f.clone(),
                        ))
                        .await;
                }
                let kind = if m == "session/update" {
                    match u["sessionUpdate"].as_str().unwrap_or("") {
                        "agent_message_chunk" | "agent_thought_chunk"
                            if u["content"]["text"].as_str().is_none_or(str::is_empty)
                                && u["_meta"]["usage"].is_object() =>
                        {
                            EventKind::Raw
                        }
                        "agent_message_chunk" => EventKind::TextDelta {
                            text: string(&u["content"], "text"),
                            item_id: u["messageId"].as_str().map(String::from),
                        },
                        "agent_thought_chunk" => EventKind::ReasoningDelta {
                            text: string(&u["content"], "text"),
                        },
                        "tool_call" => {
                            calls.insert(string(u, "toolCallId"), u.clone());
                            EventKind::ToolStarted(acp_tool(u, "running"))
                        }
                        "tool_call_update"
                            if u["status"] == "completed" || u["status"] == "failed" =>
                        {
                            EventKind::ToolCompleted(merge_tool(&mut calls, u, "completed"))
                        }
                        "tool_call_update" => {
                            EventKind::ToolUpdated(merge_tool(&mut calls, u, "running"))
                        }
                        "plan" => EventKind::PlanUpdated((u.clone()).into()),
                        "usage_update" => {
                            let used = u["used"].as_u64();
                            let window = u["size"].as_u64();
                            let _ = tx
                                .send(BackendEvent::content(
                                    EventKind::ContextUpdated(ContextUsage {
                                        used_tokens: used,
                                        context_window_tokens: window,
                                        remaining_tokens: used
                                            .zip(window)
                                            .map(|(u, w)| w.saturating_sub(u)),
                                        percent: used
                                            .zip(window)
                                            .filter(|(_, w)| *w > 0)
                                            .map(|(u, w)| u as f64 / w as f64 * 100.0),
                                    }),
                                    f.clone(),
                                ))
                                .await;
                            EventKind::UsageUpdated(usage(u))
                        }
                        "current_mode_update" => {
                            configuration_task.write().await["modes"]["currentModeId"] =
                                u["currentModeId"].clone();
                            EventKind::ConfigurationUpdated(u.clone())
                        }
                        "config_option_update" => {
                            configuration_task.write().await["configOptions"] =
                                u["configOptions"].clone();
                            EventKind::ConfigurationUpdated(u.clone())
                        }
                        "current_model_update" => {
                            configuration_task.write().await["models"]["currentModelId"] =
                                u["currentModelId"].clone();
                            EventKind::ConfigurationUpdated(u.clone())
                        }
                        "available_commands_update" => {
                            configuration_task.write().await["availableCommands"] =
                                u["availableCommands"].clone();
                            EventKind::CommandsUpdated(u["availableCommands"].clone())
                        }
                        "session_info_update" => EventKind::SessionUpdated(u.clone()),
                        _ => EventKind::Raw,
                    }
                } else {
                    EventKind::Raw
                };
                if tx.send(BackendEvent::content(kind, f)).await.is_err() {
                    break;
                }
            }
        });
        let images = capabilities.images.is_available();
        Ok(BackendConnection {
            native: native(&self.id, session.clone()),
            capabilities,
            events,
            backend: Box::new(AcpBackend {
                wire,
                session,
                requests,
                active,
                configuration,
                images,
                tx: backend_tx,
            }),
        })
    }
}
struct AcpBackend {
    wire: JsonProcess,
    session: String,
    requests: Arc<RwLock<BTreeMap<String, Value>>>,
    active: Arc<RwLock<Option<Value>>>,
    configuration: Arc<RwLock<Value>>,
    tx: mpsc::Sender<BackendEvent>,
    images: bool,
}
#[async_trait]
impl AgentBackend for AcpBackend {
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        let content = p
            .content
            .iter()
            .map(|c| match c {
                PromptContent::Text(t) => Ok(json!({"type":"text","text":t})),
                PromptContent::Image { data, mime_type } => {
                    if !self.images {
                        return Err(Error::Unsupported(
                            "ACP agent does not advertise image prompts".into(),
                        ));
                    }
                    Ok(json!({"type":"image","data":data,"mimeType":mime_type}))
                }
                PromptContent::Resource { uri } => {
                    Ok(json!({"type":"resource_link","uri":uri,"name":uri}))
                }
                PromptContent::Skill { .. } => {
                    Err(Error::Unsupported("ACP native skill input".into()))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let id = json!(uuid::Uuid::new_v4().to_string());
        *self.active.write().await = Some(id.clone());
        self.wire.set_active(true);
        if let Err(e) = self.wire.send(json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{"sessionId":self.session,"prompt":content}})).await {
            *self.active.write().await = None; self.wire.set_active(false); return Err(e);
        }
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        self.wire
            .notify("session/cancel", json!({"sessionId":self.session}))
            .await
    }
    async fn respond_permission(&self, r: &PermissionResponse) -> Result<()> {
        let mut requests = self.requests.write().await;
        let f = requests
            .get(&r.id)
            .ok_or_else(|| Error::Invalid("unknown ACP request".into()))?;
        let answer = match f["method"].as_str().unwrap_or_default() {
            "cursor/ask_question"
            | "cursor/create_plan"
            | "_x.ai/ask_user_question"
            | "_x.ai/exit_plan_mode" => extension_answer(f, r)?,
            "session/request_permission" => {
                let outcome = if r.option == "__cancel" {
                    json!({"outcome":"cancelled"})
                } else {
                    if !f["params"]["options"]
                        .as_array()
                        .is_some_and(|options| options.iter().any(|o| o["optionId"] == r.option))
                    {
                        return Err(Error::Invalid("invalid ACP permission option".into()));
                    }
                    json!({"outcome":"selected","optionId":r.option})
                };
                json!({"outcome":outcome})
            }
            _ => return Err(Error::Invalid("unknown ACP response dialect".into())),
        };
        self.wire.respond(f["id"].clone(), answer).await?;
        requests.remove(&r.id);
        Ok(())
    }
    async fn commands(&self) -> Result<Value> {
        Ok(self.configuration.read().await["availableCommands"]
            .as_array()
            .cloned()
            .map(Value::Array)
            .unwrap_or(json!([])))
    }
    async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        let state = self.configuration.read().await;
        if let Some(option) = model_option(&state) {
            return settings::model_descriptors(json!(config_choices(option)));
        }
        settings::model_descriptors(Value::Array(
            state["models"]["availableModels"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|m| json!({"id":m["modelId"],"label":m["name"]}))
                        .collect()
                })
                .unwrap_or_default(),
        ))
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let state = self.configuration.read().await.clone();
        let mut fields = vec![];
        if state["models"]["availableModels"].is_array() {
            fields.push(settings::select(
                "model",
                "Model",
                state["models"]["currentModelId"].clone(),
                json!(self.models().await?),
            ));
        }
        if let Some(modes) = state["modes"]["availableModes"].as_array() {
            fields.push(settings::select(
                "mode",
                "Mode",
                state["modes"]["currentModeId"].clone(),
                json!(
                    modes
                        .iter()
                        .map(|m| json!({"id":m["id"],"label":m["name"]}))
                        .collect::<Vec<_>>()
                ),
            ));
        }
        if let Some(options) = state["configOptions"].as_array() {
            for option in options {
                if option["type"] == "select" {
                    let choices = config_choices(option);
                    fields.push(settings::select(
                        option["id"].as_str().unwrap_or_default(),
                        option["name"].as_str().unwrap_or("Option"),
                        option["currentValue"].clone(),
                        json!(choices),
                    ));
                }
            }
        }
        settings::descriptors(json!(fields))
    }
    async fn configure(&self, config: Value) -> Result<Value> {
        let fields = config
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        let descriptors = self.configuration().await?;
        for (id, value) in fields {
            let valid = descriptors
                .iter()
                .find(|descriptor| descriptor.id == *id)
                .is_some_and(|descriptor| descriptor.validate(value).is_ok());
            if !valid {
                return Err(Error::Invalid(format!("unsupported choice for {id}")));
            }
        }
        for (id, value) in fields {
            let native_option = self.configuration.read().await["configOptions"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["id"] == *id));
            match id.as_str() {
                "model" if !native_option => {
                    self.wire
                        .request(
                            "session/set_model",
                            json!({"sessionId":self.session,"modelId":value}),
                        )
                        .await?;
                    self.configuration.write().await["models"]["currentModelId"] = value.clone();
                }
                "mode" if !native_option => {
                    self.wire
                        .request(
                            "session/set_mode",
                            json!({"sessionId":self.session,"modeId":value}),
                        )
                        .await?;
                    self.configuration.write().await["modes"]["currentModeId"] = value.clone();
                }
                _ => {
                    let response = self
                        .wire
                        .request(
                            "session/set_config_option",
                            json!({"sessionId":self.session,"configId":id,"value":value}),
                        )
                        .await?;
                    if response["configOptions"].is_array() {
                        self.configuration.write().await["configOptions"] =
                            response["configOptions"].clone();
                    }
                }
            }
        }
        let descriptors = self.configuration().await?;
        self.tx
            .send(BackendEvent::Content {
                kind: Box::new(EventKind::ConfigurationUpdated(json!(descriptors))),
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)?;
        Ok(json!(descriptors))
    }
    async fn close(&self) -> Result<()> {
        self.wire.close().await
    }
    async fn kill(&self) -> Result<()> {
        self.wire.kill().await
    }
}

fn mcp_servers(options: &SpawnOptions, capabilities: &Value) -> Result<Vec<Value>> {
    if let Some(legacy) = options.config.get("mcpServers") {
        if !options.execution.mcp_servers.is_empty() {
            return Err(Error::Invalid(
                "use either execution.mcpServers or config.mcpServers".into(),
            ));
        }
        let servers = legacy
            .as_array()
            .ok_or_else(|| Error::Invalid("ACP mcpServers must be an array".into()))?;
        for server in servers {
            validate_transport(server["type"].as_str().unwrap_or("stdio"), capabilities)?;
        }
        return Ok(servers.clone());
    }
    options.execution.mcp_servers.iter().map(|server| {
        match &server.transport {
            McpTransport::Stdio {command,args,env} => Ok(json!({"name":server.name,"command":command,"args":args,"env":env.iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>()})),
            McpTransport::Http {url,headers} => {validate_transport("http",capabilities)?; Ok(json!({"type":"http","name":server.name,"url":url,"headers":headers.iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>()}))},
            McpTransport::Sse {url,headers} => {validate_transport("sse",capabilities)?; Ok(json!({"type":"sse","name":server.name,"url":url,"headers":headers.iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>()}))},
        }
    }).collect()
}
fn validate_transport(transport: &str, capabilities: &Value) -> Result<()> {
    match transport {
        "stdio" => Ok(()),
        "http" | "sse" if capabilities["mcpCapabilities"][transport] == true => Ok(()),
        "http" | "sse" => Err(Error::Unsupported(format!("ACP MCP {transport} transport"))),
        _ => Err(Error::Invalid(format!(
            "unknown ACP MCP transport {transport}"
        ))),
    }
}
fn acp_tool(value: &Value, status: &str) -> ToolEvent {
    let mut tool = tool(value, status);
    if (tool.output.is_null() || tool.output.as_str().is_some_and(str::is_empty))
        && let Some(blocks) = value["content"]
            .as_array()
            .filter(|blocks| !blocks.is_empty())
    {
        let text: Option<Vec<_>> = blocks
            .iter()
            .map(|block| {
                if block["type"] == "content" && block["content"]["type"] == "text" {
                    block["content"]["text"].as_str()
                } else {
                    None
                }
            })
            .collect();
        tool.output = text
            .map(|text| json!(text.join("\n")))
            .unwrap_or_else(|| value["content"].clone());
    }
    tool.metadata = json!({"kind":value["kind"],"locations":value["locations"],"native":value["_meta"],"content":value["content"]});
    tool.details = Some(tool_details(value, &tool));
    tool
}
fn merge_tool(calls: &mut BTreeMap<String, Value>, update: &Value, status: &str) -> ToolEvent {
    let entry = calls
        .entry(string(update, "toolCallId"))
        .or_insert(json!({}));
    if let (Some(previous), Some(fields)) = (entry.as_object_mut(), update.as_object()) {
        for (key, value) in fields {
            previous.insert(key.clone(), value.clone());
        }
    }
    acp_tool(entry, status)
}

fn model_option(state: &Value) -> Option<&Value> {
    state["configOptions"].as_array()?.iter().find(|option| {
        option["type"] == "select" && (option["category"] == "model" || option["id"] == "model")
    })
}
fn config_choices(option: &Value) -> Vec<Value> {
    fn collect(options: &Value, choices: &mut Vec<Value>) {
        if let Some(options) = options.as_array() {
            for choice in options {
                if let Some(value) = choice["value"].as_str() {
                    choices
                        .push(json!({"id":value,"label":choice["name"].as_str().unwrap_or(value)}));
                } else {
                    collect(&choice["options"], choices);
                }
            }
        }
    }
    let mut choices = vec![];
    collect(&option["options"], &mut choices);
    choices
}
fn extension_plan(params: &Value) -> Value {
    let p = params.get("params").unwrap_or(params);
    json!({"entries":p["todos"],"markdown":p["plan"].as_str().or(p["planContent"].as_str()),"title":p["name"],"native":p})
}
fn extension_request(frame: &Value) -> Result<PermissionRequest> {
    let method = frame["method"].as_str().unwrap_or_default();
    let p = frame["params"].get("params").unwrap_or(&frame["params"]);
    let question = method.ends_with("ask_question") || method.ends_with("ask_user_question");
    let cursor = method.starts_with("cursor/");
    let (kind, options, metadata) = if question {
        let questions = p["questions"]
            .as_array()
            .filter(|q| !q.is_empty())
            .ok_or_else(|| Error::Invalid("agent question omitted questions".into()))?;
        let mut normalized = vec![];
        for (index, q) in questions.iter().enumerate() {
            let id = if cursor {
                q["id"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("Cursor question omitted id".into()))?
                    .to_owned()
            } else {
                index.to_string()
            };
            let prompt = q["prompt"]
                .as_str()
                .or(q["question"].as_str())
                .ok_or_else(|| Error::Invalid("agent question omitted prompt".into()))?;
            let mut choices = vec![];
            for choice in q["options"]
                .as_array()
                .ok_or_else(|| Error::Invalid("agent question omitted options".into()))?
            {
                let label = choice["label"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("agent choice omitted label".into()))?;
                let value = if cursor {
                    choice["id"]
                        .as_str()
                        .ok_or_else(|| Error::Invalid("Cursor choice omitted id".into()))?
                } else {
                    label
                };
                choices.push(json!({"id":value,"label":label,"description":choice["description"]}));
            }
            normalized.push(json!({"id":id,"prompt":prompt,"options":choices,"multiple":q["allowMultiple"]==true || q["multiSelect"]==true,"allowText":!cursor,"secret":false}));
        }
        (
            "question",
            vec![
                option("answer", "Submit answers", "answer"),
                option("skip", "Skip", "deny"),
                option("__cancel", "Cancel", "cancel"),
            ],
            json!({"questions":normalized,"native":p}),
        )
    } else {
        (
            "plan",
            vec![
                option("approve", "Approve plan", "approve"),
                option("deny", "Reject plan", "deny"),
                option("__cancel", "Cancel", "cancel"),
            ],
            json!({"plan":extension_plan(p),"native":p}),
        )
    };
    Ok(PermissionRequest {
        questions: Vec::new(),
        id: frame["id"].to_string(),
        kind: kind.into(),
        title: p["title"]
            .as_str()
            .or(p["name"].as_str())
            .unwrap_or(if question {
                "Agent question"
            } else {
                "Review plan"
            })
            .into(),
        description: p["plan"]
            .as_str()
            .or(p["planContent"].as_str())
            .map(String::from),
        command: None,
        paths: vec![],
        options,
        metadata,
    })
}
fn extension_answer(frame: &Value, response: &PermissionResponse) -> Result<Value> {
    let method = frame["method"].as_str().unwrap_or_default();
    let p = frame["params"].get("params").unwrap_or(&frame["params"]);
    if matches!(method, "cursor/create_plan" | "_x.ai/exit_plan_mode") {
        let outcome = match (method, response.option.as_str()) {
            ("cursor/create_plan", "approve") => "accepted",
            ("cursor/create_plan", "deny") => "rejected",
            ("cursor/create_plan", "__cancel") => "cancelled",
            (_, "approve") => "approved",
            (_, "deny") => "cancelled",
            (_, "__cancel") => "abandoned",
            _ => return Err(Error::Invalid("invalid plan decision".into())),
        };
        return Ok(if method.starts_with("cursor/") {
            json!({"outcome":{"outcome":outcome,"reason":response.data["reason"]}})
        } else {
            json!({"outcome":outcome,"feedback":response.data["feedback"]})
        });
    }
    let cursor = method.starts_with("cursor/");
    match response.option.as_str() {
        "__cancel" => {
            return Ok(if cursor {
                json!({"outcome":{"outcome":"cancelled"}})
            } else {
                json!({"outcome":"cancelled"})
            });
        }
        "skip" => {
            return Ok(if cursor {
                json!({"outcome":{"outcome":"skipped"}})
            } else {
                json!({"outcome":"cancelled"})
            });
        }
        "answer" => (),
        _ => return Err(Error::Invalid("invalid question decision".into())),
    }
    let generic = extension_request(frame)?;
    let questions = generic.metadata["questions"].as_array().unwrap();
    let answers = response.data["answers"]
        .as_object()
        .ok_or_else(|| Error::Invalid("answers must be an object".into()))?;
    if answers
        .keys()
        .any(|id| !questions.iter().any(|q| q["id"] == *id))
    {
        return Err(Error::Invalid("answer names an unknown question".into()));
    }
    let mut selected = vec![];
    let mut grok_answers = serde_json::Map::new();
    let mut annotations = serde_json::Map::new();
    for (index, q) in questions.iter().enumerate() {
        let id = q["id"].as_str().unwrap();
        let values = match answers.get(id) {
            Some(Value::String(s)) => vec![s.as_str()],
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_str().ok_or_else(|| {
                        Error::Invalid("question answer must contain strings".into())
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(Error::Invalid(format!("missing answer for {id}"))),
        };
        if values.is_empty() || (q["multiple"] != true && values.len() != 1) {
            return Err(Error::Invalid(format!(
                "invalid number of answers for {id}"
            )));
        }
        if cursor {
            if values.iter().any(|v| {
                !q["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o["id"] == *v)
            }) {
                return Err(Error::Invalid(format!("invalid Cursor choice for {id}")));
            }
            selected.push(json!({"questionId":id,"selectedOptionIds":values}));
        } else {
            let native = &p["questions"][index];
            let prompt = native["question"]
                .as_str()
                .unwrap_or(q["prompt"].as_str().unwrap());
            let (known, notes): (Vec<_>, Vec<_>) = values.into_iter().partition(|value| {
                q["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o["id"] == *value)
            });
            let mut known: Vec<String> = known.into_iter().map(String::from).collect();
            if !notes.is_empty() {
                known.push("Other".into());
                annotations.insert(prompt.into(), json!({"notes":notes.join("\n")}));
            }
            if q["multiple"] != true
                && let Some(preview) = native["options"]
                    .as_array()
                    .and_then(|options| options.iter().find(|o| o["label"] == known[0]))
                    .and_then(|o| o["preview"].as_str())
            {
                annotations.entry(prompt.to_owned()).or_insert(json!({}))["preview"] =
                    json!(preview);
            }
            grok_answers.insert(prompt.into(), json!(known));
        }
    }
    Ok(if cursor {
        json!({"outcome":{"outcome":"answered","answers":selected}})
    } else {
        json!({"outcome":"accepted","answers":grok_answers,"annotations":annotations})
    })
}

#[cfg(test)]
mod semantic_acp_tools {
    use super::*;
    #[test]
    fn sparse_acp_tool_retains_typed_diff_and_file_location_after_progress() {
        let mut calls = BTreeMap::new();
        let start = json!({"toolCallId":"edit","title":"Replace line","kind":"edit","locations":[{"path":"src/app.rs","line":7,"column":2,"vendorRange":"native"}],"content":[{"type":"diff","path":"src/app.rs","oldText":"old","newText":"new"}],"_meta":{"vendor":"native"}});
        merge_tool(&mut calls, &start, "running");
        let event = merge_tool(
            &mut calls,
            &json!({"toolCallId":"edit","status":"completed"}),
            "completed",
        );
        let details = event.details.unwrap();
        assert_eq!(details.kind.as_deref(), Some("edit"));
        assert_eq!(details.locations.len(), 1);
        assert_eq!(details.diffs.len(), 1);
        assert_eq!(details.locations[0].line, Some(7));
        assert_eq!(details.locations[0].extra["vendorRange"], "native");
        assert_eq!(details.diffs[0].old_text.as_deref(), Some("old"));
        assert_eq!(details.diffs[0].new_text.as_deref(), Some("new"));
        assert_eq!(event.metadata["native"]["vendor"], "native");
        assert_eq!(event.status, "completed");
    }
}
