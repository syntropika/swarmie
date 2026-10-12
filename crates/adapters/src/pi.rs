use super::*;
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::RwLock;

pub struct PiHarness;
impl PiHarness {
    /// Native Pi branch: cuts before the chosen user entry and returns its draft in a session event.
    /// This is distinct from Runtime::fork_at, which retains a completed assistant turn.
    pub async fn fork_before_user(
        &self,
        options: &SpawnOptions,
        saved: &NativeSessionRef,
        entry_id: &str,
    ) -> Result<BackendConnection> {
        connect_pi(options, Some(saved), true, Some(entry_id)).await
    }
}
fn capabilities() -> Capabilities {
    Capabilities {
        fork_before_user: true.into(),
        commands: true.into(),
        ephemeral: true.into(),
        output_schema: false.into(),
        tools_disabled: true.into(),
        fork_at_message: false.into(),

        resume: true.into(),
        fork: true.into(),
        steering: true.into(),
        cancel: true.into(),
        reasoning: true.into(),
        usage: true.into(),
        permissions: true.into(),
        models: true.into(),
        configure: true.into(),
        compact: true.into(),
        images: true.into(),
        metadata: BTreeMap::from([
            (
                "nativeUserEntryFork".into(),
                json!({"position":"before_user_message","target":"user_entry_id","api":"PiHarness::fork_before_user"}),
            ),
            ("mcpTransports".into(), json!([])),
        ]),
        ..Default::default()
    }
}
#[async_trait]
impl Harness for PiHarness {
    fn id(&self) -> &str {
        "pi"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(detection("pi", "pi", capabilities()))
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        readiness::probe(self.detect().await?, None).await
    }
    async fn fork_before_user(
        &self,
        options: &SpawnOptions,
        native: &NativeSessionRef,
        message_id: &str,
    ) -> Result<BackendConnection> {
        PiHarness::fork_before_user(self, options, native, message_id).await
    }
    async fn native_history(
        &self,
        query: &NativeHistoryQuery,
    ) -> Result<Vec<NativeHistorySession>> {
        if query.harness != self.id() {
            return Err(Error::Invalid(
                "history query harness does not match adapter".into(),
            ));
        }
        super::history::discover(query).await
    }
    async fn read_native_history(
        &self,
        query: &NativeHistoryQuery,
        native_id: &str,
    ) -> Result<NativeHistoryDocument> {
        if query.harness != self.id() {
            return Err(Error::Invalid(
                "history query harness does not match adapter".into(),
            ));
        }
        super::history::read(query, native_id, capabilities()).await
    }
    async fn connect(
        &self,
        o: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        connect_pi(o, saved, fork, None).await
    }
}
async fn connect_pi(
    o: &SpawnOptions,
    saved: Option<&NativeSessionRef>,
    fork: bool,
    entry: Option<&str>,
) -> Result<BackendConnection> {
    if !o.execution.mcp_servers.is_empty() {
        return Err(Error::Unsupported("Pi native MCP servers".into()));
    }
    if o.execution.output_schema.is_some() {
        return Err(Error::Unsupported("Pi structured output schema".into()));
    }
    if o.execution.ephemeral && saved.is_some() {
        return Err(Error::Unsupported("Pi ephemeral resume or fork".into()));
    }
    let args = vec!["--mode".into(), "rpc".into()];
    let (exe, mut args) = launch(o, "pi", args);
    if let Some(m) = &o.model {
        args.extend(["--model".into(), m.clone()]);
    }
    if let Some(s) = saved {
        args.extend(["--session".into(), s.id.clone()]);
    }
    if o.execution.tools == ToolPolicy::Disabled {
        if args
            .iter()
            .any(|a| a == "-e" || a == "--extension" || a.starts_with("--extension="))
        {
            return Err(Error::Unsupported(
                "Pi strict tool disabling with explicit extensions".into(),
            ));
        }
        args.extend(["--no-tools".into(), "--no-extensions".into()]);
    }
    if o.execution.ephemeral {
        args.push("--no-session".into());
    }
    let env = execution_env(o, Some("PI_CODING_AGENT_DIR"))?;
    let (wire, mut frames) = spawn_json(o, &exe, &args, WireStyle::Pi, env).await?;
    let fork_receipt = if fork {
        let result = match entry {
            Some(entry) => wire.request("fork", json!({"entryId":entry})).await?,
            None => wire.request("clone", json!({})).await?,
        };
        if result["cancelled"] == true {
            return Err(Error::Provider("Pi fork cancelled by an extension".into()));
        }
        Some(result)
    } else {
        None
    };
    let state = wire.request("get_state", json!({})).await?;
    let id = state["sessionFile"]
        .as_str()
        .or_else(|| state["sessionId"].as_str())
        .ok_or_else(|| Error::Provider("Pi get_state omitted session reference".into()))?
        .to_string();
    let settled = Arc::new(AtomicBool::new(o.config["legacyAgentEnd"] != true));
    let outcome: Arc<RwLock<(TurnOutcome, Option<String>)>> =
        Arc::new(RwLock::new((TurnOutcome::Completed, None)));
    let outcome_task = outcome.clone();
    let settled_task = settled.clone();
    let (tx, events) = mpsc::channel(512);
    let tx_backend = tx.clone();
    if let Some(receipt) = fork_receipt {
        tx.send(BackendEvent::content(EventKind::SessionUpdated(json!({"nativeFork": {"position":if entry.is_some() {"before_user_message"} else {"clone"},"draft":receipt["text"]}})), receipt)).await.map_err(|_| Error::Closed)?;
    }
    let dialogs: Arc<RwLock<BTreeMap<String, Value>>> = Arc::default();
    let dialog_task = dialogs.clone();
    let context_window = Arc::new(AtomicU64::new(
        state["model"]["contextWindow"].as_u64().unwrap_or(0),
    ));
    let window_task = context_window.clone();
    let wire_task = wire.clone();
    tokio::spawn(async move {
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
            let kind = f["type"].as_str().unwrap_or("");
            let event = match kind {
                "agent_settled" => {
                    wire_task.set_active(false);
                    settled_task.store(true, Ordering::SeqCst);
                    let (outcome, native_reason) = outcome_task.read().await.clone();
                    BackendEvent::TurnFinished {
                        outcome,
                        native_reason,
                        raw: Some(f.clone()),
                    }
                }
                "agent_end" if !settled_task.load(Ordering::SeqCst) => {
                    wire_task.set_active(false);
                    // Legacy Pi lacks agent_settled. Modern state exposes isRunning; explicit override is available.
                    let (outcome, native_reason) = outcome_task.read().await.clone();
                    BackendEvent::TurnFinished {
                        outcome,
                        native_reason,
                        raw: Some(f.clone()),
                    }
                }
                "extension_ui_request"
                    if ["select", "confirm", "input", "editor"]
                        .contains(&f["method"].as_str().unwrap_or("")) =>
                {
                    let id = f["id"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| f["id"].to_string());
                    dialog_task.write().await.insert(id.clone(), f.clone());
                    if let Some(timeout) = f["timeout"].as_u64() {
                        let pending = dialog_task.clone();
                        let sender = tx.clone();
                        let expired = id.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(timeout)).await;
                            if pending.write().await.remove(&expired).is_some() {
                                let _ = sender
                                    .send(BackendEvent::Content {
                                        kind: Box::new(EventKind::PermissionResolved {
                                            id: expired,
                                            option: "expired".into(),
                                        }),
                                        raw: None,
                                    })
                                    .await;
                            }
                        });
                    }
                    let choices = if f["method"] == "confirm" {
                        settings::choices(&["yes", "no"])
                    } else {
                        Value::Array(
                            f["options"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| {
                                            v.as_str().map(|s| json!({"id":s,"label":s}))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                        )
                    };
                    BackendEvent::content(
                        EventKind::PermissionRequested(canonical_permission(PermissionRequest {
                            questions: Vec::new(),
                            id,
                            kind: "question".into(),
                            title: f["title"].as_str().unwrap_or("Agent needs input").into(),
                            description: f["message"].as_str().map(String::from),
                            command: None,
                            paths: vec![],
                            options: vec![
                                option("answer", "Submit answer", "answer"),
                                option("cancel", "Cancel", "cancel"),
                            ],
                            metadata: json!({"questions":[{"id":"value","prompt":f["message"].as_str().or_else(|| f["title"].as_str()).unwrap_or("Answer the question"),"options":choices,"multiple":false,"allowText":f["method"]=="input" || f["method"]=="editor"}],"timeoutMs":f["timeout"],"native":f}),
                        })),
                        f.clone(),
                    )
                }
                "message_update" => {
                    let e = &f["assistantMessageEvent"];
                    let kind = match e["type"].as_str() {
                        Some("text_delta") => EventKind::TextDelta {
                            text: string(e, "delta"),
                            item_id: None,
                        },
                        Some("thinking_delta") => EventKind::ReasoningDelta {
                            text: string(e, "delta"),
                        },
                        _ => EventKind::Raw,
                    };
                    BackendEvent::content(kind, f.clone())
                }
                "message_end" => {
                    let message = &f["message"];
                    if message["role"] == "assistant" {
                        let reason = message["stopReason"].as_str();
                        let mut held = outcome_task.write().await;
                        if held.0 != TurnOutcome::Cancelled {
                            held.0 = outcome::pi(reason);
                        }
                        held.1 = reason.map(String::from);
                    }
                    if let Some(usage) = message.get("usage") {
                        let context = match wire_task
                            .request_timeout("get_session_stats", json!({}), Duration::from_secs(5))
                            .await
                        {
                            Ok(stats) => native_context(
                                &stats["contextUsage"],
                                window_task.load(Ordering::SeqCst),
                            ),
                            Err(_) => {
                                native_context(&Value::Null, window_task.load(Ordering::SeqCst))
                            }
                        };
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::ContextUpdated(context),
                                f.clone(),
                            ))
                            .await;
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::UsageUpdated(super::usage(usage)),
                                f.clone(),
                            ))
                            .await;
                    }
                    if message["stopReason"] == "error" {
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::Error {
                                    message: message["errorMessage"]
                                        .as_str()
                                        .unwrap_or("Pi model error")
                                        .into(),
                                },
                                f.clone(),
                            ))
                            .await;
                    }
                    BackendEvent::content(
                        if message["role"] == "assistant" {
                            EventKind::AssistantMessage(message.clone())
                        } else {
                            EventKind::Raw
                        },
                        f.clone(),
                    )
                }
                "model_change" | "thinking_level_change" => {
                    BackendEvent::content(EventKind::ConfigurationUpdated(f.clone()), f.clone())
                }
                "tool_execution_start" => {
                    BackendEvent::content(EventKind::ToolStarted(tool(&f, "running")), f.clone())
                }
                "tool_execution_update" => {
                    BackendEvent::content(EventKind::ToolUpdated(tool(&f, "running")), f.clone())
                }
                "tool_execution_end" => BackendEvent::content(
                    EventKind::ToolCompleted(tool(&f, "completed")),
                    f.clone(),
                ),
                _ => BackendEvent::content(EventKind::Raw, f.clone()),
            };
            if tx.send(event).await.is_err() {
                break;
            }
            if f["type"] == "message_end" && f["message"]["role"] == "assistant" {
                let _ = tx
                    .send(BackendEvent::content(
                        EventKind::MessageEnded {
                            item_id: f["message"]["id"].as_str().map(String::from),
                        },
                        f.clone(),
                    ))
                    .await;
            }
        }
    });
    // Current Pi emits agent_settled. Legacy completion must be explicitly selected to avoid premature idle.
    settled.store(o.config["legacyAgentEnd"] != true, Ordering::SeqCst);
    Ok(BackendConnection {
        native: native("pi", id),
        capabilities: {
            let mut caps = capabilities();
            caps.resume = if o.execution.ephemeral {
                Capability::unavailable("Ephemeral sessions do not retain native history")
            } else {
                true.into()
            };
            caps.fork = caps.resume.clone();
            if o.execution.ephemeral {
                caps.metadata
                    .insert("nativeUserEntryFork".into(), json!(false));
            }
            caps
        },
        events,
        backend: Box::new(PiBackend {
            wire,
            tx: tx_backend,
            outcome,
            dialogs,
            context_window,
        }),
    })
}
fn native_context(value: &Value, fallback_window: u64) -> ContextUsage {
    let used = value["tokens"].as_u64();
    let window = value["contextWindow"]
        .as_u64()
        .or(Some(fallback_window).filter(|v| *v > 0));
    ContextUsage {
        used_tokens: used,
        context_window_tokens: window,
        remaining_tokens: used.zip(window).map(|(u, w)| w.saturating_sub(u)),
        percent: value["percent"].as_f64().or_else(|| {
            used.zip(window)
                .filter(|(_, w)| *w > 0)
                .map(|(u, w)| u as f64 / w as f64 * 100.0)
        }),
    }
}
struct PiBackend {
    wire: JsonProcess,
    tx: mpsc::Sender<BackendEvent>,
    outcome: Arc<RwLock<(TurnOutcome, Option<String>)>>,
    dialogs: Arc<RwLock<BTreeMap<String, Value>>>,
    context_window: Arc<AtomicU64>,
}
fn params(p: &Prompt) -> Result<Value> {
    let mut texts = vec![];
    let mut images = vec![];
    for c in &p.content {
        match c {
            PromptContent::Text(t) => texts.push(t.clone()),
            PromptContent::Image { data, mime_type } => {
                images.push(json!({"type":"image","data":data,"mimeType":mime_type}))
            }
            _ => return Err(Error::Unsupported("Pi resource content".into())),
        }
    }
    Ok(json!({"message":texts.join("\n"),"images":images}))
}
#[async_trait]
impl AgentBackend for PiBackend {
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        let params = params(p)?;
        *self.outcome.write().await = (TurnOutcome::Completed, None);
        self.wire.set_active(true);
        let r = match self.wire.request("prompt", params).await {
            Ok(r) => r,
            Err(e) => {
                self.wire.set_active(false);
                return Err(e);
            }
        };
        if r["disposition"] == "handled" {
            self.wire.set_active(false);
            self.tx
                .send(BackendEvent::TurnFinished {
                    outcome: TurnOutcome::Handled,
                    native_reason: Some("handled".into()),
                    raw: Some(r),
                })
                .await
                .map_err(|_| Error::Closed)?;
        }
        Ok(())
    }
    async fn steer(&self, p: &Prompt) -> Result<()> {
        self.wire.request("steer", params(p)?).await?;
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        *self.outcome.write().await = (TurnOutcome::Cancelled, None);
        self.wire.request("clear_queue", json!({})).await?;
        self.wire
            .request_timeout("abort", json!({}), Duration::from_secs(15))
            .await?;
        Ok(())
    }
    async fn respond_permission(&self, r: &PermissionResponse) -> Result<()> {
        let mut dialogs = self.dialogs.write().await;
        let dialog = dialogs
            .get(&r.id)
            .ok_or_else(|| Error::Invalid("unknown Pi dialog".into()))?;
        let mut response = json!({"type":"extension_ui_response","id":dialog["id"]});
        if r.option == "cancel" {
            response["cancelled"] = json!(true);
        } else {
            let answer = &r.data["answers"]["value"];
            if dialog["method"] == "confirm" {
                response["confirmed"] = json!(answer == "yes");
            } else {
                response["value"] = answer.clone();
            }
        }
        self.wire.send(response).await?;
        dialogs.remove(&r.id);
        Ok(())
    }
    async fn configure(&self, v: Value) -> Result<Value> {
        let fields = v
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        if let Some(key) = fields
            .keys()
            .find(|k| !["model", "thinkingLevel"].contains(&k.as_str()))
        {
            return Err(Error::Invalid(format!("unknown configuration {key}")));
        }
        if let Some(model) = v.get("model") {
            let model = if let Some(id) = model.as_str() {
                let (provider, model) = id
                    .split_once('/')
                    .ok_or_else(|| Error::Invalid("Pi model requires provider/model".into()))?;
                json!({"provider":provider,"modelId":model})
            } else {
                model.clone()
            };
            self.wire.request("set_model", model).await?;
        }
        if let Some(level) = v.get("thinkingLevel") {
            let available = self
                .wire
                .request("get_available_thinking_levels", json!({}))
                .await?;
            if !available["levels"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == level))
            {
                return Err(Error::Invalid(
                    "thinking level is not supported by the selected model".into(),
                ));
            }
            self.wire
                .request("set_thinking_level", json!({"level":level}))
                .await?;
        }
        let state = self.wire.request("get_state", json!({})).await?;
        let window = state["model"]["contextWindow"].as_u64().filter(|w| *w > 0);
        self.context_window
            .store(window.unwrap_or(0), Ordering::SeqCst);
        if v.get("model").is_some() {
            self.tx
                .send(BackendEvent::Content {
                    kind: Box::new(EventKind::ContextUpdated(ContextUsage {
                        context_window_tokens: window,
                        ..Default::default()
                    })),
                    raw: None,
                })
                .await
                .map_err(|_| Error::Closed)?;
        }
        self.tx
            .send(BackendEvent::Content {
                kind: Box::new(EventKind::ConfigurationUpdated(state.clone())),
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)?;
        Ok(state)
    }
    async fn commands(&self) -> Result<Value> {
        let result = self.wire.request("get_commands", json!({})).await?;
        let mut commands = result["commands"].clone();
        if let Some(values) = commands.as_array_mut() {
            for command in values {
                if let Some(name) = command["name"].as_str() {
                    command["invocation"] = json!(format!("/{} ", name.trim_start_matches('/')));
                }
            }
        }
        Ok(commands)
    }
    async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        let models = self.wire.request("get_available_models", json!({})).await?;
        settings::model_descriptors(Value::Array(settings::models(&models).iter().filter_map(|m| Some(json!({"id":format!("{}/{}",m["provider"].as_str()?,m["id"].as_str()?),"label":m["name"].as_str().or(m["id"].as_str()),"provider":m["provider"],"metadata":m}))).collect()))
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let state = self.wire.request("get_state", json!({})).await?;
        let models = self.wire.request("get_available_models", json!({})).await?;
        let choices = Value::Array(settings::models(&models).iter().filter_map(|m| Some(json!({"id":format!("{}/{}",m["provider"].as_str()?,m["id"].as_str()?),"label":m["name"].as_str().or(m["id"].as_str()),"provider":m["provider"],"metadata":m}))).collect());
        let model = state["model"].clone();
        let mut descriptors = vec![settings::select(
            "model",
            "Model",
            json!(format!(
                "{}/{}",
                model["provider"].as_str().unwrap_or(""),
                model["id"].as_str().unwrap_or("")
            )),
            choices,
        )];
        if model["reasoning"] == true {
            let levels = self
                .wire
                .request("get_available_thinking_levels", json!({}))
                .await?;
            if let Some(levels) = levels["levels"].as_array() {
                descriptors.push(settings::select(
                    "thinkingLevel",
                    "Thinking",
                    state["thinkingLevel"].clone(),
                    json!(
                        levels
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|id| json!({"id":id,"label":id}))
                            .collect::<Vec<_>>()
                    ),
                ));
            }
        }
        settings::descriptors(json!(descriptors))
    }
    async fn compact(&self) -> Result<()> {
        self.wire
            .request_timeout("compact", json!({}), Duration::from_secs(300))
            .await?;
        let stats = self.wire.request("get_session_stats", json!({})).await?;
        self.tx
            .send(BackendEvent::Content {
                kind: Box::new(EventKind::ContextUpdated(native_context(
                    &stats["contextUsage"],
                    self.context_window.load(Ordering::SeqCst),
                ))),
                raw: Some(stats),
            })
            .await
            .map_err(|_| Error::Closed)?;
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        self.wire.close().await
    }
    async fn kill(&self) -> Result<()> {
        self.wire.kill().await
    }
}
