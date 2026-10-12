use super::*;
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Mutex, RwLock, oneshot};

pub struct CodexHarness;
async fn resolve_fork_turn(
    wire: &JsonProcess,
    native: &NativeSessionRef,
    message_id: &str,
) -> Result<String> {
    fn find(turns: &Value, message_id: &str) -> Result<Option<String>> {
        let mut found = None;
        for turn in turns.as_array().into_iter().flatten() {
            if turn["items"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["type"] == "agentMessage" && item["id"] == message_id)
            }) {
                if turn["status"] == "inProgress" {
                    return Err(Error::Busy);
                }
                let id = turn["id"]
                    .as_str()
                    .ok_or_else(|| Error::Provider("fork anchor has no native turn ID".into()))?;
                if found.is_some() {
                    return Err(Error::Invalid("ambiguous native message anchor".into()));
                }
                found = Some(id.into());
            }
        }
        Ok(found)
    }
    let read = wire
        .request(
            "thread/read",
            json!({"threadId":native.id,"includeTurns":true}),
        )
        .await?;
    if let Some(id) = find(&read["thread"]["turns"], message_id)? {
        return Ok(id);
    }
    let mut cursor = Value::Null;
    for _ in 0..100 {
        let response = wire
            .request(
                "thread/turns/list",
                json!({"threadId":native.id,"itemsView":"full","limit":100,"cursor":cursor}),
            )
            .await?;
        if let Some(id) = find(&response["data"], message_id)? {
            return Ok(id);
        }
        let next = response["nextCursor"].clone();
        if next.is_null() {
            return Err(Error::Invalid("native fork message was not found".into()));
        }
        if next == cursor {
            return Err(Error::Provider(
                "native turn pagination did not advance".into(),
            ));
        }
        cursor = next;
    }
    Err(Error::Invalid(
        "native fork lookup exceeds 10,000 turns".into(),
    ))
}
fn configure_mcp(
    servers: &[McpServer],
    args: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> Result<()> {
    let mut namespaces = std::collections::BTreeSet::new();
    for server in servers {
        if server.name.is_empty()
            || !server
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(Error::Invalid(
                "Codex MCP names must contain only letters, digits, underscores or hyphens".into(),
            ));
        }
        let namespace = server.name.to_ascii_uppercase().replace('-', "_");
        if !namespaces.insert(namespace.clone()) {
            return Err(Error::Invalid(
                "MCP names share an environment namespace".into(),
            ));
        }
        let mut override_value = |field: &str, value: String| {
            args.extend([
                "-c".into(),
                format!("mcp_servers.{}.{field}={value}", server.name),
            ]);
        };
        match &server.transport {
            McpTransport::Stdio {
                command,
                args: server_args,
                env: server_env,
            } => {
                override_value(
                    "command",
                    serde_json::to_string(&command.to_string_lossy())?,
                );
                override_value("args", serde_json::to_string(server_args)?);
                for (name, value) in server_env {
                    if matches!(name.as_str(), "CODEX_HOME" | "HOME" | "USERPROFILE")
                        || env
                            .get(name)
                            .cloned()
                            .or_else(|| std::env::var(name).ok())
                            .is_some_and(|held| held != *value)
                    {
                        return Err(Error::Invalid(format!(
                            "MCP environment conflicts at {name}"
                        )));
                    }
                    env.insert(name.clone(), value.clone());
                }
                if !server_env.is_empty() {
                    override_value(
                        "env_vars",
                        serde_json::to_string(&server_env.keys().collect::<Vec<_>>())?,
                    );
                }
            }
            McpTransport::Http { url, headers } => {
                override_value("url", serde_json::to_string(url)?);
                let mut variables = vec![];
                for (index, (header, value)) in headers.iter().enumerate() {
                    let variable = format!("MPAI_MCP_{namespace}_HEADER_{index}");
                    if env.get(&variable).is_some_and(|held| held != value) {
                        return Err(Error::Invalid("MCP header environment collision".into()));
                    }
                    env.insert(variable.clone(), value.clone());
                    variables.push(format!(
                        "{}={}",
                        serde_json::to_string(header)?,
                        serde_json::to_string(&variable)?
                    ));
                }
                if !variables.is_empty() {
                    override_value("env_http_headers", format!("{{{}}}", variables.join(",")));
                }
            }
            McpTransport::Sse { .. } => {
                return Err(Error::Unsupported("Codex SSE MCP transport".into()));
            }
        }
    }
    Ok(())
}
fn normalize_subagent(item: &Value, completed: bool) -> Value {
    let mut child = json!({"id":item["id"],"parentToolId":item["id"],"model":item["model"],"role":item["agentRole"],"status":if completed {"completed"} else {"running"},"threadIds":item["receiverThreadIds"],"operation":item["tool"]});
    if let Some(status) = item["status"].as_str() {
        child["status"] = json!(match status {
            "inProgress" => "running",
            other => other,
        });
    }
    child
}
fn normalize_quotas(value: &Value) -> Value {
    let rate = value["rateLimitsByLimitId"]["codex"]
        .as_object()
        .map(|_| &value["rateLimitsByLimitId"]["codex"])
        .unwrap_or(&value["rateLimits"]);
    let windows:Vec<_> = ["primary","secondary"].into_iter().filter_map(|id| {
        let window=&rate[id];
        window.as_object()?;
        Some(json!({"id":id,"usedPercent":window["usedPercent"],"remainingPercent":window["usedPercent"].as_f64().map(|used|(100.0-used).clamp(0.0,100.0)),"durationMinutes":window["windowDurationMins"],"resetsAt":window["resetsAt"]}))
    }).collect();
    json!({"provider":"codex","plan":rate["planType"],"windows":windows,"resetCredits":value["rateLimitResetCredits"]})
}
fn selected_model<'a>(models: &'a Value, current: &Value) -> Option<&'a Value> {
    models
        .as_array()
        .or_else(|| models["data"].as_array())
        .or_else(|| models["models"].as_array())?
        .iter()
        .find(|m| m["model"] == current["model"] || m["id"] == current["model"])
}
fn tier_choices(model: &Value) -> Value {
    let mut choices = vec![json!({"id":"default","label":"Default"})];
    for tier in model["serviceTiers"].as_array().into_iter().flatten() {
        if let Some(id) = tier.as_str().or_else(|| tier["id"].as_str())
            && id != "default"
        {
            choices.push(json!({"id":id,"label":tier["name"].as_str().unwrap_or(id)}));
        }
    }
    for tier in model["additionalSpeedTiers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !choices.iter().any(|c| c["id"] == tier) {
            choices.push(json!({"id":tier,"label":tier}));
        }
    }
    json!(choices)
}
fn sandbox_policy(value: &Value, cwd: &std::path::Path) -> Result<Value> {
    if value.is_object() {
        return Ok(value.clone());
    }
    match value.as_str() {
        Some("danger-full-access") => Ok(json!({"type":"dangerFullAccess"})),
        Some("workspace-write") => {
            Ok(json!({"type":"workspaceWrite","writableRoots":[cwd],"networkAccess":false}))
        }
        Some("read-only") => Ok(json!({"type":"readOnly","networkAccess":false})),
        _ => Err(Error::Invalid("unknown native sandbox policy".into())),
    }
}
fn configuration_for(models: &Value, current: &Value) -> Value {
    let mut fields = vec![
        settings::select(
            "model",
            "Model",
            current["model"].clone(),
            settings::model_choices(models),
        ),
        settings::select(
            "approvalPolicy",
            "Permissions",
            current["approvalPolicy"].clone(),
            settings::choices(&["untrusted", "on-failure", "on-request", "never"]),
        ),
        settings::select(
            "sandbox",
            "Sandbox",
            current["sandbox"].clone(),
            settings::choices(&["read-only", "workspace-write", "danger-full-access"]),
        ),
    ];
    if current["approvalsReviewer"].is_string() {
        fields.push(settings::select(
            "approvalsReviewer",
            "Approval reviewer",
            current["approvalsReviewer"].clone(),
            settings::choices(&["user", "auto_review"]),
        ));
    }
    if let Some(model) = selected_model(models, current) {
        let efforts: Vec<_> = model["supportedReasoningEfforts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some(json!({"id":e["reasoningEffort"].as_str()?,"label":e["reasoningEffort"]}))
            })
            .collect();
        if !efforts.is_empty() {
            fields.push(settings::select(
                "effort",
                "Thinking",
                current["effort"].clone(),
                json!(efforts),
            ));
        }
        let tiers = tier_choices(model);
        if tiers.as_array().is_some_and(|a| a.len() > 1) {
            fields.push(settings::select(
                "serviceTier",
                "Service tier",
                current["serviceTier"].clone(),
                tiers.clone(),
            ));
        }
        if tiers.as_array().is_some_and(|a| {
            a.iter()
                .any(|tier| tier["id"] == "priority" || tier["id"] == "fast")
        }) {
            fields.push(json!({"id":"fast","label":"Fast mode","type":"boolean","application":"next_turn","value":current["serviceTier"] == "priority" || current["serviceTier"] == "fast"}));
        }
    }
    json!(fields)
}
fn capabilities() -> Capabilities {
    Capabilities {
        commands: true.into(),
        quotas: true.into(),
        ephemeral: true.into(),
        output_schema: true.into(),
        tools_disabled: false.into(),
        fork_at_message: true.into(),
        native_skill_inputs: true.into(),
        submission_reconciliation: true.into(),

        resume: true.into(),
        fork: true.into(),
        steering: true.into(),
        cancel: true.into(),
        permissions: true.into(),
        operation_approvals: true.into(),
        reasoning: true.into(),
        usage: true.into(),
        models: true.into(),
        configure: true.into(),
        rollback: true.into(),
        compact: true.into(),
        images: true.into(),
        metadata: BTreeMap::from([
            ("mcpTransports".into(), json!(["stdio", "http"])),
            ("rollbackScopes".into(), json!(["conversation_only"])),
        ]),
        ..Default::default()
    }
}
#[async_trait]
impl Harness for CodexHarness {
    fn id(&self) -> &str {
        "codex"
    }
    fn compile_execution_profile(
        &self,
        _options: &SpawnOptions,
        profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        super::policy::codex_plan(profile)
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(detection("codex", "codex", capabilities()))
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        readiness::probe(self.detect().await?, Some(&["login", "status"])).await
    }
    async fn account_quotas(&self, query: &AccountQuery) -> Result<Value> {
        let options = query.connection_options();
        let (exe, args) = launch(&options, "codex", vec!["app-server".into()]);
        let env = execution_env(&options, Some("CODEX_HOME"))?;
        let (wire, mut frames) = spawn_json(&options, &exe, &args, WireStyle::JsonRpc, env).await?;
        let drain = tokio::spawn(async move { while frames.recv().await.is_some() {} });
        let result = async {
            wire.request("initialize", json!({"clientInfo":{"name":"mpai","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
            wire.notify("initialized", json!({})).await?;
            let native = wire.request("account/rateLimits/read", json!({})).await?;
            let quotas = normalize_quotas(&native);
            if quotas["windows"].as_array().is_none_or(|windows| windows.is_empty()) {
                return Err(Error::Unsupported("account quotas for this login".into()));
            }
            Ok(quotas)
        }.await;
        let closed = wire.close().await;
        drain.abort();
        closed?;
        result
    }
    async fn fork_at(
        &self,
        options: &SpawnOptions,
        native: &NativeSessionRef,
        message_id: &str,
    ) -> Result<BackendConnection> {
        let mut options = options.clone();
        if !options.config.is_object() {
            options.config = json!({});
        }
        options.config["forkAtMessage"] = json!(message_id);
        self.connect(&options, Some(native), true).await
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
        options: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        if options.execution.tools == ToolPolicy::Disabled {
            return Err(Error::Unsupported(
                "Codex cannot disable every built-in tool".into(),
            ));
        }
        let (exe, mut args) = launch(options, "codex", vec!["app-server".into()]);
        let mut env = execution_env(options, Some("CODEX_HOME"))?;
        configure_mcp(&options.execution.mcp_servers, &mut args, &mut env)?;
        let (wire, mut frames) = spawn_json(options, &exe, &args, WireStyle::JsonRpc, env).await?;
        let initialized = wire.request("initialize",json!({"clientInfo":{"name":"mpai","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
        wire.notify("initialized", json!({})).await?;
        let mut params = json!({"cwd":options.cwd,"model":options.model});
        if options.execution.ephemeral {
            params["ephemeral"] = json!(true);
        }
        for key in [
            "approvalPolicy",
            "approvalsReviewer",
            "sandbox",
            "config",
            "modelProvider",
            "baseInstructions",
            "developerInstructions",
        ] {
            if let Some(v) = options.config.get(key) {
                if key == "sandbox" && v.is_object() {
                    continue;
                }
                params[key] = v.clone();
            }
        }
        let method = if let Some(saved) = saved {
            params["threadId"] = json!(saved.id);
            if fork { "thread/fork" } else { "thread/resume" }
        } else {
            "thread/start"
        };
        if let Some(anchor) = options.config["forkAtMessage"].as_str() {
            if !fork {
                return Err(Error::Invalid("message anchor requires a fork".into()));
            }
            params["lastTurnId"] = json!(
                resolve_fork_turn(&wire, saved.expect("fork requires source"), anchor).await?
            );
        }
        let result = wire.request(method, params).await?;
        let thread = result["thread"]["id"]
            .as_str()
            .ok_or_else(|| Error::Provider("missing Codex thread id".into()))?
            .to_string();
        if !fork && saved.is_some_and(|saved| saved.id != thread) {
            let _ = wire.kill().await;
            return Err(Error::Provider(
                "native resume returned a different thread identity".into(),
            ));
        }
        let turn: Arc<RwLock<Option<String>>> = Arc::default();
        let requests: Arc<RwLock<BTreeMap<String, Value>>> = Arc::default();
        let compact_receipt: Arc<Mutex<Option<oneshot::Sender<Result<()>>>>> = Arc::default();
        let (tx, events) = mpsc::channel(512);
        let wire_task = wire.clone();
        let turn_task = turn.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_task = cancelled.clone();
        let req_task = requests.clone();
        let thread_task = thread.clone();
        let compact_task = compact_receipt.clone();
        let schema_enabled = options.execution.output_schema.is_some();
        tokio::spawn(async move {
            let mut children = BTreeMap::<String, Value>::new();
            while let Some(frame) = frames.recv().await {
                if terminal_frame(&frame, &tx).await {
                    if let Some(receipt) = compact_task.lock().await.take() {
                        let _ = receipt.send(Err(Error::Provider(
                            "Codex exited during compaction".into(),
                        )));
                    }
                    break;
                }
                if let Some(diagnostic) = frame.get("_runtime_diagnostic") {
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::Diagnostic(diagnostic.clone()),
                            frame,
                        ))
                        .await;
                    continue;
                }
                let p = &frame["params"];
                let method = frame["method"].as_str().unwrap_or("");
                // Child sessions share the app-server connection. Their completion cannot end this session.
                let belongs = p["threadId"]
                    .as_str()
                    .map(|id| id == thread_task)
                    .unwrap_or(true);
                if frame.get("id").is_some() && frame.get("method").is_some() {
                    let id = frame["id"].to_string();
                    let permission = match method {
                        "item/commandExecution/requestApproval"
                        | "item/fileChange/requestApproval"
                        | "execCommandApproval"
                        | "applyPatchApproval" => {
                            let options = vec![
                                option("approve", "Approve once", "approve"),
                                option("deny", "Deny", "deny"),
                                option("cancel", "Cancel", "cancel"),
                            ];
                            Some(PermissionRequest {
                                questions: Vec::new(),
                                id: id.clone(),
                                kind:
                                    if method.contains("fileChange") || method.contains("Patch") {
                                        "file"
                                    } else {
                                        "command"
                                    }
                                    .into(),
                                title: p["reason"]
                                    .as_str()
                                    .unwrap_or("Approve agent operation")
                                    .into(),
                                description: None,
                                command: p.get("command").cloned(),
                                paths: p["paths"]
                                    .as_array()
                                    .map(|a| {
                                        a.iter()
                                            .filter_map(Value::as_str)
                                            .map(String::from)
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                options,
                                metadata: json!({"method":method,"params":p}),
                            })
                        }
                        "item/tool/requestUserInput" => Some(PermissionRequest {
                            questions: Vec::new(),
                            id: id.clone(),
                            kind: "question".into(),
                            title: "Agent needs input".into(),
                            description: None,
                            command: None,
                            paths: vec![],
                            options: vec![
                                option("answer", "Submit answers", "answer"),
                                option("cancel", "Cancel", "cancel"),
                            ],
                            metadata: json!({"questions":settings::questions(&p["questions"]),"native":p}),
                        }),
                        "item/permissions/requestApproval" => Some(PermissionRequest {
                            questions: Vec::new(),
                            id: id.clone(),
                            kind: "permissions".into(),
                            title: "Grant requested permissions".into(),
                            description: None,
                            command: None,
                            paths: vec![],
                            options: vec![
                                option("approve", "Approve once", "approve"),
                                option("deny", "Deny", "deny"),
                                option("cancel", "Cancel", "cancel"),
                            ],
                            metadata: p.clone(),
                        }),
                        _ => None,
                    };
                    if let Some(permission) = permission {
                        req_task.write().await.insert(id, frame.clone());
                        if tx
                            .send(BackendEvent::content(
                                EventKind::PermissionRequested(canonical_permission(permission)),
                                frame,
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    } else {
                        let _ = wire_task
                            .reject(frame["id"].clone(), "unsupported server request")
                            .await;
                        let _ = tx.send(BackendEvent::content(EventKind::Raw, frame)).await;
                    }
                    continue;
                }
                if !belongs {
                    let Some(child) = children.get_mut(p["threadId"].as_str().unwrap_or_default())
                    else {
                        let _ = tx.send(BackendEvent::content(EventKind::Raw, frame)).await;
                        continue;
                    };
                    let status = match method {
                        "turn/completed" => p["turn"]["status"].as_str().unwrap_or("completed"),
                        "turn/started" => "running",
                        _ => "running",
                    };
                    if method == "turn/completed" || method == "turn/started" {
                        child["status"] = json!(status);
                    }
                    if method == "thread/tokenUsage/updated" {
                        child["usage"] = json!(usage(p));
                    }
                    if method == "item/agentMessage/delta" {
                        child["textDelta"] = p["delta"].clone();
                    }
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::SubagentUpdated((child.clone()).into()),
                            frame,
                        ))
                        .await;
                    continue;
                }
                if ((method == "item/completed" && p["item"]["type"] == "contextCompaction")
                    || method == "thread/compacted")
                    && let Some(receipt) = compact_task.lock().await.take()
                {
                    let _ = receipt.send(Ok(()));
                }
                let event = match method {
                    "turn/started" => {
                        *turn_task.write().await = p["turn"]["id"].as_str().map(String::from);
                        BackendEvent::content(EventKind::Raw, frame.clone())
                    }
                    "turn/completed" => {
                        let completed = p["turn"]["id"].as_str();
                        let active = turn_task.read().await.clone();
                        if active.as_deref().is_some_and(|id| Some(id) != completed) {
                            BackendEvent::content(EventKind::Raw, frame.clone())
                        } else {
                            *turn_task.write().await = None;
                            wire_task.set_active(false);
                            BackendEvent::TurnFinished {
                                outcome: outcome::codex(
                                    &p["turn"],
                                    cancelled_task.swap(false, Ordering::SeqCst),
                                ),
                                native_reason: p["turn"]["status"].as_str().map(String::from),
                                raw: Some(frame.clone()),
                            }
                        }
                    }
                    "item/agentMessage/delta" => BackendEvent::content(
                        EventKind::TextDelta {
                            text: string(p, "delta"),
                            item_id: p["itemId"].as_str().map(String::from),
                        },
                        frame.clone(),
                    ),
                    "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
                        BackendEvent::content(
                            EventKind::ReasoningDelta {
                                text: string(p, "delta"),
                            },
                            frame.clone(),
                        )
                    }
                    "item/started" | "item/completed" => {
                        let item = &p["item"];
                        let t = item["type"].as_str().unwrap_or("");
                        let kind = match t {
                            "agentMessage" if method == "item/completed" => {
                                EventKind::AssistantMessage(item.clone())
                            }
                            "commandExecution" | "mcpToolCall" | "dynamicToolCall"
                            | "webSearch" => {
                                if method == "item/started" {
                                    EventKind::ToolStarted(tool(item, "running"))
                                } else {
                                    EventKind::ToolCompleted(tool(item, "completed"))
                                }
                            }
                            "fileChange" => EventKind::FileChanged(item.clone()),
                            "plan" => EventKind::PlanUpdated((json!({"id":item["id"],"text":item["text"],"status":if method=="item/completed" {"completed"} else {"running"},"native":item})).into()),
                            "collabAgentToolCall" | "subAgentActivity" => {
                                let info = normalize_subagent(item, method == "item/completed");
                                for id in item["receiverThreadIds"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_str)
                                    .chain(item["agentThreadId"].as_str())
                                {
                                    let child=children.entry(id.into()).or_insert_with(||json!({"id":id,"threadId":id,"parentToolId":item["id"],"model":item["model"],"status":"running"}));
                                    if item["tool"] == "spawnAgent" {
                                        child["model"] = item["model"].clone();
                                    }
                                    let _ = tx
                                        .send(BackendEvent::content(
                                            EventKind::SubagentUpdated((child.clone()).into()),
                                            frame.clone(),
                                        ))
                                        .await;
                                }
                                EventKind::SubagentUpdated((info).into())
                            }
                            _ => EventKind::Raw,
                        };
                        BackendEvent::content(kind, frame.clone())
                    }
                    "item/commandExecution/outputDelta" => BackendEvent::content(
                        EventKind::ToolUpdated(tool(p, "running")),
                        frame.clone(),
                    ),
                    "item/mcpToolCall/progress" => BackendEvent::content(
                        EventKind::ToolUpdated(tool(p, "running")),
                        frame.clone(),
                    ),
                    "account/rateLimits/updated" => BackendEvent::content(
                        EventKind::QuotasUpdated(normalize_quotas(p)),
                        frame.clone(),
                    ),
                    "model/rerouted" if p["fromModel"].is_string() && p["toModel"].is_string() => {
                        BackendEvent::content(
                            EventKind::ModelRerouted(ModelReroute {
                                from_model: string(p, "fromModel"),
                                to_model: string(p, "toModel"),
                                reason: p["reason"].as_str().map(String::from),
                            }),
                            frame.clone(),
                        )
                    }
                    "turn/plan/updated" => BackendEvent::content(
                        EventKind::PlanUpdated((p.clone()).into()),
                        frame.clone(),
                    ),
                    "turn/diff/updated" => {
                        BackendEvent::content(EventKind::Diff(p.clone()), frame.clone())
                    }
                    "thread/tokenUsage/updated" => {
                        let native_usage = &p["tokenUsage"];
                        let used = native_usage["last"]["totalTokens"]
                            .as_u64()
                            .or_else(|| native_usage["total"]["totalTokens"].as_u64());
                        let window = native_usage["modelContextWindow"].as_u64();
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::ContextUpdated(ContextUsage {
                                    used_tokens: used,
                                    context_window_tokens: window,
                                    remaining_tokens: used
                                        .zip(window)
                                        .map(|(u, w)| w.saturating_sub(u)),
                                    percent: used
                                        .zip(window.filter(|w| *w > 0))
                                        .map(|(u, w)| u as f64 / w as f64 * 100.0),
                                }),
                                frame.clone(),
                            ))
                            .await;
                        BackendEvent::content(EventKind::UsageUpdated(usage(p)), frame.clone())
                    }
                    "error" => BackendEvent::content(
                        if p["willRetry"] == true {
                            EventKind::Diagnostic(
                                json!({"code":"provider_retry","message":p["error"]["message"],"level":"warning"}),
                            )
                        } else {
                            EventKind::Error {
                                message: p["error"]["message"]
                                    .as_str()
                                    .unwrap_or("Codex error")
                                    .into(),
                            }
                        },
                        frame.clone(),
                    ),
                    _ => BackendEvent::content(EventKind::Raw, frame.clone()),
                };
                if tx.send(event).await.is_err() {
                    break;
                }
                if method == "item/completed" && p["item"]["type"] == "agentMessage" {
                    if schema_enabled
                        && let Some(text) = p["item"]["text"].as_str()
                        && let Ok(value) = serde_json::from_str(text)
                    {
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::StructuredOutput(value),
                                frame.clone(),
                            ))
                            .await;
                    }
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::MessageEnded {
                                item_id: p["item"]["id"].as_str().map(String::from),
                            },
                            frame.clone(),
                        ))
                        .await;
                }
            }
        });
        let mut capabilities = capabilities();
        if options.execution.ephemeral {
            capabilities.submission_reconciliation =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities.rollback =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities
                .metadata
                .insert("rollbackScopes".into(), json!([]));
            capabilities.resume =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities.fork =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities.fork_at_message =
                Capability::unavailable("Ephemeral sessions do not retain native history");
        }
        let backend = CodexBackend {
            wire,
            thread,
            turn,
            cancelled,
            ephemeral: options.execution.ephemeral,
            requests,
            compact_receipt,
            config: RwLock::new(
                json!({"model":result["model"].as_str().or(options.model.as_deref()),"effort":options.config.get("effort").unwrap_or(&result["reasoningEffort"]),"approvalPolicy":result["approvalPolicy"],"approvalsReviewer":options.config.get("approvalsReviewer").unwrap_or(&result["approvalsReviewer"]),"serviceTier":options.config.get("serviceTier").unwrap_or(&result["serviceTier"]),"sandbox":options.config.get("sandbox").cloned().unwrap_or_else(||result["sandbox"].clone())}),
            ),
            policy_observation: RwLock::new(Some(super::policy::codex_observation(
                super::policy::codex_values(&result),
                PolicyEvidence::NativeReceipt,
                initialized["serverInfo"]["version"]
                    .as_str()
                    .map(String::from),
                &options.cwd,
            ))),
            output_schema: options.execution.output_schema.clone(),
            cwd: options.cwd.clone(),
        };
        let initial: serde_json::Map<_, _> = ["effort", "serviceTier", "fast"]
            .into_iter()
            .filter_map(|key| {
                options
                    .config
                    .get(key)
                    .map(|value| (key.into(), value.clone()))
            })
            .collect();
        if !initial.is_empty()
            && let Err(error) = backend.configure(Value::Object(initial)).await
        {
            let _ = backend.kill().await;
            return Err(error);
        }
        Ok(BackendConnection {
            native: native("codex", backend.thread.clone()),
            capabilities,
            events,
            backend: Box::new(backend),
        })
    }
}
struct CodexBackend {
    wire: JsonProcess,
    thread: String,
    turn: Arc<RwLock<Option<String>>>,
    cancelled: Arc<AtomicBool>,
    ephemeral: bool,
    requests: Arc<RwLock<BTreeMap<String, Value>>>,
    compact_receipt: Arc<Mutex<Option<oneshot::Sender<Result<()>>>>>,
    config: RwLock<Value>,
    output_schema: Option<Value>,
    policy_observation: RwLock<Option<PolicyObservation>>,
    cwd: PathBuf,
}
#[async_trait]
impl AgentBackend for CodexBackend {
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        self.submit_native(None, p).await
    }
    async fn submit(&self, submission_id: &str, p: &Prompt) -> Result<()> {
        self.submit_native(Some(submission_id), p).await
    }
    async fn reconcile_submission(
        &self,
        submission_id: &str,
    ) -> Result<Option<NativeSubmissionEvidence>> {
        if self.ephemeral {
            return Err(Error::Unsupported(
                "ephemeral native submission history".into(),
            ));
        }
        let mut cursor = Value::Null;
        let mut cursors = std::collections::BTreeSet::new();
        let mut turn_ids = std::collections::BTreeSet::new();
        let mut found = None;
        for _ in 0..100 {
            let page = self
                .wire
                .request(
                    "thread/turns/list",
                    json!({"threadId":self.thread,
                "sortDirection":"desc","itemsView":"full","limit":100,"cursor":cursor}),
                )
                .await?;
            let turns = page["data"]
                .as_array()
                .ok_or_else(|| Error::Provider("native turn page omitted its data".into()))?;
            for turn in turns {
                let id = turn["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| Error::Provider("native turn omitted its identity".into()))?;
                if !turn_ids.insert(id.to_string()) {
                    return Err(Error::Provider(
                        "native turn pagination repeated a turn".into(),
                    ));
                }
                let items = turn["items"]
                    .as_array()
                    .ok_or_else(|| Error::Provider("native turn omitted full items".into()))?;
                let matches = items
                    .iter()
                    .filter(|item| {
                        item["type"] == "userMessage" && item["clientId"] == submission_id
                    })
                    .count();
                if matches > 1 || matches == 1 && found.is_some() {
                    return Err(Error::Provider(
                        "ambiguous native submission identity".into(),
                    ));
                }
                if matches == 1 {
                    found = Some(turn.clone());
                }
            }
            let next = page.get("nextCursor").ok_or_else(|| {
                Error::Provider("native turn page omitted its next cursor".into())
            })?;
            if next.is_null() {
                return found
                    .map(|turn| submission_evidence(submission_id, turn))
                    .transpose();
            }
            let next = next
                .as_str()
                .filter(|next| !next.is_empty())
                .ok_or_else(|| Error::Provider("invalid native turn cursor".into()))?;
            if !cursors.insert(next.to_string()) {
                return Err(Error::Provider(
                    "native turn pagination did not advance".into(),
                ));
            }
            cursor = json!(next);
        }
        Err(Error::Provider(
            "native submission lookup exceeds 10,000 turns".into(),
        ))
    }
    async fn steer(&self, p: &Prompt) -> Result<()> {
        let id = self
            .turn
            .read()
            .await
            .clone()
            .ok_or_else(|| Error::Invalid("no native turn".into()))?;
        self.wire
            .request(
                "turn/steer",
                json!({"threadId":self.thread,"expectedTurnId":id,"input":input(p)?}),
            )
            .await?;
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        if let Some(id) = self.turn.read().await.clone() {
            self.cancelled.store(true, Ordering::SeqCst);
            if let Err(error) = self
                .wire
                .request(
                    "turn/interrupt",
                    json!({"threadId":self.thread,"turnId":id}),
                )
                .await
            {
                self.cancelled.store(false, Ordering::SeqCst);
                return Err(error);
            }
        }
        Ok(())
    }
    async fn respond_permission(&self, r: &PermissionResponse) -> Result<()> {
        let mut requests = self.requests.write().await;
        let f = requests
            .get(&r.id)
            .ok_or_else(|| Error::Invalid("unknown native request".into()))?;
        let method = f["method"].as_str().unwrap_or("");
        let answer = if method == "item/tool/requestUserInput" {
            if r.option == "answer" {
                let mut answers = serde_json::Map::new();
                if let Some(values) = r.data["answers"].as_object() {
                    for (id, value) in values {
                        let entries = if value.is_array() {
                            value.clone()
                        } else {
                            json!([value])
                        };
                        answers.insert(id.clone(), json!({"answers":entries}));
                    }
                }
                json!({"answers":answers})
            } else {
                json!({"answers":{}})
            }
        } else if method == "item/permissions/requestApproval" {
            json!({"permissions":if r.option == "approve" { f["params"]["permissions"].clone() } else { json!({}) },"scope":"turn"})
        } else {
            let decision = match r.option.as_str() {
                "approve" => {
                    if method == "execCommandApproval" || method == "applyPatchApproval" {
                        "approved"
                    } else {
                        "accept"
                    }
                }
                "deny" => {
                    if method == "execCommandApproval" || method == "applyPatchApproval" {
                        "denied"
                    } else {
                        "decline"
                    }
                }
                _ => {
                    if method == "execCommandApproval" || method == "applyPatchApproval" {
                        "abort"
                    } else {
                        "cancel"
                    }
                }
            };
            json!({"decision":decision})
        };
        self.wire.respond(f["id"].clone(), answer).await?;
        requests.remove(&r.id);
        Ok(())
    }
    async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        settings::model_descriptors(settings::model_choices(
            &self.wire.request("model/list", json!({})).await?,
        ))
    }
    async fn configure(&self, patch: Value) -> Result<Value> {
        let fields = patch
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        let models = self.wire.request("model/list", json!({})).await?;
        let mut current = self.config.write().await;
        let mut candidate = current.clone();
        for (key, value) in fields {
            candidate[key] = value.clone();
        }
        let descriptors = configuration_for(&models, &candidate);
        settings::validate(&patch, &descriptors)?;
        if fields.contains_key("model")
            && let Some(model) = selected_model(&models, &candidate)
        {
            if !fields.contains_key("effort")
                && !model["supportedReasoningEfforts"]
                    .as_array()
                    .is_some_and(|a| {
                        a.iter()
                            .any(|e| e["reasoningEffort"] == candidate["effort"])
                    })
            {
                candidate["effort"] = model["defaultReasoningEffort"].clone();
            }
            if !fields.contains_key("serviceTier")
                && !tier_choices(model)
                    .as_array()
                    .is_some_and(|a| a.iter().any(|e| e["id"] == candidate["serviceTier"]))
            {
                candidate["serviceTier"] = json!("default");
            }
        }
        if let Some(fast) = fields.get("fast").and_then(Value::as_bool) {
            let model = selected_model(&models, &candidate)
                .ok_or_else(|| Error::Invalid("model not advertised".into()))?;
            let tier = tier_choices(model)
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|e| e["id"] == "priority" || e["id"] == "fast")
                })
                .cloned()
                .ok_or_else(|| Error::Unsupported("fast mode for this model".into()))?;
            candidate["serviceTier"] = if fast {
                tier["id"].clone()
            } else {
                json!("default")
            };
        }
        candidate["fast"] =
            json!(candidate["serviceTier"] == "priority" || candidate["serviceTier"] == "fast");
        *current = candidate;
        Ok(current.clone())
    }
    async fn execution_policy(&self) -> Result<ExecutionPolicyReport> {
        Ok(super::policy::codex_report(
            &*self.config.read().await,
            self.policy_observation.read().await.clone(),
            &self.cwd,
        ))
    }
    async fn compile_execution_profile(
        &self,
        profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        self.execution_policy().await?.require(profile)?;
        super::policy::codex_plan(profile)
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let models = self.wire.request("model/list", json!({})).await?;
        settings::descriptors(configuration_for(&models, &*self.config.read().await))
    }
    async fn commands(&self) -> Result<Value> {
        let response = self
            .wire
            .request(
                "skills/list",
                json!({"cwds":[self.cwd],"forceReload":false}),
            )
            .await?;
        let commands:Vec<_> = response["data"].as_array().into_iter().flatten().flat_map(|entry|entry["skills"].as_array().into_iter().flatten()).filter(|skill|skill["enabled"] != false).filter_map(|skill| {
            Some(json!({"name":skill["name"].as_str()?,"description":skill["description"],"source":"skill","path":skill["path"],"invocation":format!("${} ",skill["name"].as_str()?)}))
        }).collect();
        Ok(json!(commands))
    }
    async fn quotas(&self) -> Result<Value> {
        let result = self
            .wire
            .request("account/rateLimits/read", json!({}))
            .await?;
        let normalized = normalize_quotas(&result);
        if normalized["windows"]
            .as_array()
            .is_none_or(|a| a.is_empty())
        {
            return Err(Error::Unsupported("account quotas for this login".into()));
        }
        Ok(normalized)
    }
    async fn rollback(&self, turns: u64) -> Result<()> {
        if self.ephemeral {
            return Err(Error::Unsupported(
                "rollback of an ephemeral Codex session".into(),
            ));
        }
        if turns == 0 || turns > 10_000 {
            return Err(Error::Invalid(
                "rollback count must be between 1 and 10,000 turns".into(),
            ));
        }
        if self.turn.read().await.is_some() {
            return Err(Error::Busy);
        }
        let mut remaining = turns;
        let mut cursor = Value::Null;
        let mut cursors = std::collections::BTreeSet::new();
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..100 {
            let page = self.wire.request(
                "thread/turns/list",
                json!({"threadId":self.thread,"sortDirection":"desc", "itemsView":"notLoaded","limit":100,"cursor":cursor}),
            ).await?;
            let records = page["data"]
                .as_array()
                .ok_or_else(|| Error::Provider("Codex turn list omitted its data array".into()))?;
            for turn in records {
                match turn["status"].as_str() {
                    Some("inProgress") => return Err(Error::Busy),
                    Some("completed" | "failed" | "interrupted") => (),
                    _ => return Err(Error::Provider("Codex turn has an unknown status".into())),
                }
                let id = turn["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| {
                        Error::Provider("Codex rollback anchor has no native turn ID".into())
                    })?;
                if !ids.insert(id.to_owned()) {
                    return Err(Error::Provider("Codex turn list repeated a turn".into()));
                }
                remaining -= 1;
                if remaining == 0 {
                    self.wire
                        .request(
                            "thread/revert",
                            json!({"threadId":self.thread,"beforeTurnId":id}),
                        )
                        .await?;
                    return Ok(());
                }
            }
            let next = page["nextCursor"].clone();
            match next.as_str() {
                Some(next) if !next.is_empty() && cursors.insert(next.to_owned()) => {
                    cursor = json!(next)
                }
                None if next.is_null() => {
                    return Err(Error::Invalid(
                        "rollback exceeds available native turns".into(),
                    ));
                }
                _ => {
                    return Err(Error::Provider(
                        "Codex turn pagination did not advance".into(),
                    ));
                }
            }
        }
        Err(Error::Invalid(
            "native rollback lookup exceeds 100 pages".into(),
        ))
    }
    async fn compact(&self) -> Result<()> {
        if self.turn.read().await.is_some() {
            return Err(Error::Busy);
        }
        let (receipt, completion) = oneshot::channel();
        {
            let mut pending = self.compact_receipt.lock().await;
            if pending.is_some() {
                return Err(Error::Busy);
            }
            *pending = Some(receipt);
        }
        if let Err(error) = self
            .wire
            .request("thread/compact/start", json!({"threadId":self.thread}))
            .await
        {
            self.compact_receipt.lock().await.take();
            return Err(error);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(60), completion).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::Provider(
                "Codex compaction receipt channel closed".into(),
            )),
            Err(_) => {
                self.compact_receipt.lock().await.take();
                // A late receipt must never be mistaken for a subsequent operation.
                let _ = self.wire.kill().await;
                Err(Error::Provider(
                    "Codex compaction completion was not confirmed within 60 seconds".into(),
                ))
            }
        }
    }
    async fn close(&self) -> Result<()> {
        self.wire.close().await
    }
    async fn kill(&self) -> Result<()> {
        self.wire.kill().await
    }
}

impl CodexBackend {
    async fn submit_native(&self, submission_id: Option<&str>, p: &Prompt) -> Result<()> {
        // Reserve the native turn before sending. Notifications can precede the RPC receipt.
        let mut active = self.turn.write().await;
        if active.is_some() || self.compact_receipt.lock().await.is_some() {
            return Err(Error::Busy);
        }
        let mut params = json!({"threadId":self.thread,"input":input(p)?});
        if let Some(id) = submission_id {
            params["clientUserMessageId"] = json!(id);
        }
        if let Some(schema) = &self.output_schema {
            params["outputSchema"] = schema.clone();
        }
        if let Some(settings) = self.config.read().await.as_object() {
            for (key, value) in settings {
                if key == "sandbox" && value.is_null() {
                    continue;
                }
                if key == "sandbox" {
                    params["sandboxPolicy"] = sandbox_policy(value, &self.cwd)?;
                } else if key != "fast" && !value.is_null() {
                    params[key] = value.clone();
                }
            }
        }
        self.cancelled.store(false, Ordering::SeqCst);
        self.wire.set_active(true);
        let r = match self.wire.request("turn/start", params).await {
            Ok(value) => value,
            Err(error) => {
                self.wire.set_active(false);
                return Err(error);
            }
        };
        let id = r["turn"]["id"]
            .as_str()
            .ok_or_else(|| Error::Provider("missing turn id".into()))?;
        *active = Some(id.into());
        let mut observation = self.policy_observation.write().await;
        let version = observation
            .as_ref()
            .and_then(|observation| observation.native_version.clone());
        *observation = Some(super::policy::codex_observation(
            super::policy::codex_values(&*self.config.read().await),
            PolicyEvidence::NativeAcknowledgement,
            version,
            &self.cwd,
        ));
        Ok(())
    }
}

fn submission_evidence(submission_id: &str, turn: Value) -> Result<NativeSubmissionEvidence> {
    // History views can synthesize Interrupted for an abandoned InProgress turn.
    // Without a native completion timestamp that is acceptance evidence only.
    let terminal = matches!(turn["status"].as_str(), Some("completed" | "failed"))
        || turn["status"] == "interrupted"
            && turn["completedAt"].as_i64().is_some_and(|time| time > 0);
    let messages = if terminal {
        turn["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "agentMessage")
            .map(|item| {
                if item["id"].as_str().is_none_or(|id| id.is_empty()) {
                    return Err(Error::Provider(
                        "recovered native message omitted its identity".into(),
                    ));
                }
                Ok(item.clone())
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![]
    };
    Ok(NativeSubmissionEvidence {
        submission_id: submission_id.into(),
        native_turn_id: turn["id"].as_str().unwrap().into(),
        outcome: terminal.then(|| outcome::codex(&turn, false)),
        native_reason: turn["status"].as_str().map(String::from),
        messages,
        raw: turn,
    })
}
