use super::*;
use async_trait::async_trait;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

pub struct ClaudeHarness;
fn sdk_node(options: &SpawnOptions) -> Result<PathBuf> {
    let requested = options.config["sdkNodeExecutable"]
        .as_str()
        .map(String::from)
        .or_else(|| std::env::var("SWARMIE_NODE_EXECUTABLE").ok())
        .unwrap_or_else(|| "node".into());
    process::find_executable(&requested).ok_or_else(|| {
        Error::Provider("Claude Agent SDK requires Node.js 22 or later on this runtime node".into())
    })
}
fn sdk_worker(options: &SpawnOptions) -> Result<PathBuf> {
    if let Some(path) = options.config["sdkWorkerPath"]
        .as_str()
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SWARMIE_CLAUDE_SDK_WORKER").map(PathBuf::from))
    {
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| Error::Provider("Configured Claude SDK worker does not exist".into()));
    }
    let mut candidates = vec![];
    if let Some(root) = super::agent_tools_root() {
        candidates.push(root.join("claude/worker.mjs"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/claude-agent-worker/worker.mjs"),
    );
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            Error::Provider(
                "Claude Agent SDK worker is missing; install the runtime agent tools bundle".into(),
            )
        })
}
pub(crate) async fn sdk_readiness(options: &SpawnOptions) -> Result<()> {
    let node = sdk_node(options)?;
    let worker = sdk_worker(options)?;
    let package = worker
        .parent()
        .ok_or_else(|| Error::Invalid("SDK worker path has no parent".into()))?;
    if [
        "session.mjs",
        "diagnostics.mjs",
        "transport.mjs",
        "tasks.mjs",
        "input-queue.mjs",
    ]
    .iter()
    .any(|file| !package.join(file).is_file())
        || !package
            .join("node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs")
            .is_file()
    {
        return Err(Error::Provider(
            "Claude Agent SDK dependencies are missing; install the runtime agent tools bundle"
                .into(),
        ));
    }
    let mut command = tokio::process::Command::new(node);
    command.arg("--version").kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(3), command.output())
        .await
        .map_err(|_| Error::Timeout("Node.js version probe".into()))??;
    let major = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok());
    if !output.status.success() || major.is_none_or(|major| major < 22) {
        return Err(Error::Provider(
            "Claude Agent SDK requires Node.js 22 or later on this runtime node".into(),
        ));
    }
    Ok(())
}
fn configuration_for(init: &Value, config: &Value) -> Value {
    let mut fields = vec![
        settings::select(
            "model",
            "Model",
            config["model"].clone(),
            settings::model_choices(&init["models"]),
        ),
        settings::select(
            "permissionMode",
            "Permissions",
            config["permissionMode"].clone(),
            settings::choices(&[
                "default",
                "acceptEdits",
                "plan",
                "dontAsk",
                "bypassPermissions",
            ]),
        ),
    ];
    if let Some(model) = init["models"]
        .as_array()
        .and_then(|a| a.iter().find(|m| m["value"] == config["model"]))
    {
        if let Some(levels) = model["supportedEffortLevels"].as_array() {
            fields.push(settings::select(
                "effort",
                "Thinking",
                config["effort"].clone(),
                json!(
                    levels
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|id| json!({"id":id,"label":id}))
                        .collect::<Vec<_>>()
                ),
            ));
        }
        if model["supportsFastMode"] == true {
            fields.push(json!({"id":"fast","label":"Fast mode","type":"boolean","value":config["fast"],"application":"next_turn"}));
        }
    }
    json!(fields)
}

fn mcp_servers(servers: &[McpServer]) -> Value {
    let entries: serde_json::Map<String, Value> = servers
        .iter()
        .map(|server| {
            let transport = match &server.transport {
                McpTransport::Stdio { command, args, env } => {
                    json!({"command":command,"args":args,"env":env})
                }
                McpTransport::Http { url, headers } => {
                    json!({"type":"http","url":url,"headers":headers})
                }
                McpTransport::Sse { url, headers } => {
                    json!({"type":"sse","url":url,"headers":headers})
                }
            };
            (server.name.clone(), transport)
        })
        .collect();
    json!(entries)
}
fn result_error(frame: &Value) -> String {
    if let Some(message) = frame["result"]
        .as_str()
        .filter(|message| !message.trim().is_empty())
    {
        return message.into();
    }
    if let Some(errors) = frame["errors"].as_array() {
        let messages: Vec<_> = errors
            .iter()
            .filter_map(Value::as_str)
            .filter(|message| !message.trim().is_empty())
            .collect();
        if !messages.is_empty() {
            return messages.join("\n");
        }
    }
    if let Some(message) = frame["errors"]
        .as_str()
        .filter(|message| !message.trim().is_empty())
    {
        return message.into();
    }
    "Claude failed without an error message. Check its authentication and provider configuration."
        .into()
}
fn unix_time(value: &Value) -> Option<i64> {
    time::OffsetDateTime::parse(
        value.as_str()?,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()
    .map(|date| date.unix_timestamp())
}
fn quota_windows(receipt: &Value) -> Result<Value> {
    let limits = &receipt["rate_limits"];
    let mut windows = vec![];
    if let Some(entries) = limits["limits"].as_array() {
        for (index, limit) in entries.iter().enumerate() {
            let Some(used) = limit["percent"]
                .as_f64()
                .filter(|number| number.is_finite())
            else {
                continue;
            };
            let used = used.clamp(0.0, 100.0);
            windows.push(json!({"id":limit["id"].as_str().or(limit["kind"].as_str()).map(String::from).unwrap_or_else(||index.to_string()),"label":limit["group"].as_str().or(limit["kind"].as_str()),"usedPercent":used,"remainingPercent":100.0-used,"resetsAt":unix_time(&limit["resets_at"]),"metadata":limit}));
        }
    } else {
        for (id, label) in [("five_hour", "Session"), ("seven_day", "Week")] {
            let window = &limits[id];
            let Some(used) = window["utilization"]
                .as_f64()
                .filter(|number| number.is_finite())
            else {
                continue;
            };
            let used = used.clamp(0.0, 100.0);
            windows.push(json!({"id":id,"label":label,"usedPercent":used,"remainingPercent":100.0-used,"resetsAt":unix_time(&window["resets_at"]),"metadata":window}));
        }
    }
    if windows.is_empty() {
        return Err(Error::Unsupported(
            "Claude account quotas for this login".into(),
        ));
    }
    let credit = &limits["cedar_ember"];
    let reset_credits = if credit["eligible"] == true {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let grants: Vec<_> = credit["grants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|grant| {
                grant["paused"] != true
                    && grant["usable_now"] == true
                    && (grant["ends_at"].is_null()
                        || unix_time(&grant["ends_at"]).is_some_and(|end| end > now))
            })
            .collect();
        let next = credit["next_grant_id"]
            .as_str()
            .and_then(|id| grants.iter().find(|grant| grant["id"] == id));
        json!({"available":next.map(|_|grants.iter().filter_map(|grant|grant["resets_left"].as_u64()).fold(0u64,u64::saturating_add)).unwrap_or(0),"nextExpiresAt":next.and_then(|grant|unix_time(&grant["ends_at"])),"metadata":credit})
    } else {
        Value::Null
    };
    Ok(
        json!({"provider":"claude","plan":receipt["subscription_type"],"windows":windows,"resetCredits":reset_credits}),
    )
}

fn capabilities() -> Capabilities {
    Capabilities {
        steering: true.into(),
        commands: true.into(),
        quotas: true.into(),
        ephemeral: true.into(),
        output_schema: true.into(),
        tools_disabled: true.into(),
        fork_at_message: true.into(),
        background_tasks: true.into(),

        resume: true.into(),
        fork: true.into(),
        cancel: true.into(),
        permissions: true.into(),
        operation_approvals: true.into(),
        reasoning: true.into(),
        usage: true.into(),
        models: true.into(),
        configure: true.into(),
        images: true.into(),
        rollback: true.into(),
        compact: true.into(),
        metadata: BTreeMap::from([("mcpTransports".into(), json!(["stdio", "http", "sse"]))]),
        ..Default::default()
    }
}
#[async_trait]
impl Harness for ClaudeHarness {
    fn id(&self) -> &str {
        "claude"
    }
    fn compile_execution_profile(
        &self,
        _options: &SpawnOptions,
        profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        super::policy::claude_plan(profile)
    }
    async fn detect(&self) -> Result<Detection> {
        let mut detected = detection("claude", "claude", capabilities());
        if detected.available {
            let options = SpawnOptions {
                project_id: None,
                project_revision: None,
                harness: "claude".into(),
                cwd: PathBuf::from("."),
                model: None,
                execution: Default::default(),
                config: json!({}),
            };
            if let Err(error) = sdk_readiness(&options).await {
                detected.available = false;
                detected
                    .capabilities
                    .metadata
                    .insert("sdkError".into(), json!(error.to_string()));
            }
        }
        Ok(detected)
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        let detected = self.detect().await?;
        let sdk_error = detected
            .capabilities
            .metadata
            .get("sdkError")
            .and_then(Value::as_str)
            .map(String::from);
        let mut probe = readiness::probe(detected, Some(&["auth", "status", "--json"])).await?;
        if let Some(error) = sdk_error {
            probe.message = Some(error);
        }
        Ok(probe)
    }
    async fn account_quotas(&self, query: &AccountQuery) -> Result<Value> {
        let mut options = query.connection_options();
        options.execution.ephemeral = true;
        options.execution.tools = ToolPolicy::Disabled;
        let launched = launch_connection(&options, None, false).await?;
        let wire = launched.wire;
        let mut frames = launched.frames;
        let drain = tokio::spawn(async move { while frames.recv().await.is_some() {} });
        let result = match wire.request("get_usage", json!({})).await {
            Ok(receipt) => quota_windows(&receipt),
            Err(error) => Err(error),
        };
        let closed = close_worker(&wire).await;
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
        uuid::Uuid::parse_str(message_id)
            .map_err(|_| Error::Invalid("Claude fork requires the native assistant UUID".into()))?;
        let mut options = options.clone();
        if !options.config.is_object() {
            options.config = json!({});
        }
        options.config["resumeAtMessage"] = json!(message_id);
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
        o: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        let launched = launch_connection(o, saved, fork).await?;
        let requests: Arc<RwLock<BTreeMap<String, Value>>> = Arc::default();
        let config = Arc::new(RwLock::new(
            json!({"model":o.model,"permissionMode":o.config["permissionMode"].as_str().unwrap_or("default"),"effort":o.config["effort"],"fast":o.config["fast"]}),
        ));
        let policy_observation = Arc::new(RwLock::new(
            launched
                .init
                .get("permissionMode")
                .filter(|mode| mode.is_string())
                .map(|mode| {
                    super::policy::claude_observation(
                        mode.clone(),
                        PolicyEvidence::NativeReceipt,
                        launched.init["version"].as_str().map(String::from),
                    )
                }),
        ));
        let commands = Arc::new(RwLock::new(
            launched.skills.catalog(&launched.init["commands"]),
        ));
        let history: History = Arc::default();
        let compaction: Compaction = Arc::default();
        let (tx, events) = mpsc::channel(512);
        let id = launched.id.clone();
        let connection = attach_decoder(
            launched,
            requests.clone(),
            config.clone(),
            commands.clone(),
            history.clone(),
            compaction.clone(),
            tx.clone(),
            o.execution.output_schema.is_some(),
            policy_observation.clone(),
        );
        let mut capabilities = capabilities();
        capabilities
            .metadata
            .insert("maxNativeSkillsPerMessage".into(), json!(1));
        capabilities.native_skill_inputs = if o.execution.tools == ToolPolicy::Disabled {
            Capability::unavailable("Skills are disabled by execution policy")
        } else {
            true.into()
        };
        if o.config.get("nativeHistory").is_some_and(Value::is_object) {
            capabilities
                .metadata
                .insert("rollbackHistorySupported".into(), json!(false));
        }
        if o.execution.ephemeral {
            capabilities.resume =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities.fork =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities.rollback =
                Capability::unavailable("Ephemeral sessions do not retain native history");
            capabilities.fork_at_message =
                Capability::unavailable("Ephemeral sessions do not retain native history");
        }
        capabilities.metadata.insert("rollbackHistory".into(), json!("completed turns observed by this live connection after its latest native compaction; earlier resumed or compacted history cannot be rolled back"));
        let file_rewind = !o
            .execution
            .env
            .get("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING")
            .is_some_and(|value| value != "true" && value != "1");
        capabilities.metadata.insert(
            "rollbackScopes".into(),
            if o.execution.ephemeral {
                json!([])
            } else if file_rewind {
                json!(["conversation_only", "conversation_and_files"])
            } else {
                json!(["conversation_only"])
            },
        );
        Ok(BackendConnection {
            native: native("claude", id),
            capabilities,
            events,
            backend: Box::new(ClaudeBackend {
                current: RwLock::new(connection),
                options: o.clone(),
                requests,
                config,
                policy_observation,
                commands,
                history,
                compaction,
                events: tx,
                operation: tokio::sync::Mutex::new(()),
            }),
        })
    }
}

struct Launched {
    wire: JsonProcess,
    frames: mpsc::Receiver<Value>,
    id: String,
    init: Value,
    skills: Arc<super::skills::ClaudeSkills>,
}
async fn launch_connection(
    o: &SpawnOptions,
    saved: Option<&NativeSessionRef>,
    fork: bool,
) -> Result<Launched> {
    let id = if fork {
        uuid::Uuid::new_v4().to_string()
    } else {
        saved
            .map(|s| s.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    };
    let (cli, mut cli_args) = launch(o, "claude", vec![]);
    let cli = process::find_executable(&cli.to_string_lossy()).ok_or_else(|| {
        Error::Provider("Claude Code executable is unavailable on this runtime node".into())
    })?;
    let node = sdk_node(o)?;
    let worker = sdk_worker(o)?;
    if o.execution.tools == ToolPolicy::Disabled
        && (!o.execution.mcp_servers.is_empty()
            || cli_args.iter().any(|arg| {
                matches!(
                    arg.split('=').next().unwrap_or_default(),
                    "--mcp-config" | "--plugin-dir" | "--plugin-url" | "--tools"
                )
            }))
    {
        return Err(Error::Invalid(
            "disabled tools cannot be combined with explicit tool or MCP arguments".into(),
        ));
    }
    if o.execution.ephemeral && saved.is_some() {
        return Err(Error::Unsupported(
            "resume an ephemeral Claude session".into(),
        ));
    }
    if o.config["resumeAtMessage"].is_string() && !fork {
        return Err(Error::Invalid("resumeAtMessage requires a fork".into()));
    }
    let skills = Arc::new(super::skills::ClaudeSkills::prepare(o, &mut cli_args)?);
    let mut env = execution_env(o, Some("CLAUDE_CONFIG_DIR"))?;
    env.entry("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING".into())
        .or_insert_with(|| "true".into());
    let file_checkpointing = env
        .get("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING")
        .is_none_or(|value| value == "true" || value == "1");
    let mut sdk_options = json!({
        "cwd":o.cwd, "includePartialMessages":true,
        "persistSession":!o.execution.ephemeral,
        "enableFileCheckpointing":file_checkpointing,
        "permissionMode":o.config["permissionMode"].as_str().unwrap_or("default"),
        "allowDangerouslySkipPermissions":o.config["permissionMode"] == "bypassPermissions",
        "settingSources":["user","project","local"],
        "systemPrompt":{"type":"preset","preset":"claude_code"},
        "extraArgs":{"replay-user-messages":null},
    });
    if let Some(model) = &o.model {
        sdk_options["model"] = json!(model);
    }
    if let Some(saved) = saved {
        sdk_options["resume"] = json!(saved.id);
        if fork {
            sdk_options["forkSession"] = json!(true);
            sdk_options["sessionId"] = json!(id);
        }
    } else {
        sdk_options["sessionId"] = json!(id);
    }
    if let Some(anchor) = o.config["resumeAtMessage"].as_str() {
        sdk_options["resumeSessionAt"] = json!(anchor);
    }
    if o.execution.tools == ToolPolicy::Disabled {
        sdk_options["tools"] = json!([]);
        sdk_options["strictMcpConfig"] = json!(true);
        sdk_options["settingSources"] = json!([]);
        sdk_options["extraArgs"]["disable-slash-commands"] = Value::Null;
        sdk_options["extraArgs"]["bare"] = Value::Null;
    }
    if let Some(schema) = &o.execution.output_schema {
        sdk_options["outputFormat"] = json!({"type":"json_schema","schema":schema});
    }
    let (wire, frames) = spawn_json(
        o,
        &node,
        &[worker.to_string_lossy().into_owned()],
        WireStyle::JsonRpc,
        env,
    )
    .await?;
    let init = wire
        .request(
            "initialize",
            json!({"cliExecutable":cli,"cliArgs":cli_args,"options":sdk_options}),
        )
        .await?;
    skills.bind_native(init["pid"].as_u64())?;
    if !o.execution.mcp_servers.is_empty() {
        let receipt = wire
            .request(
                "mcp_set_servers",
                json!({"servers":mcp_servers(&o.execution.mcp_servers)}),
            )
            .await?;
        if receipt["errors"]
            .as_object()
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(Error::Provider(
                "one or more declared MCP servers failed to connect".into(),
            ));
        }
    }
    let startup_config = json!({"model":o.model,"permissionMode":o.config["permissionMode"],"effort":o.config["effort"],"fast":o.config["fast"]});
    let mut advanced = json!({});
    for key in ["effort", "fast"] {
        if let Some(value) = o.config.get(key).filter(|v| !v.is_null()) {
            advanced[key] = value.clone();
        }
    }
    settings::validate(&advanced, &configuration_for(&init, &startup_config))?;
    let mut startup_settings = json!({});
    if let Some(effort) = o.config.get("effort").filter(|v| !v.is_null()) {
        startup_settings["effortLevel"] = effort.clone();
    }
    if let Some(fast) = o.config.get("fast").filter(|v| !v.is_null()) {
        startup_settings["fastMode"] = fast.clone();
    }
    if startup_settings.as_object().is_some_and(|m| !m.is_empty()) {
        wire.request("apply_flag_settings", json!({"settings":startup_settings}))
            .await?;
    }
    Ok(Launched {
        wire,
        frames,
        id,
        init,
        skills,
    })
}

struct LiveConnection {
    wire: JsonProcess,
    id: Arc<RwLock<String>>,
    init: Value,
    decoder: tokio::task::AbortHandle,
    skills: Arc<super::skills::ClaudeSkills>,
}
impl Drop for LiveConnection {
    fn drop(&mut self) {
        self.decoder.abort();
    }
}
#[derive(Default)]
struct TurnHistory {
    completed: Vec<TurnAnchors>,
    user: Option<String>,
    assistant: Option<String>,
    cancelled: bool,
}
struct TurnAnchors {
    user: Option<String>,
    assistant: Option<String>,
}
struct CompactPending {
    reply: tokio::sync::oneshot::Sender<Result<()>>,
    refusal: Option<String>,
}
type History = Arc<std::sync::Mutex<TurnHistory>>;
type Compaction = Arc<std::sync::Mutex<Option<CompactPending>>>;
#[allow(clippy::too_many_arguments)]
fn attach_decoder(
    launched: Launched,
    requests: Arc<RwLock<BTreeMap<String, Value>>>,
    config: Arc<RwLock<Value>>,
    commands: Arc<RwLock<Value>>,
    history: History,
    compaction: Compaction,
    tx: mpsc::Sender<BackendEvent>,
    schema_enabled: bool,
    policy_observation: Arc<RwLock<Option<PolicyObservation>>>,
) -> LiveConnection {
    let Launched {
        wire,
        frames,
        id,
        init,
        skills,
    } = launched;
    let id = Arc::new(RwLock::new(id));
    let decoder = decode(
        wire.clone(),
        frames,
        id.clone(),
        requests,
        config,
        commands,
        history,
        compaction,
        tx,
        schema_enabled,
        policy_observation,
        skills.clone(),
    )
    .abort_handle();
    LiveConnection {
        wire,
        id,
        init,
        decoder,
        skills,
    }
}
#[allow(clippy::too_many_arguments)]
fn decode(
    wire_task: JsonProcess,
    mut frames: mpsc::Receiver<Value>,
    native_id: Arc<RwLock<String>>,
    req_task: Arc<RwLock<BTreeMap<String, Value>>>,
    config_task: Arc<RwLock<Value>>,
    commands: Arc<RwLock<Value>>,
    history: History,
    compaction: Compaction,
    tx: mpsc::Sender<BackendEvent>,
    schema_enabled: bool,
    policy_observation: Arc<RwLock<Option<PolicyObservation>>>,
    skills: Arc<super::skills::ClaudeSkills>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut message_id = None;
        let mut blocks = BTreeMap::<u64, String>::new();
        let mut inputs = BTreeMap::<u64, String>::new();
        let mut calls = BTreeMap::<String, ToolEvent>::new();
        let mut denied = std::collections::BTreeSet::new();
        let mut context_tokens = None;
        while let Some(f) = frames.recv().await {
            if (f.get("_runtime_exit").is_some() || f.get("_runtime_error").is_some())
                && let Some(pending) = compaction.lock().unwrap().take()
            {
                let _ = pending.reply.send(Err(Error::Closed));
            }
            if terminal_frame(&f, &tx).await {
                break;
            }
            if f["type"] == "sdk_task" {
                match serde_json::from_value::<AgentTask>(f["task"].clone()) {
                    Ok(task) => {
                        let _ = tx
                            .send(BackendEvent::content(EventKind::TaskUpdated(task), f))
                            .await;
                    }
                    Err(_) => {
                        let _ = tx.send(BackendEvent::content(EventKind::Diagnostic(json!({"code":"invalid_task_update","level":"warning","message":"Claude SDK task update could not be decoded"})), f)).await;
                    }
                }
                continue;
            }
            if f["type"] == "sdk_permission" {
                if compaction.lock().unwrap().is_some() {
                    let _=wire_task.request("permission", json!({"id":f["request_id"],"response":{"behavior":"deny","message":"Compaction cannot open interactive tool requests"}})).await;
                    continue;
                }
                if f["request"]["tool_name"].is_string() {
                    let r = &f["request"];
                    let id = string(&f, "request_id");
                    req_task.write().await.insert(id.clone(), f.clone());
                    let p = PermissionRequest {
                        questions: Vec::new(),
                        id,
                        kind: if r["tool_name"] == "AskUserQuestion" {
                            "question"
                        } else if r["tool_name"] == "ExitPlanMode" {
                            "plan"
                        } else {
                            "tool"
                        }
                        .into(),
                        title: if r["tool_name"] == "ExitPlanMode" {
                            "Review proposed plan".into()
                        } else {
                            format!("Allow {}", string(r, "tool_name"))
                        },
                        description: r["input"]["plan"]
                            .as_str()
                            .or(r["decision_reason"].as_str())
                            .map(String::from),
                        command: r.get("input").cloned(),
                        paths: vec![],
                        options: if r["tool_name"] == "AskUserQuestion" {
                            vec![
                                option("answer", "Submit answers", "answer"),
                                option("cancel", "Cancel", "cancel"),
                            ]
                        } else {
                            vec![
                                option("approve", "Approve once", "approve"),
                                option("deny", "Deny", "deny"),
                                option("cancel", "Cancel turn", "cancel"),
                            ]
                        },
                        metadata: if r["tool_name"] == "AskUserQuestion" {
                            json!({"questions":settings::questions(&r["input"]["questions"]),"native":r})
                        } else if r["tool_name"] == "ExitPlanMode" {
                            json!({"plan":{"markdown":r["input"]["plan"],"status":"proposed"},"native":r})
                        } else {
                            r.clone()
                        },
                    };
                    if r["tool_name"] == "ExitPlanMode" {
                        let _ = tx.send(BackendEvent::content(EventKind::PlanUpdated((json!({"markdown":r["input"]["plan"],"status":"proposed","requestId":p.id})).into()),f.clone())).await;
                    }
                    if tx
                        .send(BackendEvent::content(
                            EventKind::PermissionRequested(canonical_permission(p)),
                            f,
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                } else {
                    let _ = wire_task.request("permission", json!({"id":f["request_id"],"response":{"behavior":"deny","message":"Unsupported permission request"}})).await;
                }
                continue;
            }
            if f["type"] == "sdk_permission_cancel" {
                let id = string(&f, "request_id");
                if req_task.write().await.remove(&id).is_some() {
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::PermissionResolved {
                                id,
                                option: "expired".into(),
                            },
                            f,
                        ))
                        .await;
                }
                continue;
            }
            // A child's result is metadata for its parent tool, not the parent turn boundary.
            if let Some(parent) = f["parent_tool_use_id"].as_str()
                && f["type"] != "tool_progress"
            {
                let child = json!({"id":parent,"parentToolId":parent,"sessionId":f["session_id"],"model":f["model"],"usage":if f["usage"].is_object() {json!(usage(&f))} else {Value::Null},"nativeUsage":f["usage"],"status":if f["type"] == "result" {if f["is_error"] == true {"failed"} else {"completed"}} else {"running"},"message":f.get("message"),"textDelta":f["event"]["delta"]["text"]});
                if tx
                    .send(BackendEvent::content(
                        EventKind::SubagentUpdated((child).into()),
                        f,
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
            if f["type"] == "sdk_hook" {
                let _ = tx.send(BackendEvent::content(EventKind::Diagnostic(json!({"code":"claude_sdk_hook","level":"info","hook":f["hook"],"data":f["data"]})), f)).await;
                continue;
            }
            if let Some(diagnostic) = f.get("_runtime_diagnostic") {
                let _ = tx
                    .send(BackendEvent::content(
                        EventKind::Diagnostic(diagnostic.clone()),
                        f,
                    ))
                    .await;
                continue;
            }
            if f["type"] == "system" && f["subtype"] == "compact_boundary" {
                // Native compaction removes old UUID cut points from the resumable transcript.
                {
                    let mut history = history.lock().unwrap();
                    history.completed.clear();
                    history.assistant = None;
                }
                let _ = tx
                    .send(BackendEvent::content(
                        EventKind::SessionUpdated(
                            json!({"rollbackHistoryReset":"native_compaction"}),
                        ),
                        f.clone(),
                    ))
                    .await;
            }
            if compaction.lock().unwrap().is_some()
                && !(f["type"] == "system" && f["subtype"] == "init")
            {
                if let Some(error) = f["compact_error"].as_str()
                    && let Some(pending) = compaction.lock().unwrap().as_mut()
                {
                    pending.refusal = Some(error.into());
                }
                if f["type"] == "result" {
                    wire_task.set_active(false);
                    let pending = compaction.lock().unwrap().take();
                    if let Some(pending) = pending {
                        let result = if let Some(error) = pending.refusal {
                            Err(Error::Provider(format!(
                                "Claude compaction refused: {error}"
                            )))
                        } else if f["is_error"] == true {
                            Err(Error::Provider(result_error(&f)))
                        } else {
                            Ok(())
                        };
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::SessionUpdated(
                                    json!({"compaction":"finished","success":result.is_ok()}),
                                ),
                                f.clone(),
                            ))
                            .await;
                        let _ = pending.reply.send(result);
                    }
                } else {
                    let _ = tx.send(BackendEvent::content(EventKind::Raw, f)).await;
                }
                continue;
            }
            if f["type"] == "system" {
                match f["subtype"].as_str() {
                    Some("commands_changed") => {
                        let catalog = skills.catalog(&f["commands"]);
                        if !catalog.is_null() {
                            *commands.write().await = catalog.clone();
                            let _ = tx
                                .send(BackendEvent::content(
                                    EventKind::CommandsUpdated(catalog),
                                    f,
                                ))
                                .await;
                        } else {
                            let _ = tx.send(BackendEvent::content(EventKind::Diagnostic(json!({"code":"invalid_command_catalog","level":"warning","message":"Claude command update omitted its catalog"})), f)).await;
                        }
                        continue;
                    }
                    Some("status") if f["permissionMode"].is_string() => {
                        config_task.write().await["permissionMode"] = f["permissionMode"].clone();
                        *policy_observation.write().await =
                            Some(super::policy::claude_observation(
                                f["permissionMode"].clone(),
                                PolicyEvidence::NativeReceipt,
                                f["version"].as_str().map(String::from),
                            ));
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::ConfigurationUpdated(config_task.read().await.clone()),
                                f,
                            ))
                            .await;
                        continue;
                    }
                    Some("permission_denied") => {
                        if let Some(id) = f["tool_use_id"].as_str() {
                            denied.insert(id.to_string());
                            if let Some(mut call) = calls.remove(id) {
                                call.status = "denied".into();
                                call.output = f["message"].clone();
                                let _ = tx
                                    .send(BackendEvent::content(EventKind::ToolCompleted(call), f))
                                    .await;
                            } else {
                                let _ = tx.send(BackendEvent::content(EventKind::Raw, f)).await;
                            }
                        }
                        continue;
                    }
                    Some("task_started" | "task_progress" | "task_notification") => {
                        if let Some(id) = f["tool_use_id"].as_str()
                            && let Some(call) = calls.get_mut(id)
                        {
                            if !call.metadata.is_object() {
                                call.metadata = json!({});
                            }
                            call.metadata["taskId"] = f["task_id"].clone();
                            call.metadata["summary"] = f["summary"].clone();
                            call.metadata["usage"] = f["usage"].clone();
                            call.metadata["background"] = json!(true);
                            let finished = f["subtype"] == "task_notification";
                            if finished {
                                call.status = match f["status"].as_str() {
                                    Some("failed") => "failed",
                                    Some("stopped") => "cancelled",
                                    _ => "completed",
                                }
                                .into();
                                if !f["summary"].is_null() {
                                    call.output = f["summary"].clone();
                                }
                            }
                            call.details = Some(tool_details(&f, call));
                            let call = call.clone();
                            if matches!(call.name.as_deref(), Some("Agent" | "Task")) {
                                let _=tx.send(BackendEvent::content(EventKind::SubagentUpdated((json!({"id":id,"parentToolId":id,"role":call.input["subagent_type"],"model":call.input["model"],"summary":f["summary"].as_str().or(f["description"].as_str()),"usage":if f["usage"].is_object() {json!(usage(&f))} else {Value::Null},"nativeUsage":f["usage"],"status":call.status,"background":true})).into()),f.clone())).await;
                            }
                            let _ = tx
                                .send(BackendEvent::content(
                                    if finished {
                                        EventKind::ToolCompleted(call)
                                    } else {
                                        EventKind::ToolUpdated(call)
                                    },
                                    f,
                                ))
                                .await;
                        } else {
                            let _ = tx.send(BackendEvent::content(EventKind::Raw, f)).await;
                        }
                        continue;
                    }
                    _ => (),
                }
            }
            if f["type"] == "tool_progress" {
                let id = f["parent_tool_use_id"]
                    .as_str()
                    .or(f["tool_use_id"].as_str());
                if let Some(call) = id.and_then(|id| calls.get_mut(id)) {
                    if !call.metadata.is_object() {
                        call.metadata = json!({});
                    }
                    call.metadata["progress"] = json!({"elapsedMs":f["elapsed_time_seconds"].as_f64().map(|seconds|seconds*1000.0),"message":f["message"]});
                    call.details = Some(tool_details(&f, call));
                    let _ = tx
                        .send(BackendEvent::content(
                            EventKind::ToolUpdated(call.clone()),
                            f,
                        ))
                        .await;
                } else {
                    let _ = tx.send(BackendEvent::content(EventKind::Raw, f)).await;
                }
                continue;
            }
            let event = match f["type"].as_str().unwrap_or("") {
                "system" if f["subtype"] == "init" => {
                    if f["model"].is_string() {
                        config_task.write().await["model"] = f["model"].clone();
                    }
                    if let Some(s) = f["session_id"].as_str() {
                        let changed = {
                            let mut current = native_id.write().await;
                            if current.as_str() == s {
                                false
                            } else {
                                *current = s.into();
                                true
                            }
                        };
                        if changed {
                            let _ = tx
                                .send(BackendEvent::NativeSessionChanged(native(
                                    "claude",
                                    s.into(),
                                )))
                                .await;
                        }
                    }
                    BackendEvent::content(EventKind::SessionUpdated(f.clone()), f.clone())
                }
                "stream_event" => {
                    let e = &f["event"];
                    let kind = match e["type"].as_str().unwrap_or("") {
                        "message_start" => {
                            let usage = &e["message"]["usage"];
                            context_tokens = usage["input_tokens"].as_u64().map(|n| {
                                n + usage["cache_read_input_tokens"].as_u64().unwrap_or(0)
                                    + usage["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                            });
                            message_id = e["message"]["id"].as_str().map(String::from);
                            blocks.clear();
                            inputs.clear();
                            EventKind::Raw
                        }
                        "content_block_delta" if e["delta"]["type"] == "text_delta" => {
                            EventKind::TextDelta {
                                text: string(&e["delta"], "text"),
                                item_id: message_id.clone(),
                            }
                        }
                        "content_block_delta" if e["delta"]["type"] == "thinking_delta" => {
                            EventKind::ReasoningDelta {
                                text: string(&e["delta"], "thinking"),
                            }
                        }
                        "content_block_start" if e["content_block"]["type"] == "tool_use" => {
                            let call = tool(&e["content_block"], "running");
                            if let Some(index) = e["index"].as_u64() {
                                blocks.insert(index, call.id.clone());
                            }
                            calls.insert(call.id.clone(), call.clone());
                            if schema_enabled && call.name.as_deref() == Some("StructuredOutput") {
                                EventKind::Raw
                            } else {
                                EventKind::ToolStarted(call)
                            }
                        }
                        "content_block_delta" if e["delta"]["type"] == "input_json_delta" => {
                            if let Some(index) = e["index"].as_u64()
                                && let Some(id) = blocks.get(&index)
                                && let Some(call) = calls.get_mut(id)
                            {
                                let input = inputs.entry(index).or_default();
                                input.push_str(e["delta"]["partial_json"].as_str().unwrap_or(""));
                                if let Ok(value) = serde_json::from_str(input) {
                                    call.input = value;
                                }
                                if schema_enabled
                                    && call.name.as_deref() == Some("StructuredOutput")
                                {
                                    EventKind::StructuredOutput(call.input.clone())
                                } else {
                                    EventKind::ToolUpdated(call.clone())
                                }
                            } else {
                                EventKind::Raw
                            }
                        }
                        _ => EventKind::Raw,
                    };
                    BackendEvent::content(kind, f.clone())
                }
                "assistant" => {
                    if let Some(anchor) = f["uuid"]
                        .as_str()
                        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    {
                        let mut history = history.lock().unwrap();
                        if history.user.is_some() {
                            history.assistant = Some(anchor.into());
                        }
                    }
                    let mut message = f["message"].clone();
                    if message.is_object() && f["uuid"].is_string() {
                        message["uuid"] = f["uuid"].clone();
                    }
                    if schema_enabled {
                        for block in message["content"].as_array().into_iter().flatten() {
                            if block["type"] == "tool_use" && block["name"] == "StructuredOutput" {
                                let _ = tx
                                    .send(BackendEvent::content(
                                        EventKind::StructuredOutput(block["input"].clone()),
                                        f.clone(),
                                    ))
                                    .await;
                            }
                        }
                    }
                    BackendEvent::content(EventKind::AssistantMessage(message), f.clone())
                }
                "user" => {
                    if let Some(blocks) = f["message"]["content"].as_array() {
                        for b in blocks {
                            if b["type"] == "tool_result" {
                                let mut completed = tool(b, "completed");
                                if denied.remove(&completed.id) {
                                    continue;
                                }
                                if let Some(call) = calls.remove(&completed.id) {
                                    completed.name = call.name;
                                    completed.input = call.input;
                                    completed.metadata = call.metadata;
                                }
                                let typed = &f["tool_use_result"];
                                let background = typed["backgroundTaskId"].is_string()
                                    || typed["status"] == "async_launched";
                                if background {
                                    completed.status = "running".into();
                                    if !completed.metadata.is_object() {
                                        completed.metadata = json!({});
                                    }
                                    completed.metadata["background"] = json!(true);
                                    completed.metadata["taskId"] =
                                        typed["backgroundTaskId"].clone();
                                    calls.insert(completed.id.clone(), completed.clone());
                                }
                                completed.details = Some(tool_details(&f, &completed));
                                if schema_enabled
                                    && completed.name.as_deref() == Some("StructuredOutput")
                                {
                                    continue;
                                }
                                let _ = tx
                                    .send(BackendEvent::content(
                                        if background {
                                            EventKind::ToolUpdated(completed)
                                        } else {
                                            EventKind::ToolCompleted(completed)
                                        },
                                        f.clone(),
                                    ))
                                    .await;
                            }
                        }
                    }
                    BackendEvent::content(EventKind::Raw, f.clone())
                }
                "result" => {
                    wire_task.set_active(false);
                    let cancelled = history.lock().unwrap().cancelled
                        && (f["is_error"] == true
                            || matches!(
                                f["terminal_reason"].as_str(),
                                Some("aborted_tools" | "cancelled" | "interrupted")
                            ));
                    {
                        let mut history = history.lock().unwrap();
                        if let Some(user) = history.user.take() {
                            let assistant = history.assistant.take();
                            history.completed.push(TurnAnchors {
                                user: Some(user),
                                assistant,
                            });
                        }
                    }
                    if let Some(output) = f.get("structured_output") {
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::StructuredOutput(output.clone()),
                                f.clone(),
                            ))
                            .await;
                    }
                    if let Some(item_id) = message_id.clone() {
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::MessageEnded {
                                    item_id: Some(item_id),
                                },
                                f.clone(),
                            ))
                            .await;
                    }
                    message_id = None;
                    blocks.clear();
                    inputs.clear();
                    calls.retain(|_, call| call.status == "running");
                    denied.clear();
                    if let Some(usage) = f.get("usage") {
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::UsageUpdated(super::usage(
                                    &json!({"usage":usage,"costUsd":f["total_cost_usd"]}),
                                )),
                                f.clone(),
                            ))
                            .await;
                    }
                    if let Some(used) = context_tokens.take() {
                        let window = f["modelUsage"].as_object().and_then(|m| {
                            m.values().filter_map(|v| v["contextWindow"].as_u64()).max()
                        });
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::ContextUpdated(ContextUsage {
                                    used_tokens: Some(used),
                                    context_window_tokens: window,
                                    remaining_tokens: window.map(|w| w.saturating_sub(used)),
                                    percent: window
                                        .filter(|w| *w > 0)
                                        .map(|w| used as f64 / w as f64 * 100.0),
                                }),
                                f.clone(),
                            ))
                            .await;
                    }
                    if f["is_error"] == true && !cancelled {
                        let _ = tx
                            .send(BackendEvent::content(
                                EventKind::Error {
                                    message: result_error(&f),
                                },
                                f.clone(),
                            ))
                            .await;
                    }
                    BackendEvent::TurnFinished {
                        outcome: outcome::claude(&f, cancelled),
                        native_reason: f["subtype"].as_str().map(String::from),
                        raw: Some(f.clone()),
                    }
                }
                _ => BackendEvent::content(EventKind::Raw, f.clone()),
            };
            if tx.send(event).await.is_err() {
                break;
            }
        }
    })
}

struct ClaudeBackend {
    current: RwLock<LiveConnection>,
    options: SpawnOptions,
    requests: Arc<RwLock<BTreeMap<String, Value>>>,
    config: Arc<RwLock<Value>>,
    policy_observation: Arc<RwLock<Option<PolicyObservation>>>,
    commands: Arc<RwLock<Value>>,
    history: History,
    compaction: Compaction,
    events: mpsc::Sender<BackendEvent>,
    operation: tokio::sync::Mutex<()>,
}
fn prompt_content(prompt: &Prompt) -> Result<Vec<Value>> {
    prompt.content.iter().map(|content| match content {
        PromptContent::Text(text) => Ok(json!({"type":"text","text":text})),
        PromptContent::Image { data, mime_type } => Ok(json!({"type":"image","source":{"type":"base64","data":data,"media_type":mime_type}})),
        _ => Err(Error::Unsupported("Claude resource content".into())),
    }).collect()
}
async fn rewind_files(wire: &JsonProcess, user: &str, dry_run: bool) -> Result<()> {
    let receipt = wire
        .request(
            "rewind_files",
            json!({"user_message_id":user,"dry_run":dry_run}),
        )
        .await?;
    if receipt["canRewind"] == false || receipt["error"].is_string() {
        return Err(Error::Provider(
            receipt["error"]
                .as_str()
                .unwrap_or("Claude refused file rewind")
                .into(),
        ));
    }
    Ok(())
}
async fn close_worker(wire: &JsonProcess) -> Result<()> {
    let graceful = wire
        .request_timeout("close", json!({}), std::time::Duration::from_secs(5))
        .await;
    let closed = wire.close().await;
    closed?;
    graceful.map(|_| ())
}
#[async_trait]
impl AgentBackend for ClaudeBackend {
    async fn tasks(&self) -> Result<Vec<AgentTask>> {
        let connection = self.current.read().await;
        let receipt = connection.wire.request("tasks", json!({})).await?;
        serde_json::from_value(receipt)
            .map_err(|_| Error::Provider("Claude SDK task list could not be decoded".into()))
    }
    async fn stop_task(&self, task_id: &str) -> Result<()> {
        let connection = self.current.read().await;
        connection
            .wire
            .request("stop_task", json!({"taskId":task_id}))
            .await?;
        Ok(())
    }
    async fn steer(&self, p: &Prompt) -> Result<()> {
        let _operation = self.operation.lock().await;
        if self.history.lock().unwrap().user.is_none() || self.compaction.lock().unwrap().is_some()
        {
            return Err(Error::Invalid(
                "Claude steering requires an active turn".into(),
            ));
        }
        let content = prompt_content(p)?;
        let connection = self.current.read().await;
        connection.wire.request("prompt", json!({"type":"user","uuid":uuid::Uuid::new_v4().to_string(),"priority":"now","session_id":*connection.id.read().await,"message":{"role":"user","content":content},"parent_tool_use_id":null})).await?;
        Ok(())
    }
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        let _operation = self.operation.lock().await;
        if self.history.lock().unwrap().user.is_some() || self.compaction.lock().unwrap().is_some()
        {
            return Err(Error::Busy);
        }
        let connection = self.current.read().await;
        let commands = connection.skills.refresh(&*self.commands.read().await);
        let command = super::skills::native_command(p, &commands)?;
        let mut content = p.content.iter().filter(|c| !matches!(c, PromptContent::Skill { .. }) && (command.is_none() || !matches!(c, PromptContent::Text(_)))).map(|c| match c {
            PromptContent::Text(t) => Ok(json!({"type":"text","text":t})),
            PromptContent::Image { data,mime_type } => Ok(json!({"type":"image","source":{"type":"base64","data":data,"media_type":mime_type}})),
            _ => Err(Error::Unsupported("Claude resource content".into())),
        }).collect::<Result<Vec<_>>>()?;
        if let Some(command) = command {
            content.push(json!({"type":"text","text":super::skills::command_text(p, &command)}));
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        {
            let mut history = self.history.lock().unwrap();
            history.user = Some(uuid.clone());
            history.assistant = None;
            history.cancelled = false;
        }
        connection.wire.set_active(true);
        let result=connection.wire.request("prompt", json!({"type":"user","uuid":uuid,"session_id":*connection.id.read().await,"message":{"role":"user","content":content},"parent_tool_use_id":null})).await.map(|_| ());
        if result.is_err() {
            connection.wire.set_active(false);
            let mut history = self.history.lock().unwrap();
            history.user = None;
            history.assistant = None;
        }
        result
    }
    async fn cancel(&self) -> Result<()> {
        let connection = self.current.read().await;
        let previous = {
            let mut history = self.history.lock().unwrap();
            let previous = history.cancelled;
            history.cancelled = history.user.is_some();
            previous
        };
        let receipt = match connection.wire.request("interrupt", json!({})).await {
            Ok(receipt) => receipt,
            Err(error) => {
                self.history.lock().unwrap().cancelled = previous;
                return Err(error);
            }
        };
        let cancelled_queued = {
            let mut history = self.history.lock().unwrap();
            let cancelled = receipt["cancelled"]
                .as_array()
                .zip(history.user.as_deref())
                .is_some_and(|(cancelled, user)| {
                    cancelled.iter().any(|id| id.as_str() == Some(user))
                });
            if cancelled {
                history.user = None;
                history.assistant = None;
                history.completed.push(TurnAnchors {
                    user: None,
                    assistant: None,
                });
            }
            cancelled
        };
        if cancelled_queued {
            connection.wire.set_active(false);
            self.events
                .send(BackendEvent::TurnFinished {
                    outcome: TurnOutcome::Cancelled,
                    native_reason: None,
                    raw: Some(receipt),
                })
                .await
                .map_err(|_| Error::Closed)?;
        }
        Ok(())
    }
    async fn respond_permission(&self, r: &PermissionResponse) -> Result<()> {
        let connection = self.current.read().await;
        let mut requests = self.requests.write().await;
        let f = requests
            .get(&r.id)
            .ok_or_else(|| Error::Invalid("unknown Claude permission".into()))?;
        let response = if r.option == "answer" && f["request"]["tool_name"] == "AskUserQuestion" {
            let mut updated = f["request"]["input"].clone();
            let mut answers = serde_json::Map::new();
            if let Some(questions) = updated["questions"].as_array() {
                for (index, q) in questions.iter().enumerate() {
                    let id = q["id"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| index.to_string());
                    let value = &r.data["answers"][&id];
                    let text = if let Some(values) = value.as_array() {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        value.as_str().unwrap_or_default().into()
                    };
                    answers.insert(q["question"].as_str().unwrap_or(&id).into(), json!(text));
                }
            }
            updated["answers"] = json!(answers);
            json!({"behavior":"allow","updatedInput":updated})
        } else if r.option == "approve" {
            json!({"behavior":"allow","updatedInput":if r.data.is_null() { f["request"]["input"].clone() } else { r.data.clone() }})
        } else if r.option == "cancel" {
            json!({"behavior":"deny","message":"Cancelled by client","interrupt":true})
        } else {
            json!({"behavior":"deny","message":r.data["message"].as_str().or_else(||r.data["reason"].as_str()).unwrap_or("Denied by client")})
        };
        let cancelled = r.option == "cancel";
        if cancelled {
            self.history.lock().unwrap().cancelled = true;
        }
        let sent = connection
            .wire
            .request("permission", json!({"id":r.id,"response":response}))
            .await
            .map(|_| ());
        if let Err(error) = sent {
            if cancelled {
                self.history.lock().unwrap().cancelled = false;
            }
            return Err(error);
        }
        requests.remove(&r.id);
        Ok(())
    }
    async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        let connection = self.current.read().await;
        settings::model_descriptors(settings::model_choices(&connection.init["models"]))
    }
    async fn configure(&self, v: Value) -> Result<Value> {
        let connection = self.current.read().await;
        let mut candidate = self.config.read().await.clone();
        if let Some(fields) = v.as_object() {
            for (id, value) in fields {
                candidate[id] = value.clone();
            }
        }
        let descriptors = configuration_for(&connection.init, &candidate);
        let fields = v
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        settings::validate(&v, &descriptors)?;
        for (id, value) in fields {
            match id.as_str() {
                "model" => {
                    connection
                        .wire
                        .request("set_model", json!({"model":value}))
                        .await?;
                }
                "permissionMode" => {
                    connection
                        .wire
                        .request("set_permission_mode", json!({"mode":value}))
                        .await?;
                    *self.policy_observation.write().await =
                        Some(super::policy::claude_observation(
                            value.clone(),
                            PolicyEvidence::NativeAcknowledgement,
                            None,
                        ));
                }
                "effort" => {
                    connection
                        .wire
                        .request(
                            "apply_flag_settings",
                            json!({"settings":{"effortLevel":value}}),
                        )
                        .await?;
                }
                "fast" => {
                    connection
                        .wire
                        .request(
                            "apply_flag_settings",
                            json!({"settings":{"fastMode":value}}),
                        )
                        .await?;
                }
                _ => unreachable!("configuration was validated"),
            }
            let mut config = self.config.write().await;
            config[id] = value.clone();
            let _ = self
                .events
                .send(BackendEvent::content(
                    EventKind::ConfigurationUpdated(config.clone()),
                    json!({"source":"native_configuration_receipt","field":id}),
                ))
                .await;
        }
        Ok(self.config.read().await.clone())
    }
    async fn execution_policy(&self) -> Result<ExecutionPolicyReport> {
        Ok(super::policy::claude_report(
            &*self.config.read().await,
            self.policy_observation.read().await.clone(),
        ))
    }
    async fn compile_execution_profile(
        &self,
        profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        self.execution_policy().await?.require(profile)?;
        super::policy::claude_plan(profile)
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let connection = self.current.read().await;
        settings::descriptors(configuration_for(
            &connection.init,
            &*self.config.read().await,
        ))
    }
    async fn commands(&self) -> Result<Value> {
        let connection = self.current.read().await;
        let commands = connection.skills.refresh(&*self.commands.read().await);
        if commands.is_null() {
            return Err(Error::Unsupported(
                "Claude command catalog was not advertised".into(),
            ));
        }
        Ok(commands)
    }
    async fn quotas(&self) -> Result<Value> {
        let connection = self.current.read().await;
        let receipt = connection.wire.request("get_usage", json!({})).await?;
        let quotas = quota_windows(&receipt)?;
        self.events
            .send(BackendEvent::content(
                EventKind::QuotasUpdated(quotas.clone()),
                json!({"source":"claude_agent_sdk","operation":"usage","response":receipt}),
            ))
            .await
            .map_err(|_| Error::Closed)?;
        Ok(quotas)
    }
    async fn compact(&self) -> Result<()> {
        let _operation = self.operation.lock().await;
        if self.history.lock().unwrap().user.is_some() {
            return Err(Error::Busy);
        }
        let connection = self.current.read().await;
        let (reply, receipt) = tokio::sync::oneshot::channel();
        {
            let mut pending = self.compaction.lock().unwrap();
            if pending.is_some() {
                return Err(Error::Busy);
            }
            *pending = Some(CompactPending {
                reply,
                refusal: None,
            });
        }
        connection.wire.set_active(true);
        let sent = connection.wire.request("prompt", json!({"type":"user","uuid":uuid::Uuid::new_v4().to_string(),"session_id":*connection.id.read().await,"message":{"role":"user","content":"/compact"},"parent_tool_use_id":null})).await;
        if let Err(error) = sent {
            self.compaction.lock().unwrap().take();
            connection.wire.set_active(false);
            return Err(error);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(300), receipt).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::Closed),
            Err(_) => {
                self.compaction.lock().unwrap().take();
                // A late compaction result cannot safely be attributed to a later prompt.
                connection.wire.kill().await?;
                Err(Error::Timeout(
                    "Claude compaction receipt; session terminated".into(),
                ))
            }
        }
    }
    async fn rollback(&self, turns: u64) -> Result<()> {
        self.rollback_with(turns, RollbackScope::ConversationOnly)
            .await
    }
    async fn rollback_with(&self, turns: u64, scope: RollbackScope) -> Result<()> {
        let _operation = self.operation.lock().await;
        if self.options.execution.ephemeral {
            return Err(Error::Unsupported(
                "rollback of an ephemeral Claude session".into(),
            ));
        }
        let (cut, user, remaining) = {
            let history = self.history.lock().unwrap();
            if history.user.is_some() {
                return Err(Error::Busy);
            }
            let count = usize::try_from(turns)
                .map_err(|_| Error::Invalid("rollback count exceeds recorded history".into()))?;
            if count == 0 || count >= history.completed.len() {
                return Err(Error::Invalid(format!(
                    "Claude rollback needs at least one retained turn among {} completed turns observed by this live connection; earlier resumed history is unavailable and native compaction resets rollback anchors",
                    history.completed.len()
                )));
            }
            let remaining = history.completed.len() - count;
            let cut = history.completed[remaining - 1]
                .assistant
                .clone()
                .ok_or_else(|| {
                    Error::Unsupported("Claude rollback cut has no native assistant UUID".into())
                })?;
            (cut, history.completed[remaining].user.clone(), remaining)
        };
        let mut current = self.current.write().await;
        let previous_native = native("claude", current.id.read().await.clone());
        if scope == RollbackScope::ConversationAndFiles {
            if self
                .options
                .execution
                .env
                .get("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING")
                .is_some_and(|value| value != "true" && value != "1")
            {
                return Err(Error::Unsupported(
                    "Claude file checkpointing was disabled by the launch environment".into(),
                ));
            }
            rewind_files(
                &current.wire,
                user.as_deref().ok_or_else(|| {
                    Error::Unsupported(
                        "Claude rollback has no native user UUID for file rewind".into(),
                    )
                })?,
                true,
            )
            .await?;
        }
        let mut options = self.options.clone();
        let config = self.config.read().await.clone();
        if !options.config.is_object() {
            options.config = json!({});
        }
        if let Some(values) = config.as_object() {
            for (key, value) in values {
                options.config[key] = value.clone();
            }
        }
        options.model = config["model"].as_str().map(String::from);
        options.config["resumeAtMessage"] = json!(cut);
        if let Some(path) = options.config["diagnostics"]["recording"]["path"].as_str() {
            let path = std::path::PathBuf::from(path)
                .with_extension(format!("{}.jsonl", uuid::Uuid::new_v4()));
            options.config["diagnostics"]["recording"]["path"] = json!(path);
        }
        // Initialize the replacement before mutating the live connection or files.
        let replacement = launch_connection(&options, Some(&previous_native), true).await?;
        if scope == RollbackScope::ConversationAndFiles
            && let Err(error) = rewind_files(&current.wire, user.as_deref().unwrap(), false).await
        {
            let _ = replacement.wire.kill().await;
            return Err(error);
        }
        let id = replacement.id.clone();
        *self.commands.write().await = replacement.skills.catalog(&replacement.init["commands"]);
        current.decoder.abort();
        let old_wire = current.wire.clone();
        *self.policy_observation.write().await = None;
        let next = attach_decoder(
            replacement,
            self.requests.clone(),
            self.config.clone(),
            self.commands.clone(),
            self.history.clone(),
            self.compaction.clone(),
            self.events.clone(),
            self.options.execution.output_schema.is_some(),
            self.policy_observation.clone(),
        );
        *current = next;
        self.requests.write().await.clear();
        {
            let mut history = self.history.lock().unwrap();
            history.completed.truncate(remaining);
            history.user = None;
            history.assistant = None;
        }
        self.events
            .send(BackendEvent::NativeSessionChanged(native("claude", id)))
            .await
            .map_err(|_| Error::Closed)?;
        let _ = close_worker(&old_wire).await;
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        let connection = self.current.read().await;
        let result = close_worker(&connection.wire).await;
        if result.is_ok() {
            connection.skills.cleanup();
        }
        result
    }
    async fn kill(&self) -> Result<()> {
        let connection = self.current.read().await;
        let result = connection.wire.kill().await;
        if result.is_ok() {
            connection.skills.cleanup();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_results_preserve_authentication_errors_and_handle_missing_errors() {
        let message = "Failed to authenticate: OAuth session expired and could not be refreshed";
        assert_eq!(
            result_error(&json!({"is_error": true, "result": message, "errors": null})),
            message
        );
        assert_eq!(
            result_error(&json!({"errors": ["First error", "Second error"]})),
            "First error\nSecond error"
        );
        assert_eq!(
            result_error(&json!({"errors": "Provider unavailable"})),
            "Provider unavailable"
        );
        assert!(result_error(&json!({"errors": null})).contains("Check its authentication"));
    }
}
