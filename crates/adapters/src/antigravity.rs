use super::*;
use async_trait::async_trait;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, RwLock};

pub struct AntigravityHarness;
fn capabilities() -> Capabilities {
    Capabilities {
        resume: true,
        cancel: true,
        usage: true,
        models: true,
        metadata: BTreeMap::from([
            ("wire".into(), json!("native_stream_json")),
            ("contextUsage".into(), json!(true)),
            ("toolsDisabled".into(), json!(false)),
            ("ephemeral".into(), json!(false)),
            ("outputSchema".into(), json!(true)),
            ("mcpTransports".into(), json!([])),
            ("configurationScope".into(), json!("creation_only")),
            ("cancelStrategy".into(), json!("restart_and_resume")),
        ]),
        ..Default::default()
    }
}
#[async_trait]
impl Harness for AntigravityHarness {
    fn id(&self) -> &str {
        "antigravity"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(detection("antigravity", "agy", capabilities()))
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        readiness::probe(self.detect().await?, None).await
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        if fork {
            return Err(Error::Unsupported("Antigravity native stream fork".into()));
        }
        if !options.execution.mcp_servers.is_empty() {
            return Err(Error::Unsupported(
                "Antigravity session-local MCP injection".into(),
            ));
        }
        if options.execution.tools == ToolPolicy::Disabled {
            return Err(Error::Unsupported(
                "Antigravity strict tool disabling".into(),
            ));
        }
        if options.execution.ephemeral {
            return Err(Error::Unsupported("Antigravity ephemeral sessions".into()));
        }
        let env = execution_env(options, None)?;
        let (wire, frames, init, early) = start(options, saved.map(|s| s.id.as_str()), env).await?;
        let id = conversation(&init)?.to_owned();
        let state = Arc::new(Mutex::new(Live {
            wire: Some(wire),
            active: false,
            tools: BTreeMap::new(),
        }));
        let epoch = Arc::new(AtomicU64::new(0));
        let (tx, events) = mpsc::channel(512);
        for frame in early {
            let kind = frame
                .get("_runtime_diagnostic")
                .cloned()
                .map(EventKind::Diagnostic)
                .unwrap_or(EventKind::Raw);
            tx.send(BackendEvent::content(kind, frame))
                .await
                .map_err(|_| Error::Closed)?;
        }
        tx.send(BackendEvent::content(
            EventKind::ConfigurationUpdated(init["init"].clone()),
            init.clone(),
        ))
        .await
        .map_err(|_| Error::Closed)?;
        pump(
            frames,
            Arc::downgrade(&state),
            epoch.clone(),
            0,
            tx.clone(),
            id.clone(),
        );
        Ok(BackendConnection {
            backend: Box::new(AntigravityBackend {
                state,
                epoch,
                options: options.clone(),
                id: id.clone(),
                init: RwLock::new(init),
                tx,
                operation: Mutex::new(()),
            }),
            native: native("antigravity", id),
            capabilities: capabilities(),
            events,
        })
    }
}
fn conversation(init: &Value) -> Result<&str> {
    init["conversation_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Provider("Antigravity init omitted conversation_id".into()))
}
async fn start(
    options: &SpawnOptions,
    saved: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<(JsonProcess, mpsc::Receiver<Value>, Value, Vec<Value>)> {
    let (exe, mut args) = launch(options, "agy", vec![]);
    args.extend([
        "--input-format=stream-json".into(),
        "--output-format=stream-json".into(),
        "--add-dir".into(),
        options.cwd.to_string_lossy().into_owned(),
    ]);
    if let Some(id) = saved {
        args.extend(["--conversation".into(), id.into()]);
    }
    if let Some(model) = &options.model {
        args.extend(["--model".into(), model.clone()]);
    }
    for (key, allowed) in [
        ("mode", &["accept-edits", "plan"][..]),
        ("effort", &["low", "medium", "high", "xhigh", "max"][..]),
    ] {
        if let Some(value) = options.config.get(key) {
            let value = value
                .as_str()
                .filter(|v| allowed.contains(v))
                .ok_or_else(|| Error::Invalid(format!("invalid Antigravity {key}")))?;
            args.extend([format!("--{key}"), value.into()]);
        }
    }
    if let Some(agent) = options.config.get("agent") {
        let agent = agent
            .as_str()
            .filter(|a| !a.is_empty())
            .ok_or_else(|| Error::Invalid("Antigravity agent must be a nonempty string".into()))?;
        args.extend(["--agent".into(), agent.into()]);
    }
    if options.config["autoApprove"] == true {
        args.push("--dangerously-skip-permissions".into());
    }
    if let Some(schema) = &options.execution.output_schema {
        if schema["type"] != "object" {
            return Err(Error::Invalid(
                "Antigravity requires an object output schema".into(),
            ));
        }
        args.extend(["--json-schema".into(), serde_json::to_string(schema)?]);
    }
    let (wire, mut frames) = spawn_json(options, &exe, &args, WireStyle::Claude, env).await?;
    let mut early = vec![];
    let initialized = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(frame) = frames.recv().await {
            if frame["event"] == "init" {
                conversation(&frame)?;
                return Ok(frame);
            }
            if frame["event"] == "result" {
                return Err(Error::Provider(
                    frame["result"]["error"]
                        .as_str()
                        .unwrap_or("Antigravity refused to initialize")
                        .into(),
                ));
            }
            if let Some(error) = frame
                .get("_runtime_exit")
                .or_else(|| frame.get("_runtime_error"))
                .and_then(Value::as_str)
            {
                return Err(Error::Provider(error.into()));
            }
            if early.len() < 256 {
                early.push(frame);
            }
        }
        Err(Error::Provider("Antigravity closed before init".into()))
    })
    .await;
    match initialized {
        Ok(Ok(init)) => Ok((wire, frames, init, early)),
        outcome => {
            let _ = wire.kill().await;
            match outcome {
                Err(_) => Err(Error::Timeout("Antigravity init".into())),
                Ok(Err(error)) => Err(Error::Provider(format!("{error}; {}", wire.stderr()))),
                Ok(Ok(_)) => unreachable!(),
            }
        }
    }
}
struct Live {
    wire: Option<JsonProcess>,
    active: bool,
    tools: BTreeMap<String, ToolEvent>,
}
struct AntigravityBackend {
    state: Arc<Mutex<Live>>,
    epoch: Arc<AtomicU64>,
    options: SpawnOptions,
    id: String,
    init: RwLock<Value>,
    tx: mpsc::Sender<BackendEvent>,
    operation: Mutex<()>,
}
fn pump(
    mut frames: mpsc::Receiver<Value>,
    state: Weak<Mutex<Live>>,
    epoch: Arc<AtomicU64>,
    generation: u64,
    tx: mpsc::Sender<BackendEvent>,
    conversation_id: String,
) {
    tokio::spawn(async move {
        let mut message: Option<String> = None;
        let mut streamed = false;
        let mut last_context: Option<u64> = None;
        let mut counted = BTreeSet::new();
        while let Some(frame) = frames.recv().await {
            if epoch.load(Ordering::SeqCst) != generation {
                break;
            }
            let Some(state) = state.upgrade() else {
                break;
            };
            let mut live = state.lock().await;
            if epoch.load(Ordering::SeqCst) != generation {
                break;
            }
            let mut kinds = vec![];
            let mut finished = None;
            if let Some(diagnostic) = frame.get("_runtime_diagnostic") {
                kinds.push(EventKind::Diagnostic(diagnostic.clone()));
            } else if let Some(error) = frame
                .get("_runtime_exit")
                .or_else(|| frame.get("_runtime_error"))
                .and_then(Value::as_str)
            {
                live.active = false;
                for (_, mut tool) in std::mem::take(&mut live.tools) {
                    tool.status = "cancelled".into();
                    kinds.push(EventKind::ToolCompleted(tool));
                }
                for kind in kinds {
                    if tx
                        .send(BackendEvent::content(kind, frame.clone()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                let _ = tx
                    .send(BackendEvent::Exited {
                        message: error.into(),
                    })
                    .await;
                break;
            } else {
                match frame["event"].as_str().unwrap_or_default() {
                    "step_update" => {
                        let step = &frame["step_update"];
                        let index = step["step_index"].as_u64();
                        let id = index.map(|i| format!("step-{i}"));
                        let done = matches!(step["state"].as_str(), Some("DONE" | "ERROR"));
                        match step["step_type"].as_str().unwrap_or_default() {
                            "agent_response" => {
                                if let Some(text) = step["text_delta"].as_str().filter(|t| !t.is_empty()) {
                                    streamed = true; message = id.clone();
                                    kinds.push(EventKind::TextDelta {text: text.into(), item_id: id.clone()});
                                }
                                if done && step["usage"].is_object() && counted.insert(index) {
                                    let usage = &step["usage"];
                                    last_context = usage["input_tokens"].as_u64().zip(usage["output_tokens"].as_u64()).map(|(i,o)| i.saturating_add(o).saturating_add(usage["cache_read_tokens"].as_u64().unwrap_or(0)));
                                }
                                if done && let Some(item_id) = message.take() { kinds.push(EventKind::MessageEnded {item_id: Some(item_id)}); }
                            }
                            "tool" | "subagent" => {
                                if let Some(id) = id {
                                    let info = &step["tool_info"];
                                    let previous = live.tools.get(&id);
                                    let status = if step["state"] == "ERROR" || !info["error"].is_null() {"failed"} else if done {"completed"} else {"running"};
                                    let mut tool = ToolEvent { details: None,
                                        id: id.clone(),
                                        name: step["tool_name"].as_str().or(info["name"].as_str()).map(String::from).or_else(|| previous.and_then(|p| p.name.clone())),
                                        status: status.into(),
                                        input: info.get("parameters").cloned().or_else(|| previous.map(|p| p.input.clone())).unwrap_or(Value::Null),
                                        output: info.get("output").or_else(|| info.get("error")).cloned().unwrap_or(Value::Null),
                                        metadata: json!({"kind":step["step_type"],"native":step,"locations":info["parameters"]["TargetFile"],"durationSeconds":step["duration_seconds"]}),
                                    };
                                    tool.details = Some(tool_details(step, &tool));
                                    let first = previous.is_none();
                                    if done { live.tools.remove(&id); } else { live.tools.insert(id, tool.clone()); }
                                    kinds.push(if done {EventKind::ToolCompleted(tool)} else if first {EventKind::ToolStarted(tool)} else {EventKind::ToolUpdated(tool)});
                                }
                                if let Some(children) = step["subagent_info"]["subagents"].as_array() {
                                    for child in children {
                                        if let Some(id) = child["conversation_id"].as_str() {
                                            kinds.push(EventKind::SubagentUpdated((json!({"id":id,"parentSessionId":conversation_id,"role":child["role"],"name":child["type_name"],"status":if done {"completed"} else {"running"},"metadata":child})).into()));
                                        }
                                    }
                                }
                            }
                            "checkpoint" => kinds.push(EventKind::Raw),
                            "unknown" => kinds.push(EventKind::Diagnostic(json!({"severity":"warning","message":"Antigravity headless mode skipped an interactive step","step":step}))),
                            _ => kinds.push(EventKind::Raw),
                        }
                    }
                    "result" => {
                        let result = &frame["result"];
                        if !streamed
                            && let Some(text) =
                                result["response"].as_str().filter(|t| !t.is_empty())
                        {
                            kinds.push(EventKind::TextDelta {
                                text: text.into(),
                                item_id: None,
                            });
                        }
                        if let Some(item_id) = message.take() {
                            kinds.push(EventKind::MessageEnded {
                                item_id: Some(item_id),
                            });
                        }
                        if let Some(output) = result.get("structured_output") {
                            kinds.push(EventKind::StructuredOutput(output.clone()));
                        }
                        if result["usage"].is_object() {
                            let mut totals = usage(result);
                            totals.scope = "session".into();
                            totals.cached_input_tokens =
                                result["usage"]["cache_read_tokens"].as_u64();
                            kinds.push(EventKind::UsageUpdated(totals));
                        }
                        if let Some(used) = last_context.take() {
                            kinds.push(EventKind::ContextUpdated(ContextUsage {
                                used_tokens: Some(used),
                                ..Default::default()
                            }));
                        }
                        for (_, mut tool) in std::mem::take(&mut live.tools) {
                            tool.status = "cancelled".into();
                            kinds.push(EventKind::ToolCompleted(tool));
                        }
                        let success = result["status"] == "SUCCESS";
                        if !success {
                            kinds.push(EventKind::Error {
                                message: result["error"]
                                    .as_str()
                                    .unwrap_or("Antigravity turn failed")
                                    .into(),
                            });
                        }
                        live.active = false;
                        if let Some(wire) = &live.wire {
                            wire.set_active(false);
                        }
                        streamed = false;
                        counted.clear();
                        finished = Some(if success { "completed" } else { "failed" });
                    }
                    _ => kinds.push(EventKind::Raw),
                }
            }
            // Hold the state lock through delivery so cancellation cannot interleave old-generation events.
            for kind in kinds {
                if tx
                    .send(BackendEvent::content(kind, frame.clone()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            if let Some(reason) = finished
                && tx
                    .send(BackendEvent::TurnFinished {
                        reason: reason.into(),
                        raw: Some(frame),
                    })
                    .await
                    .is_err()
            {
                return;
            }
        }
    });
}
#[async_trait]
impl AgentBackend for AntigravityBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        let text = prompt.text()?;
        let _operation = self.operation.lock().await;
        let mut live = self.state.lock().await;
        if live.active {
            return Err(Error::Busy);
        }
        let wire = live.wire.as_ref().ok_or(Error::Closed)?.clone();
        wire.set_active(true);
        live.active = true;
        if let Err(error) = wire
            .send(json!({"event":"user","message":{"content":text}}))
            .await
        {
            wire.set_active(false);
            live.active = false;
            return Err(error);
        }
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        let _operation = self.operation.lock().await;
        let mut live = self.state.lock().await;
        if !live.active {
            return Ok(());
        }
        let generation = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let wire = live.wire.take().ok_or(Error::Closed)?;
        wire.set_active(false);
        live.active = false;
        let cancelled = std::mem::take(&mut live.tools);
        drop(live);
        for (_, mut tool) in cancelled {
            tool.status = "cancelled".into();
            self.tx
                .send(BackendEvent::content(
                    EventKind::ToolCompleted(tool),
                    Value::Null,
                ))
                .await
                .map_err(|_| Error::Closed)?;
        }
        wire.kill().await?;
        let restarted = start(
            &self.options,
            Some(&self.id),
            execution_env(&self.options, None)?,
        )
        .await;
        let (wire, frames, init, early) = match restarted {
            Ok(connection) => connection,
            Err(error) => {
                let _ = self
                    .tx
                    .send(BackendEvent::Exited {
                        message: format!(
                            "Antigravity could not resume after cancellation: {error}"
                        ),
                    })
                    .await;
                return Err(error);
            }
        };
        if conversation(&init)? != self.id {
            wire.kill().await?;
            let message = "Antigravity resumed a different conversation after cancellation";
            let _ = self
                .tx
                .send(BackendEvent::Exited {
                    message: message.into(),
                })
                .await;
            return Err(Error::Provider(message.into()));
        }
        self.state.lock().await.wire = Some(wire);
        *self.init.write().await = init;
        for frame in early {
            let _ = self
                .tx
                .send(BackendEvent::content(EventKind::Raw, frame))
                .await;
        }
        self.tx
            .send(BackendEvent::TurnFinished {
                reason: "cancelled".into(),
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)?;
        pump(
            frames,
            Arc::downgrade(&self.state),
            self.epoch.clone(),
            generation,
            self.tx.clone(),
            self.id.clone(),
        );
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Err(Error::Unsupported(
            "Antigravity headless interactive permissions".into(),
        ))
    }
    async fn configuration(&self) -> Result<Value> {
        Ok(
            json!({"options":[],"metadata":{"scope":"creation_only","native":self.init.read().await["init"]}}),
        )
    }
    async fn models(&self) -> Result<Value> {
        let (exe, mut args) = launch(&self.options, "agy", vec![]);
        args.extend(["--output-format=json".into(), "models".into()]);
        let mut command = tokio::process::Command::new(exe);
        command
            .args(args)
            .current_dir(&self.options.cwd)
            .envs(execution_env(&self.options, None)?)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output())
            .await
            .map_err(|_| Error::Timeout("Antigravity models".into()))??;
        if !output.status.success() {
            return Err(Error::Provider(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        let report: Value = serde_json::from_slice(&output.stdout)?;
        let models = report["command"]["data"]["models"]
            .as_array()
            .ok_or_else(|| Error::Provider("Antigravity model catalog omitted models".into()))?;
        Ok(Value::Array(models.iter().filter_map(|m| Some(json!({"id":m["id"].as_str()?,"name":m["label"].as_str().or(m["id"].as_str())?,"provider":"antigravity","metadata":m}))).collect()))
    }
    async fn close(&self) -> Result<()> {
        let _operation = self.operation.lock().await;
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let wire = self.state.lock().await.wire.take();
        match wire {
            Some(wire) => wire.close().await,
            None => Ok(()),
        }
    }
    async fn kill(&self) -> Result<()> {
        let _operation = self.operation.lock().await;
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let wire = self.state.lock().await.wire.take();
        match wire {
            Some(wire) => wire.kill().await,
            None => Ok(()),
        }
    }
}
