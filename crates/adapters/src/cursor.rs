use super::*;
#[path = "cursor_bridge.rs"]
mod bridge;
#[path = "cursor_history.rs"]
mod history;
use async_trait::async_trait;
use bridge::Bridge;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::RwLock;

pub struct CursorHarness;
pub(crate) fn bridge_executable() -> Option<PathBuf> {
    bridge::executable()
}
pub(crate) async fn verify_account(options: &SpawnOptions, key: &str) -> Result<()> {
    let bridge = Bridge::spawn(options).await?;
    let result = bridge
        .call("SdkCursorService", "Me", json!({"options":{"apiKey":key}}))
        .await;
    let _ = bridge.shutdown().await;
    result.map(|_| ())
}
pub(crate) async fn probe_options(options: &SpawnOptions) -> Result<HarnessProbe> {
    let mut detection = CursorHarness.detect().await?;
    if let Some(executable) = options.config["executable"].as_str() {
        detection.executable = process::find_executable(executable);
        detection.available = detection.executable.is_some();
        detection.capabilities.tools_disabled = detection
            .executable
            .as_deref()
            .is_some_and(bridge::pinned_contract)
            .into();
    }
    let mut result = HarnessProbe {
        harness: "cursor".into(),
        available: detection.available,
        executable: detection.executable.clone(),
        version: None,
        auth_state: "unknown".into(),
        message: None,
        capabilities: detection.capabilities,
    };
    if detection.executable.is_none() {
        result.message = Some("Install Cursor SDK Bridge or set SWARMIE_CURSOR_EXECUTABLE; cursor-agent alone is insufficient.".into());
        return Ok(result);
    }
    let bridge = match Bridge::spawn(options).await {
        Ok(bridge) => bridge,
        Err(error) => {
            result.available = false;
            result.message = Some(error.to_string());
            return Ok(result);
        }
    };
    let version = bridge
        .call("SdkBridgeControlService", "GetVersion", json!({}))
        .await?;
    result.version = version["bridgeVersion"].as_str().map(String::from);
    if let Some(key) = bridge::key(&options.execution.env) {
        match bridge
            .call("SdkCursorService", "Me", json!({"options":{"apiKey":key}}))
            .await
        {
            Ok(_) => result.auth_state = "authenticated".into(),
            Err(error) => {
                result.message = Some(format!("Cursor account verification failed: {error}"))
            }
        }
    } else {
        result.auth_state = "unauthenticated".into();
        result.message = Some("Provide CURSOR_API_KEY to the runtime's secret environment; Cursor CLI login does not authenticate the SDK Bridge.".into());
    }
    let _ = bridge.shutdown().await;
    Ok(result)
}
fn capabilities() -> Capabilities {
    Capabilities {
        resume: true.into(),
        cancel: true.into(),
        reasoning: true.into(),
        usage: true.into(),
        models: true.into(),
        configure: true.into(),
        modes: true.into(),
        images: true.into(),
        tools_disabled: false.into(),
        metadata: BTreeMap::from([
            ("transport".into(), json!("cursor_sdk_bridge")),
            ("sdkVersion".into(), json!(bridge::SDK_VERSION)),
            ("mcpTransports".into(), json!(["stdio", "http", "sse"])),
            ("nativeHistory".into(), json!(true)),
            (
                "nativeHistorySource".into(),
                json!("local_sdk_checkpoint_store"),
            ),
        ]),
        ..Default::default()
    }
}

#[async_trait]
impl Harness for CursorHarness {
    fn id(&self) -> &str {
        "cursor"
    }
    async fn detect(&self) -> Result<Detection> {
        let executable = bridge::executable();
        let mut capabilities = capabilities();
        capabilities.tools_disabled = executable
            .as_deref()
            .is_some_and(bridge::pinned_contract)
            .into();
        Ok(Detection {
            harness: self.id().into(),
            available: executable.is_some(),
            executable,
            capabilities,
        })
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        probe_options(&SpawnOptions::new("cursor", std::env::current_dir()?)).await
    }
    async fn native_history(
        &self,
        query: &NativeHistoryQuery,
    ) -> Result<Vec<NativeHistorySession>> {
        history::discover(query, &AgentToolExecutionOverlay::default()).await
    }
    async fn native_history_with(
        &self,
        query: &NativeHistoryQuery,
        overlay: &AgentToolExecutionOverlay,
    ) -> Result<Vec<NativeHistorySession>> {
        history::discover(query, overlay).await
    }
    async fn read_native_history(
        &self,
        query: &NativeHistoryQuery,
        native_id: &str,
    ) -> Result<NativeHistoryDocument> {
        history::read(query, native_id, &AgentToolExecutionOverlay::default()).await
    }
    async fn read_native_history_with(
        &self,
        query: &NativeHistoryQuery,
        native_id: &str,
        overlay: &AgentToolExecutionOverlay,
    ) -> Result<NativeHistoryDocument> {
        history::read(query, native_id, overlay).await
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        validate(options, fork)?;
        if saved.is_some_and(|native| native.provider != "cursor" || native.id.is_empty()) {
            return Err(Error::Invalid(
                "invalid Cursor native agent reference".into(),
            ));
        }
        let key = bridge::key(&options.execution.env).ok_or_else(|| {
            Error::Invalid(
                "Cursor SDK requires CURSOR_API_KEY in the secret execution environment".into(),
            )
        })?;
        let bridge = Bridge::spawn(options).await?;
        if options.execution.tools == ToolPolicy::Disabled && !bridge.pinned_contract {
            return Err(Error::Unsupported(
                "Cursor strict tool disabling requires the pinned SDK Bridge distribution manifest"
                    .into(),
            ));
        }
        let models = catalog(&bridge, &key).await?;
        let model = options
            .model
            .as_deref()
            .or_else(|| options.config["model"].as_str())
            .map(String::from)
            .or_else(|| models.first().map(|model| model.id.clone()))
            .ok_or_else(|| {
                Error::Provider("Cursor account has no available local models".into())
            })?;
        if !models.iter().any(|entry| entry.id == model) {
            return Err(Error::Invalid(
                "model is not in the Cursor SDK catalog".into(),
            ));
        }
        let mode = options.config["mode"].as_str().unwrap_or("agent");
        let cwd = std::fs::canonicalize(&options.cwd)?
            .to_string_lossy()
            .into_owned();
        let mcp = mcp_servers(options)?;
        let mut local = json!({"cwd":[cwd], "settingSources":["SETTING_SOURCE_ALL"], "sandboxOptions":{"enabled":true}, "autoReview":true});
        let mut agent_options = json!({"apiKey":key,"model":{"id":model}, "mode":mode_wire(mode),"local":local,"mcpServers":mcp});
        if options.execution.tools == ToolPolicy::Disabled {
            local["settingSources"] = json!([]);
            agent_options["local"] = local;
            agent_options["tools"] = json!({"names":[]});
        }
        let body = if let Some(saved) = saved {
            json!({"agentId":saved.id,"options":agent_options})
        } else {
            json!({"options":agent_options})
        };
        let response = bridge
            .call(
                "SdkAgentService",
                if saved.is_some() {
                    "ResumeAgent"
                } else {
                    "CreateAgent"
                },
                body,
            )
            .await?;
        let id = response["agentId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| Error::Provider("Cursor SDK did not return an agent ID".into()))?
            .to_owned();
        if saved.is_some_and(|saved| saved.id != id) {
            return Err(Error::Provider(
                "Cursor resume returned a different agent ID".into(),
            ));
        }
        let (events, receiver) = mpsc::channel(256);
        let closed = Arc::new(AtomicBool::new(false));
        let monitor_bridge = bridge.clone();
        let monitor_closed = closed.clone();
        let monitor_events = events.clone();
        let active = Arc::new(Mutex::new(Active::default()));
        let monitor_active = active.clone();
        let monitor = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if monitor_closed.load(Ordering::SeqCst) {
                    return;
                }
                if monitor_bridge.exited() {
                    monitor_closed.store(true, Ordering::SeqCst);
                    if !monitor_active.lock().unwrap().running {
                        let _ = monitor_events
                            .send(BackendEvent::Exited {
                                message: "Cursor SDK Bridge exited".into(),
                            })
                            .await;
                    }
                    return;
                }
            }
        });
        let mut capabilities = capabilities();
        capabilities.tools_disabled = bridge.pinned_contract.into();
        let backend = CursorBackend {
            bridge,
            id: id.clone(),
            key,
            settings: RwLock::new(json!({"model":model,"mode":mode})),
            models,
            events,
            active,
            task: Mutex::new(None),
            closed,
            monitor,
        };
        Ok(BackendConnection {
            backend: Box::new(backend),
            native: native("cursor", id),
            capabilities,
            events: receiver,
        })
    }
}

fn validate(options: &SpawnOptions, fork: bool) -> Result<()> {
    if fork {
        return Err(Error::Unsupported("Cursor SDK local agent fork".into()));
    }
    if options.execution.ephemeral {
        return Err(Error::Unsupported("Cursor SDK ephemeral agents".into()));
    }
    if options.execution.output_schema.is_some() {
        return Err(Error::Unsupported(
            "Cursor SDK structured output schema".into(),
        ));
    }
    if options.execution.tools == ToolPolicy::Disabled && !options.execution.mcp_servers.is_empty()
    {
        return Err(Error::Unsupported(
            "Cursor SDK strict tool disabling with MCP servers".into(),
        ));
    }
    if let Some(fields) = options.config.as_object() {
        for field in fields.keys() {
            if !["executable", "args", "model", "mode"].contains(&field.as_str()) {
                return Err(Error::Unsupported(format!(
                    "Cursor SDK launch configuration {field}; manual approval policies are unavailable"
                )));
            }
        }
    } else if !options.config.is_null() {
        return Err(Error::Invalid(
            "Cursor configuration must be an object".into(),
        ));
    }
    if let Some(mode) = options.config.get("mode")
        && !matches!(mode.as_str(), Some("agent" | "plan"))
    {
        return Err(Error::Invalid("Cursor mode must be agent or plan".into()));
    }
    if let Some(model) = options.config.get("model")
        && model.as_str().is_none_or(|model| model.is_empty())
    {
        return Err(Error::Invalid(
            "Cursor model must be a nonempty string".into(),
        ));
    }
    Ok(())
}
fn mode_wire(mode: &str) -> &'static str {
    if mode == "plan" {
        "AGENT_MODE_OPTION_PLAN"
    } else {
        "AGENT_MODE_OPTION_AGENT"
    }
}
fn mcp_servers(options: &SpawnOptions) -> Result<Value> {
    let mut servers = serde_json::Map::new();
    for server in &options.execution.mcp_servers {
        let value = match &server.transport {
            McpTransport::Stdio { command, args, env } => {
                json!({"stdio":{"command":command,"args":args,"env":env,"cwd":options.cwd}})
            }
            McpTransport::Http { url, headers } => {
                json!({"http":{"type":"HTTP_MCP_TRANSPORT_TYPE_HTTP","url":url,"headers":headers}})
            }
            McpTransport::Sse { url, headers } => {
                json!({"http":{"type":"HTTP_MCP_TRANSPORT_TYPE_SSE","url":url,"headers":headers}})
            }
        };
        if servers.insert(server.name.clone(), value).is_some() {
            return Err(Error::Invalid("duplicate Cursor MCP server name".into()));
        }
    }
    Ok(Value::Object(servers))
}
async fn catalog(bridge: &Bridge, key: &str) -> Result<Vec<ModelDescriptor>> {
    let response = bridge
        .call(
            "SdkCursorService",
            "ListModels",
            json!({"options":{"apiKey":key}}),
        )
        .await?;
    let items = response["items"]
        .as_array()
        .ok_or_else(|| Error::Provider("Cursor SDK returned invalid model catalog".into()))?;
    settings::model_descriptors(json!(items.iter().filter_map(|item| {
        let id = item["id"].as_str()?;
        Some(json!({"id":id,"label":item["displayName"].as_str().unwrap_or(id),"description":item["description"],"metadata":item}))
    }).collect::<Vec<_>>()))
}

#[derive(Default)]
struct Active {
    running: bool,
    run_id: Option<String>,
    cancel_requested: bool,
}
struct CursorBackend {
    bridge: Arc<Bridge>,
    id: String,
    key: String,
    settings: RwLock<Value>,
    models: Vec<ModelDescriptor>,
    events: mpsc::Sender<BackendEvent>,
    active: Arc<Mutex<Active>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    closed: Arc<AtomicBool>,
    monitor: tokio::task::JoinHandle<()>,
}
impl Drop for CursorBackend {
    fn drop(&mut self) {
        self.monitor.abort();
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }
}

#[async_trait]
impl AgentBackend for CursorBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        let mut text = Vec::new();
        let mut images = Vec::new();
        for part in &prompt.content {
            match part {
                PromptContent::Text(value) => text.push(value.clone()),
                PromptContent::Image { data, mime_type } => {
                    images.push(json!({"data":{"data":data,"mimeType":mime_type}}))
                }
                _ => {
                    return Err(Error::Rejected(
                        "Cursor SDK supports text and inline images only".into(),
                    ));
                }
            }
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::Rejected("Cursor agent is closed".into()));
        }
        {
            let mut active = self.active.lock().unwrap();
            if active.running {
                return Err(Error::Rejected(
                    "Cursor agent already has an active run".into(),
                ));
            }
            *active = Active {
                running: true,
                ..Default::default()
            };
        }
        let config = self.settings.read().await.clone();
        let frames = self.bridge.send(json!({"agentId":self.id,"message":{"text":text.join("\n"),"images":images},"options":{"model":{"id":config["model"]},"mode":mode_wire(config["mode"].as_str().unwrap_or("agent")),"enableDeltas":true,"enableSteps":false}})).await;
        let frames = match frames {
            Ok(frames) => frames,
            Err(error) => {
                if matches!(error, Error::Rejected(_)) {
                    self.active.lock().unwrap().running = false;
                } else {
                    // Missing response headers can follow a native acceptance.
                    // Stop the local bridge and require an explicit resume rather
                    // than admitting another prompt into the uncertain native run.
                    self.closed.store(true, Ordering::SeqCst);
                    let _ = self.bridge.kill().await;
                }
                return Err(error);
            }
        };
        let bridge = self.bridge.clone();
        let active = self.active.clone();
        let id = self.id.clone();
        let events = self.events.clone();
        let closed = self.closed.clone();
        let task = tokio::spawn(async move {
            if !run(frames, &bridge, &id, &active, &events).await {
                closed.store(true, Ordering::SeqCst);
                let _ = bridge.kill().await;
            }
        });
        *self.task.lock().unwrap() = Some(task);
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        let run_id = {
            let mut active = self.active.lock().unwrap();
            if !active.running {
                return Ok(());
            }
            active.cancel_requested = active.run_id.is_none();
            active.run_id.clone()
        };
        if let Some(run_id) = run_id {
            self.bridge
                .call(
                    "SdkAgentService",
                    "CancelRun",
                    json!({"runId":run_id,"agentId":self.id}),
                )
                .await?;
        }
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Err(Error::Unsupported(
            "Cursor SDK manual tool approval callbacks".into(),
        ))
    }
    async fn configure(&self, config: Value) -> Result<Value> {
        let descriptors: Vec<Value> = self
            .configuration()
            .await?
            .iter()
            .map(|entry| serde_json::to_value(entry).unwrap())
            .collect();
        settings::validate(&config, &json!(descriptors))?;
        let mut current = self.settings.write().await;
        for (key, value) in config.as_object().unwrap() {
            current[key] = value.clone();
        }
        Ok(current.clone())
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let config = self.settings.read().await;
        settings::descriptors(json!([
            settings::select(
                "model",
                "Model",
                config["model"].clone(),
                json!(
                    self.models
                        .iter()
                        .map(|model| json!({"id":model.id,"label":model.label}))
                        .collect::<Vec<_>>()
                )
            ),
            settings::select(
                "mode",
                "Mode",
                config["mode"].clone(),
                settings::choices(&["agent", "plan"])
            )
        ]))
    }
    async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        catalog(&self.bridge, &self.key).await
    }
    async fn close(&self) -> Result<()> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _ = self.cancel().await;
        let _ = self
            .bridge
            .call("SdkAgentService", "CloseAgent", json!({"agentId":self.id}))
            .await;
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
        self.bridge.shutdown().await
    }
    async fn kill(&self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
        self.bridge.kill().await
    }
}

struct Projection {
    streamed_text: bool,
    tools: HashSet<String>,
    completed_tools: HashSet<String>,
    failure: Option<String>,
}
async fn run(
    mut frames: bridge::Frames,
    bridge: &Bridge,
    id: &str,
    active: &Mutex<Active>,
    events: &mpsc::Sender<BackendEvent>,
) -> bool {
    let mut state = Projection {
        streamed_text: false,
        tools: HashSet::new(),
        completed_tools: HashSet::new(),
        failure: None,
    };
    let result: Result<()> = async {
        while let Some(frame) = frames.next().await? {
            let message = frame
                .get("sdkMessage")
                .map(|value| &value["message"])
                .unwrap_or(&Value::Null)
                .clone();
            let run_id = message["run_id"]
                .as_str()
                .or_else(|| message["runId"].as_str())
                .or_else(|| frame["result"]["runId"].as_str())
                .or_else(|| frame["done"]["runId"].as_str());
            if let Some(run_id) = run_id {
                let cancel = {
                    let mut active = active.lock().unwrap();
                    active.run_id = Some(run_id.into());
                    std::mem::take(&mut active.cancel_requested)
                };
                if cancel {
                    bridge
                        .call(
                            "SdkAgentService",
                            "CancelRun",
                            json!({"runId":run_id,"agentId":id}),
                        )
                        .await?;
                }
            }
            if let Some(result) = frame.get("result") {
                active.lock().unwrap().running = false;
                finish(result, &state, events, frame.clone()).await;
                // The native terminal result is authoritative. Release the run
                // guard before the runtime dispatches its next queued prompt.
                return Ok(());
            } else {
                project(&frame, &mut state, events).await;
            }
        }
        Err(Error::Provider(
            "Cursor run stream omitted its terminal result".into(),
        ))
    }
    .await;
    if let Err(error) = result {
        let run_id = active.lock().unwrap().run_id.clone();
        if let Some(run_id) = run_id {
            // A disconnected live stream does not cancel the native run. Wait for
            // its native result rather than resubmitting or trusting live offsets.
            match bridge
                .call_timeout(
                    "SdkAgentService",
                    "WaitLiveRun",
                    json!({"runId":run_id}),
                    false,
                )
                .await
            {
                Ok(result) if result.get("result").is_some() => {
                    active.lock().unwrap().running = false;
                    finish(
                        &json!({"status":result["result"]["status"],"result":result["result"]}),
                        &state,
                        events,
                        result.clone(),
                    )
                    .await;
                    return true;
                }
                _ => (),
            }
        }
        let _ = events
            .send(BackendEvent::Exited {
                message: format!("Cursor run outcome is unresolved: {error}"),
            })
            .await;
        return false;
    }
    true
}
async fn emit(events: &mpsc::Sender<BackendEvent>, kind: EventKind, raw: &Value) {
    let _ = events.send(BackendEvent::content(kind, raw.clone())).await;
}
async fn project(frame: &Value, state: &mut Projection, events: &mpsc::Sender<BackendEvent>) {
    if let Some(update) = frame.get("interactionUpdate") {
        let kind = update["type"].as_str().unwrap_or_default();
        let value = &update["update"];
        match kind {
            "text-delta" => {
                if let Some(text) = value["text"].as_str() {
                    state.streamed_text = true;
                    emit(
                        events,
                        EventKind::TextDelta {
                            text: text.into(),
                            item_id: value["id"].as_str().map(String::from),
                        },
                        frame,
                    )
                    .await;
                }
            }
            "thinking-delta" => {
                if let Some(text) = value["text"].as_str() {
                    emit(
                        events,
                        EventKind::ReasoningDelta { text: text.into() },
                        frame,
                    )
                    .await;
                }
            }
            "tool-call-started" | "partial-tool-call" | "tool-call-completed" => {
                tool_update(value, kind == "tool-call-completed", state, events, frame).await
            }
            "step-completed" | "turn-ended" => {
                emit(events, EventKind::MessageEnded { item_id: None }, frame).await
            }
            _ => (),
        }
    }
    if let Some(message) = frame.get("sdkMessage") {
        let value = &message["message"];
        match message["type"].as_str() {
            Some("assistant") => {
                emit(events, EventKind::AssistantMessage(value.clone()), frame).await;
                // sdk_message assistant payloads repeat the same interaction
                // text deltas. Only interaction_update drives visible streaming;
                // the terminal result supplies a fallback when deltas are absent.
            }
            Some("tool_call") => {
                tool_update(
                    value,
                    matches!(value["status"].as_str(), Some("completed" | "error")),
                    state,
                    events,
                    frame,
                )
                .await
            }
            Some("usage") => usage(value.get("usage").unwrap_or(value), events, frame).await,
            Some("status") => {
                if matches!(
                    value["status"].as_str(),
                    Some("error" | "ERROR" | "RUN_LIFECYCLE_STATUS_ERROR")
                ) {
                    state.failure = value["message"].as_str().map(String::from);
                }
            }
            _ => (),
        }
    }
}
async fn tool_update(
    value: &Value,
    completed: bool,
    state: &mut Projection,
    events: &mpsc::Sender<BackendEvent>,
    raw: &Value,
) {
    let call = value.get("toolCall").unwrap_or(value);
    let id = value["callId"]
        .as_str()
        .or_else(|| value["tool_call_id"].as_str())
        .or_else(|| value["call_id"].as_str())
        .or_else(|| value["id"].as_str())
        .unwrap_or_default();
    if id.is_empty() {
        return;
    }
    if state.completed_tools.contains(id) {
        return;
    }
    if completed {
        state.completed_tools.insert(id.into());
    }
    let event = ToolEvent {
        id: id.into(),
        name: call["type"]
            .as_str()
            .or_else(|| call["name"].as_str())
            .map(String::from),
        status: if completed {
            if call["result"]["status"] == "error" || value["status"] == "error" {
                "failed"
            } else {
                "completed"
            }
        } else {
            "running"
        }
        .into(),
        input: call
            .get("args")
            .or_else(|| call.get("input"))
            .cloned()
            .unwrap_or(Value::Null),
        output: call.get("result").cloned().unwrap_or(Value::Null),
        details: None,
        metadata: json!({"nativeCallId":id}),
    };
    let first = state.tools.insert(id.into());
    let kind = if completed {
        EventKind::ToolCompleted(event)
    } else if first {
        EventKind::ToolStarted(event)
    } else {
        EventKind::ToolUpdated(event)
    };
    emit(events, kind, raw).await;
}
async fn usage(value: &Value, events: &mpsc::Sender<BackendEvent>, raw: &Value) {
    let count = |key: &str| {
        value[key]
            .as_u64()
            .or_else(|| value[key].as_str().and_then(|s| s.parse().ok()))
    };
    emit(
        events,
        EventKind::UsageUpdated(Usage {
            scope: "turn".into(),
            input_tokens: count("inputTokens"),
            output_tokens: count("outputTokens"),
            cached_input_tokens: count("cacheReadTokens"),
            cache_write_tokens: count("cacheWriteTokens"),
            total_tokens: count("totalTokens"),
            cost_usd: None,
        }),
        raw,
    )
    .await;
}
async fn finish(
    result: &Value,
    state: &Projection,
    events: &mpsc::Sender<BackendEvent>,
    raw: Value,
) {
    let status = result["status"].as_str().unwrap_or_default();
    let outcome = match status {
        "RUN_LIFECYCLE_STATUS_FINISHED" | "finished" | "3" => TurnOutcome::Completed,
        "RUN_LIFECYCLE_STATUS_CANCELLED" | "cancelled" | "5" => TurnOutcome::Cancelled,
        "RUN_LIFECYCLE_STATUS_ERROR" | "error" | "4" => TurnOutcome::Failed,
        "RUN_LIFECYCLE_STATUS_EXPIRED" | "expired" | "6" => TurnOutcome::Limited,
        _ => TurnOutcome::Unknown,
    };
    if let Some(value) = result["result"].get("usage") {
        usage(value, events, &raw).await;
    }
    if !state.streamed_text
        && let Some(text) = result["result"]["result"]
            .as_str()
            .filter(|text| !text.is_empty())
    {
        emit(
            events,
            EventKind::TextDelta {
                text: text.into(),
                item_id: None,
            },
            &raw,
        )
        .await;
    }
    if outcome == TurnOutcome::Failed {
        emit(
            events,
            EventKind::Error {
                message: state
                    .failure
                    .clone()
                    .or_else(|| result["errorCode"].as_str().map(String::from))
                    .unwrap_or_else(|| "Cursor run failed".into()),
            },
            &raw,
        )
        .await;
    }
    let _ = events
        .send(BackendEvent::TurnFinished {
            outcome,
            native_reason: Some(status.into()),
            raw: Some(raw),
        })
        .await;
}

#[cfg(all(test, unix))]
mod setup_tests {
    use super::*;
    #[tokio::test]
    async fn account_verification_uses_bridge_control_and_me_without_agent_or_model_calls() {
        let root = std::env::temp_dir().join(format!("swarmie-cursor-me-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cursor_sdk_bridge.py");
        let launcher = root.join("cursor-sdk-bridge");
        std::fs::write(
            &launcher,
            format!(
                "#!/bin/sh\nexec python3 '{}' --workspace '{}' \"$@\"\n",
                script.to_string_lossy().replace('\'', "'\\''"),
                root.to_string_lossy().replace('\'', "'\\''")
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut options = SpawnOptions::new("cursor", &root);
        options.config = json!({"executable":launcher});
        options
            .execution
            .env
            .insert("CURSOR_API_KEY".into(), "fixture-cursor-account-key".into());
        verify_account(&options, "fixture-cursor-account-key")
            .await
            .unwrap();
        let requests: Vec<Value> = std::fs::read_to_string(root.join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            requests
                .iter()
                .map(|r| r["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["Ping", "GetVersion", "Me", "Shutdown"]
        );
        use crate::setup::NativeSetup;
        let native = crate::setup::native::NativeAgentTools::new();
        let overlay = swarmie::AgentToolExecutionOverlay {
            config: options.config.clone(),
            env: options.execution.env.clone(),
            ..Default::default()
        };
        let readiness = native.readiness_with("cursor", &overlay).await.unwrap();
        assert!(readiness.installed);
        assert_eq!(readiness.auth_state, "authenticated");
        let requests: Vec<Value> = std::fs::read_to_string(root.join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(requests.iter().all(|request| matches!(
            request["method"].as_str(),
            Some("Ping" | "GetVersion" | "Me" | "Shutdown")
        )));
        std::fs::remove_dir_all(root).unwrap();
    }
}
