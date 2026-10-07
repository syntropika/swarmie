use super::*;
#[path = "http_diagnostics.rs"]
mod http_diagnostics;
use async_trait::async_trait;
use futures_util::StreamExt;
use http_diagnostics::HttpDiagnostics;
use process::ManagedProcess;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, RwLock};

pub struct OpenCodeHarness;
fn capabilities() -> Capabilities {
    Capabilities {
        resume: true,
        fork: true,
        cancel: true,
        permissions: true,
        reasoning: true,
        usage: true,
        models: true,
        configure: true,
        images: true,
        rollback: true,
        compact: true,
        metadata: BTreeMap::from([
            ("forkAtMessage".into(), json!(true)),
            ("mcpTransports".into(), json!(["stdio", "http", "sse"])),
            ("toolsDisabled".into(), json!(false)),
            ("ephemeral".into(), json!(false)),
            ("outputSchema".into(), json!(true)),
            ("commands".into(), json!(true)),
            ("rollbackScopes".into(), json!(["conversation_and_files"])),
        ]),
        ..Default::default()
    }
}
struct Http {
    client: reqwest::Client,
    url: reqwest::Url,
    cwd: String,
    auth: Option<(String, String)>,
    diagnostics: Arc<HttpDiagnostics>,
}
impl Http {
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut url = self.url.clone();
        url.set_path(&format!(
            "{}{}",
            self.url.path().trim_end_matches('/'),
            path
        ));
        url.set_fragment(None);
        let mut request = self
            .client
            .request(method, url)
            .query(&[("directory", &self.cwd)]);
        if let Some((user, password)) = &self.auth {
            request = request.basic_auth(user, Some(password));
        }
        request
    }
    async fn event_stream(&self) -> Result<reqwest::Response> {
        let _pending = self.diagnostics.request();
        self.diagnostics.record(
            "send",
            &json!({"transport":"http","method":"GET","path":"/event","body":null}),
        );
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.request(reqwest::Method::GET, "/event").send(),
        )
        .await
        .map_err(|_| Error::Timeout("OpenCode event stream handshake".into()))?
        .map_err(|e| Error::Provider(e.without_url().to_string()))?;
        self.diagnostics.activity();
        self.diagnostics.record("receive", &json!({"transport":"http","method":"GET","path":"/event","status":response.status().as_u16(),"body":null}));
        if !response.status().is_success() {
            return Err(Error::Provider("OpenCode event stream refused".into()));
        }
        Ok(response)
    }
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let _pending = self.diagnostics.request();
        self.diagnostics.record(
            "send",
            &json!({"transport":"http","method":method.as_str(),"path":path,"body":body}),
        );
        let mut request = self
            .request(method.clone(), path)
            .timeout(Duration::from_secs(30));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|e| {
            self.diagnostics.record("receive", &json!({"transport":"http","method":method.as_str(),"path":path,"error":"request_failed"}));
            Error::Provider(e.without_url().to_string())
        })?;
        self.diagnostics.activity();
        let status = response.status();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::Provider(e.without_url().to_string()))?;
            self.diagnostics.activity();
            if bytes.len().saturating_add(chunk.len()) > 16 * 1024 * 1024 {
                self.diagnostics.record("receive", &json!({"transport":"http","method":method.as_str(),"path":path,"status":status.as_u16(),"error":"response_too_large"}));
                return Err(Error::Provider(
                    "OpenCode HTTP response exceeds 16 MiB".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let parsed = if bytes.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_slice::<Value>(&bytes)
        };
        self.diagnostics.record("receive", &json!({"transport":"http","method":method.as_str(),"path":path,"status":status.as_u16(),"body":parsed.as_ref().ok(),"error":if parsed.is_err() { Some("invalid_json") } else { None },"bytes":bytes.len()}));
        if !status.is_success() {
            return Err(Error::Provider(format!("OpenCode {path}: {status}")));
        }
        Ok(parsed?)
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }
    async fn get(&self, path: &str) -> Result<Value> {
        self.call(reqwest::Method::GET, path, None).await
    }
}
#[async_trait]
impl Harness for OpenCodeHarness {
    fn id(&self) -> &str {
        "opencode"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(detection("opencode", "opencode", capabilities()))
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
        connect_opencode(o, saved, fork, None).await
    }
    async fn fork_at(
        &self,
        o: &SpawnOptions,
        saved: &NativeSessionRef,
        message_id: &str,
    ) -> Result<BackendConnection> {
        connect_opencode(o, Some(saved), true, Some(message_id)).await
    }
}
async fn connect_opencode(
    o: &SpawnOptions,
    saved: Option<&NativeSessionRef>,
    fork: bool,
    message_id: Option<&str>,
) -> Result<BackendConnection> {
    if o.execution.tools == ToolPolicy::Disabled {
        return Err(Error::Unsupported(
            "OpenCode strict tool disabling; deny policies do not remove every native/MCP tool"
                .into(),
        ));
    }
    if o.execution.ephemeral {
        return Err(Error::Unsupported(
            "OpenCode ephemeral sessions; the native server persists conversations".into(),
        ));
    }
    if o.config["url"].is_string()
        && (!o.execution.env.is_empty()
            || o.execution.config_home.is_some()
            || !o.execution.mcp_servers.is_empty())
    {
        return Err(Error::Unsupported(
            "process environment/configuration/MCP injection on an external OpenCode server".into(),
        ));
    }
    let model = match &o.model {
        Some(m) => {
            let (p, m) = m
                .split_once('/')
                .ok_or_else(|| Error::Invalid("OpenCode model must be provider/model".into()))?;
            json!({"providerID":p,"modelID":m})
        }
        None => Value::Null,
    };
    let (tx, events) = mpsc::channel(512);
    let mut secrets = o.execution.env.clone();
    for (index, server) in o.execution.mcp_servers.iter().enumerate() {
        let values = match &server.transport {
            McpTransport::Stdio { env, .. } => env,
            McpTransport::Http { headers, .. } | McpTransport::Sse { headers, .. } => headers,
        };
        for (key, value) in values {
            secrets.insert(format!("mcp-{index}-{key}"), value.clone());
        }
        if let McpTransport::Http { url, .. } | McpTransport::Sse { url, .. } = &server.transport
            && let Ok(url) = reqwest::Url::parse(url)
        {
            for (query, (_, value)) in url.query_pairs().enumerate() {
                secrets.insert(format!("mcp-{index}-query-{query}"), value.into_owned());
            }
            if let Some(password) = url.password() {
                secrets.insert(format!("mcp-{index}-password"), password.into());
            }
        }
    }
    let mut process = None;
    let (url, auth): (String, Option<(String, String)>) =
        if let Some(url) = o.config["url"].as_str() {
            (
                url.trim_end_matches('/').to_string(),
                o.config["password"].as_str().map(|p| {
                    (
                        o.config["username"].as_str().unwrap_or("opencode").into(),
                        p.into(),
                    )
                }),
            )
        } else {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();
            drop(listener);
            let secret = uuid::Uuid::new_v4().to_string();
            let (exe, args) = launch(
                o,
                "opencode",
                vec![
                    "serve".into(),
                    "--hostname".into(),
                    "127.0.0.1".into(),
                    "--port".into(),
                    port.to_string(),
                ],
            );
            let mut environment = opencode_env(o)?;
            environment.insert("OPENCODE_SERVER_USERNAME".into(), "opencode".into());
            environment.insert("OPENCODE_SERVER_PASSWORD".into(), secret.clone());
            process = Some(
                ManagedProcess::spawn(
                    &exe,
                    &args,
                    &o.cwd,
                    &environment.into_iter().collect::<Vec<_>>(),
                )
                .await?,
            );
            (
                format!("http://127.0.0.1:{port}"),
                Some(("opencode".into(), secret)),
            )
        };
    let url = reqwest::Url::parse(&url)
        .map_err(|_| Error::Invalid("invalid OpenCode server URL".into()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::Invalid(
            "OpenCode server requires an HTTP(S) URL".into(),
        ));
    }
    for (index, (_, value)) in url.query_pairs().enumerate() {
        secrets.insert(format!("query-{index}"), value.into_owned());
    }
    if !url.username().is_empty() {
        secrets.insert("url-user".into(), url.username().into());
    }
    if let Some(password) = url.password() {
        secrets.insert("url-password".into(), password.into());
    }
    let mut auth_request = reqwest::Client::new().get(url.clone());
    if let Some((user, password)) = &auth {
        secrets.insert("server-user".into(), user.clone());
        secrets.insert("server-password".into(), password.clone());
        auth_request = auth_request.basic_auth(user, Some(password));
    }
    let auth_request = auth_request
        .build()
        .map_err(|e| Error::Provider(e.without_url().to_string()))?;
    if let Some(value) = auth_request
        .headers()
        .get(reqwest::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        secrets.insert("server-authorization".into(), value.into());
        if let Some(value) = value.strip_prefix("Basic ") {
            secrets.insert("server-authorization-encoded".into(), value.into());
        }
    }
    let diagnostics = HttpDiagnostics::open(&o.config, &secrets, tx.clone())?;
    let http = Arc::new(Http {
        client: reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| Error::Provider(format!("{e:?}")))?,
        url,
        cwd: o.cwd.to_string_lossy().into_owned(),
        auth,
        diagnostics,
    });
    let startup_error = Arc::new(Mutex::new(String::new()));
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if process.as_ref().is_some_and(ManagedProcess::exited) {
                return Err(Error::Provider(
                    "OpenCode server exited during startup".into(),
                ));
            }
            match tokio::time::timeout(Duration::from_millis(500), http.get("/global/health")).await
            {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(e)) => *startup_error.lock().await = e.to_string(),
                Err(_) => *startup_error.lock().await = "health probe deadline".into(),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| {
        Error::Timeout(format!(
            "OpenCode startup: {}",
            startup_error
                .try_lock()
                .map(|v| v.clone())
                .unwrap_or_default()
        ))
    })??;
    // Establish SSE before creating a session or sending prompts.
    let response = http.event_stream().await?;
    let mut fork_history = None;
    let result = if let Some(s) = saved {
        if fork {
            let history = fork_boundary(&http, &s.id, message_id).await?;
            let boundary = history.boundary.clone();
            fork_history = Some(history);
            http.post(
                &format!("/session/{}/fork", s.id),
                boundary
                    .map(|id| json!({"messageID":id}))
                    .unwrap_or(json!({})),
            )
            .await?
        } else {
            http.get(&format!("/session/{}", s.id)).await?
        }
    } else {
        http.post("/session", json!({})).await?
    };
    let id = result["id"]
        .as_str()
        .ok_or_else(|| Error::Provider("OpenCode omitted session id".into()))?
        .to_string();
    let mut connection_capabilities = capabilities();
    if let Some(history) = fork_history {
        let cloned = http.get(&format!("/session/{id}/message")).await?;
        let aliases = fork_aliases(
            &history.messages,
            &cloned,
            &o.config["_runtimeMessageIdAliases"],
        )?;
        connection_capabilities
            .metadata
            .insert("messageIdAliases".into(), json!(aliases));
    } else if saved.is_some() && !o.config["_runtimeMessageIdAliases"].is_null() {
        let aliases = retained_aliases(
            &http.get(&format!("/session/{id}/message")).await?,
            result["revert"]["messageID"].as_str(),
            &o.config["_runtimeMessageIdAliases"],
        )?;
        connection_capabilities
            .metadata
            .insert("messageIdAliases".into(), json!(aliases));
    }
    let requests: Arc<RwLock<BTreeMap<String, Value>>> = Arc::default();
    let request_task = requests.clone();
    let active = Arc::new(AtomicBool::new(false));
    let active_task = active.clone();
    let outcome: Arc<RwLock<String>> = Arc::new(RwLock::new("completed".into()));
    let outcome_task = outcome.clone();
    let http_task = http.clone();
    let session = id.clone();
    let catalog = http.get("/config/providers").await?;
    let model = if model.is_null() && result["model"].is_object() {
        json!({"providerID":result["model"]["providerID"],"modelID":result["model"]["id"].as_str().or(result["model"]["modelID"].as_str())})
    } else {
        model
    };
    if !model.is_null() {
        catalog_model(&catalog, &model)?;
    }
    let effort = selected_effort(&catalog, &model, o.config.get("effort"))?;
    let model = Arc::new(Mutex::new(Selection { model, effort }));
    let context_windows = Arc::new(RwLock::new(model_windows(&catalog)));
    let windows_task = context_windows.clone();
    let model_task = model.clone();
    let backend_tx = tx.clone();
    let task = tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::<u8>::new();
        let mut scan_offset = 0usize;
        let mut text_offsets: BTreeMap<String, usize> = BTreeMap::new();
        let mut message_roles: BTreeMap<String, String> = BTreeMap::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    http_task.diagnostics.turn(false);
                    let _ = tx
                        .send(BackendEvent::Exited {
                            message: format!("OpenCode SSE: {}", e.without_url()),
                        })
                        .await;
                    return;
                }
            };
            http_task.diagnostics.activity();
            for chunk in chunk.chunks(64 * 1024) {
                buffer.extend_from_slice(chunk);
                loop {
                    // A delimiter can straddle chunks; retain only its three-byte overlap.
                    let tail = &buffer[scan_offset..];
                    let lf = tail
                        .windows(2)
                        .position(|w| w == b"\n\n")
                        .map(|i| (i + scan_offset, 2));
                    let crlf = tail
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|i| (i + scan_offset, 4));
                    let delimiter = [lf, crlf].into_iter().flatten().min_by_key(|(end, _)| *end);
                    let Some((end, n)) = delimiter else {
                        scan_offset = buffer.len().saturating_sub(3);
                        break;
                    };
                    if end > 8 * 1024 * 1024 {
                        http_task.diagnostics.turn(false);
                        let _ = tx
                            .send(BackendEvent::Exited {
                                message: "OpenCode SSE record too large".into(),
                            })
                            .await;
                        return;
                    }
                    let frame = String::from_utf8_lossy(&buffer[..end]).into_owned();
                    buffer.drain(..end + n);
                    scan_offset = 0;
                    let data = frame
                        .lines()
                        .filter_map(|l| l.strip_prefix("data:"))
                        .map(str::trim_start)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if data.is_empty() {
                        continue;
                    }
                    let f: Value = match serde_json::from_str(&data) {
                        Ok(v) => v,
                        Err(e) => {
                            http_task.diagnostics.turn(false);
                            http_task.diagnostics.record("receive", &json!({"transport":"sse","error":"invalid_json","bytes":data.len()}));
                            let _ = tx
                                .send(BackendEvent::Exited {
                                    message: format!("malformed OpenCode SSE: {e}"),
                                })
                                .await;
                            return;
                        }
                    };
                    http_task
                        .diagnostics
                        .record("receive", &json!({"transport":"sse","event":f}));
                    let p = &f["properties"];
                    let t = f["type"].as_str().unwrap_or("");
                    if t == "session.created" && p["info"]["parentID"] == session {
                        let _ = tx.send(BackendEvent::content(EventKind::SubagentUpdated(json!({"id":p["info"]["id"],"parentSessionId":session,"title":p["info"]["title"],"status":"created","native":p["info"]}).into()),f.clone())).await;
                        continue;
                    }
                    let sid = p["sessionID"]
                        .as_str()
                        .or_else(|| p["part"]["sessionID"].as_str())
                        .or_else(|| p["info"]["sessionID"].as_str())
                        .or_else(|| {
                            p["info"]["id"]
                                .as_str()
                                .filter(|_| t.starts_with("session."))
                        });
                    if sid.is_some_and(|s| s != session) {
                        if tx
                            .send(BackendEvent::content(EventKind::Raw, f))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        continue;
                    }
                    if t == "message.updated"
                        && let (Some(id), Some(role)) =
                            (p["info"]["id"].as_str(), p["info"]["role"].as_str())
                    {
                        message_roles.insert(id.into(), role.into());
                    }
                    let mid = p["messageID"]
                        .as_str()
                        .or_else(|| p["part"]["messageID"].as_str());
                    if mid
                        .and_then(|id| message_roles.get(id))
                        .is_some_and(|role| role != "assistant")
                    {
                        if tx
                            .send(BackendEvent::content(EventKind::Raw, f))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        continue;
                    }
                    if t == "session.idle"
                        || (t == "session.status" && p["status"]["type"] == "idle")
                    {
                        if !active_task.load(Ordering::SeqCst) {
                            if tx
                                .send(BackendEvent::content(EventKind::Raw, f))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }
                        // Abort can emit a late duplicate idle after a new prompt has started.
                        // Validate against authoritative server state before ending the current turn.
                        match http_task.get("/session/status").await {
                            Ok(status)
                                if status[&session]["type"]
                                    .as_str()
                                    .is_some_and(|s| s != "idle") =>
                            {
                                if tx
                                    .send(BackendEvent::content(EventKind::Raw, f))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                continue;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(BackendEvent::content(
                                        EventKind::Error {
                                            message: e.to_string(),
                                        },
                                        f.clone(),
                                    ))
                                    .await;
                                continue;
                            }
                            _ => (),
                        }
                        let finished = active_task.swap(false, Ordering::SeqCst);
                        if finished {
                            http_task.diagnostics.turn(false);
                        }
                        if finished
                            && tx
                                .send(BackendEvent::TurnFinished {
                                    reason: outcome_task.read().await.clone(),
                                    raw: Some(f),
                                })
                                .await
                                .is_err()
                        {
                            return;
                        }
                        continue;
                    }
                    let event = match t {
                        "session.error" => {
                            active_task.store(false, Ordering::SeqCst);
                            http_task.diagnostics.turn(false);
                            let _ = tx
                                .send(BackendEvent::content(
                                    EventKind::Error {
                                        message: p["error"].to_string(),
                                    },
                                    f.clone(),
                                ))
                                .await;
                            BackendEvent::TurnFinished {
                                reason: "failed".into(),
                                raw: Some(f.clone()),
                            }
                        }
                        "message.part.delta" => {
                            let field = p["field"].as_str().unwrap_or("text");
                            let delta = string(p, "delta");
                            let id = string(p, "partID");
                            *text_offsets.entry(id.clone()).or_default() += delta.len();
                            BackendEvent::content(
                                if field == "text" {
                                    EventKind::TextDelta {
                                        text: delta,
                                        item_id: Some(id),
                                    }
                                } else {
                                    EventKind::ReasoningDelta { text: delta }
                                },
                                f.clone(),
                            )
                        }
                        "message.part.updated" => {
                            let part = &p["part"];
                            if part["tool"] == "task"
                                && let Some(child) = part["state"]["metadata"]["sessionId"].as_str()
                            {
                                let _ = tx.send(BackendEvent::content(EventKind::SubagentUpdated(json!({"id":child,"parentSessionId":session,"title":part["state"]["title"],"status":part["state"]["status"],"model":part["state"]["metadata"]["model"],"background":part["state"]["metadata"]["background"]}).into()),f.clone())).await;
                            }
                            let kind = match part["type"].as_str().unwrap_or("") {
                                "text" | "reasoning" => {
                                    let id = string(part, "id");
                                    let text = part["text"].as_str().unwrap_or("");
                                    let offset = text_offsets.entry(id.clone()).or_default();
                                    let delta = p["delta"]
                                        .as_str()
                                        .unwrap_or_else(|| text.get(*offset..).unwrap_or(""));
                                    *offset = text.len().max(*offset);
                                    if part["type"] == "text" {
                                        EventKind::TextDelta {
                                            text: delta.into(),
                                            item_id: Some(id),
                                        }
                                    } else {
                                        EventKind::ReasoningDelta { text: delta.into() }
                                    }
                                }
                                "tool" => match part["state"]["status"].as_str().unwrap_or("") {
                                    "pending" => {
                                        EventKind::ToolStarted(opencode_tool(part, "running"))
                                    }
                                    "completed" | "error" => {
                                        EventKind::ToolCompleted(opencode_tool(part, "completed"))
                                    }
                                    _ => EventKind::ToolUpdated(opencode_tool(part, "running")),
                                },
                                "step-finish" => EventKind::UsageUpdated(usage(part)),
                                _ => EventKind::Raw,
                            };
                            BackendEvent::content(kind, f.clone())
                        }
                        "message.updated" if p["info"]["role"] == "assistant" => {
                            let info = &p["info"];
                            if info["providerID"].is_string() && info["modelID"].is_string() {
                                let mut selection = model_task.lock().await;
                                if selection.model.is_null() {
                                    selection.model = json!({"providerID":info["providerID"],"modelID":info["modelID"]});
                                }
                            }
                            if !info["structured"].is_null() {
                                let _ = tx
                                    .send(BackendEvent::content(
                                        EventKind::StructuredOutput(info["structured"].clone()),
                                        f.clone(),
                                    ))
                                    .await;
                            }
                            if info["time"]["completed"].is_number() {
                                let window = windows_task
                                .read()
                                .await
                                .get(&model_key(&json!({"providerID":info["providerID"],"modelID":info["modelID"]})))
                                .copied();
                                let context = opencode_context(&info["tokens"], window);
                                let _ = tx
                                    .send(BackendEvent::content(
                                        EventKind::ContextUpdated(context),
                                        f.clone(),
                                    ))
                                    .await;
                                let _ = tx
                                    .send(BackendEvent::content(
                                        EventKind::MessageEnded {
                                            item_id: info["id"].as_str().map(String::from),
                                        },
                                        f.clone(),
                                    ))
                                    .await;
                            }
                            BackendEvent::content(
                                EventKind::AssistantMessage(p["info"].clone()),
                                f.clone(),
                            )
                        }
                        "permission.asked" | "permission.updated" => {
                            let id = string(p, "id");
                            request_task.write().await.insert(id.clone(), f.clone());
                            let request = PermissionRequest {
                                id,
                                kind: p["permission"].as_str().unwrap_or("tool").into(),
                                title: p["title"]
                                    .as_str()
                                    .or_else(|| p["permission"].as_str())
                                    .unwrap_or("Approve operation")
                                    .into(),
                                description: None,
                                command: p.get("metadata").cloned(),
                                paths: p["patterns"]
                                    .as_array()
                                    .map(|a| {
                                        a.iter()
                                            .filter_map(Value::as_str)
                                            .map(String::from)
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                questions: Vec::new(),
                                options: vec![
                                    option("once", "Approve once", "approve_once"),
                                    option("always", "Approve for session", "approve_session"),
                                    option("reject", "Deny", "deny"),
                                ],
                                metadata: {
                                    let mut metadata = p.clone();
                                    metadata["cancelSemantics"] = json!("reject_request");
                                    metadata["rejectionMessage"] = json!(true);
                                    metadata
                                },
                            };
                            let mut request = request;
                            if let Err(error) = request.normalize_questions() {
                                http_task.diagnostics.turn(false);
                                let _ = tx
                                    .send(BackendEvent::Exited {
                                        message: error.to_string(),
                                    })
                                    .await;
                                return;
                            }
                            BackendEvent::content(
                                EventKind::PermissionRequested(request),
                                f.clone(),
                            )
                        }
                        "question.asked" => {
                            let id = string(p, "id");
                            request_task.write().await.insert(id.clone(), f.clone());
                            let mut request = PermissionRequest {
                                id,
                                kind: "question".into(),
                                title: "Agent needs answers".into(),
                                description: None,
                                command: None,
                                paths: vec![],
                                questions: Vec::new(),
                                options: vec![
                                    option("answer", "Submit answers", "answer"),
                                    option("reject", "Reject question", "deny"),
                                ],
                                metadata: json!({"questions":settings::questions(&p["questions"]),"native":p,"cancelSemantics":"reject_request","rejectionMessage":false}),
                            };
                            if let Err(error) = request.normalize_questions() {
                                http_task.diagnostics.turn(false);
                                let _ = tx
                                    .send(BackendEvent::Exited {
                                        message: error.to_string(),
                                    })
                                    .await;
                                return;
                            }
                            BackendEvent::content(
                                EventKind::PermissionRequested(request),
                                f.clone(),
                            )
                        }
                        "session.diff" => {
                            BackendEvent::content(EventKind::Diff(p.clone()), f.clone())
                        }
                        "todo.updated" => BackendEvent::content(
                            EventKind::PlanUpdated(p.clone().into()),
                            f.clone(),
                        ),
                        _ => BackendEvent::content(EventKind::Raw, f.clone()),
                    };
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
                if buffer.len() > 8 * 1024 * 1024 {
                    http_task.diagnostics.turn(false);
                    let _ = tx
                        .send(BackendEvent::Exited {
                            message: "OpenCode SSE record too large".into(),
                        })
                        .await;
                    return;
                }
            }
        }
        http_task.diagnostics.turn(false);
        let _ = tx
            .send(BackendEvent::Exited {
                message: "OpenCode event stream closed".into(),
            })
            .await;
    });
    Ok(BackendConnection {
        native: native("opencode", id.clone()),
        capabilities: connection_capabilities,
        events,
        backend: Box::new(OpenCodeBackend {
            http,
            id,
            process,
            task,
            requests,
            model,
            context_windows,
            tx: backend_tx,
            output_schema: o.execution.output_schema.clone(),
            active,
            outcome,
        }),
    })
}
fn opencode_tool(part: &Value, status: &str) -> ToolEvent {
    let mut event = tool(part, status);
    event.metadata["native"] = part["state"]["metadata"].clone();
    event.metadata["attachments"] = part["state"]["attachments"].clone();
    event
}
fn model_key(model: &Value) -> String {
    format!(
        "{}/{}",
        model["providerID"].as_str().unwrap_or_default(),
        model["modelID"].as_str().unwrap_or_default()
    )
}
fn parse_model(value: &Value) -> Result<Value> {
    let model = if let Some(id) = value.as_str() {
        let (provider, model) = id
            .split_once('/')
            .ok_or_else(|| Error::Invalid("OpenCode model requires provider/model".into()))?;
        json!({"providerID":provider,"modelID":model})
    } else {
        value.clone()
    };
    if model["providerID"].as_str().is_none_or(|id| id.is_empty())
        || model["modelID"].as_str().is_none_or(|id| id.is_empty())
    {
        return Err(Error::Invalid(
            "model requires providerID and modelID".into(),
        ));
    }
    Ok(json!({"providerID":model["providerID"],"modelID":model["modelID"]}))
}
fn catalog_model<'a>(catalog: &'a Value, model: &Value) -> Result<&'a Value> {
    catalog["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|provider| provider["id"] == model["providerID"])
        .and_then(|provider| provider["models"].get(model["modelID"].as_str()?))
        .filter(|model| model.is_object())
        .ok_or_else(|| Error::Invalid("OpenCode model is not in the native catalog".into()))
}
fn model_variants(catalog: &Value, model: &Value) -> Vec<String> {
    catalog_model(catalog, model)
        .ok()
        .and_then(|model| model["variants"].as_object())
        .map(|variants| {
            variants
                .iter()
                .filter(|(_, settings)| settings["disabled"] != true)
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}
fn selected_effort(
    catalog: &Value,
    model: &Value,
    value: Option<&Value>,
) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let effort = value.as_str().ok_or_else(|| {
                Error::Invalid("OpenCode effort must be a native variant ID or null".into())
            })?;
            if !model_variants(catalog, model).iter().any(|id| id == effort) {
                return Err(Error::Invalid(
                    "OpenCode effort is not supported by the selected model".into(),
                ));
            }
            Ok(Some(effort.into()))
        }
    }
}
fn model_choices(catalog: &Value) -> Value {
    let mut choices = vec![];
    for provider in catalog["providers"].as_array().into_iter().flatten() {
        if let Some(models) = provider["models"].as_object() {
            for (id, model) in models {
                let native = json!({"providerID":provider["id"],"modelID":id});
                choices.push(json!({"id":model_key(&native),"label":model["name"].as_str().unwrap_or(id),"variants":model_variants(catalog,&native)}));
            }
        }
    }
    json!(choices)
}
fn model_windows(catalog: &Value) -> BTreeMap<String, u64> {
    let mut windows = BTreeMap::new();
    if let Some(providers) = catalog["providers"].as_array() {
        for provider in providers {
            if let Some(models) = provider["models"].as_object() {
                for (id, model) in models {
                    if let Some(window) = model["limit"]["context"].as_u64() {
                        windows.insert(
                            format!("{}/{}", provider["id"].as_str().unwrap_or_default(), id),
                            window,
                        );
                    }
                }
            }
        }
    }
    windows
}
fn opencode_context(tokens: &Value, window: Option<u64>) -> ContextUsage {
    let used = tokens["input"]
        .as_u64()
        .zip(tokens["output"].as_u64())
        .map(|(i, o)| {
            i + o
                + tokens["cache"]["read"].as_u64().unwrap_or(0)
                + tokens["cache"]["write"].as_u64().unwrap_or(0)
        });
    ContextUsage {
        used_tokens: used,
        context_window_tokens: window,
        remaining_tokens: used.zip(window).map(|(u, w)| w.saturating_sub(u)),
        percent: used
            .zip(window)
            .filter(|(_, w)| *w > 0)
            .map(|(u, w)| u as f64 / w as f64 * 100.0),
    }
}
fn opencode_env(options: &SpawnOptions) -> Result<BTreeMap<String, String>> {
    let mut env = execution_env(options, Some("OPENCODE_CONFIG_DIR"))?;
    if options.execution.mcp_servers.is_empty() {
        return Ok(env);
    }
    let inherited = env
        .get("OPENCODE_CONFIG_CONTENT")
        .cloned()
        .or_else(|| std::env::var("OPENCODE_CONFIG_CONTENT").ok());
    let mut config: Value = match inherited {
        Some(content) => serde_json::from_str(&content)?,
        None => json!({}),
    };
    let object = config
        .as_object_mut()
        .ok_or_else(|| Error::Invalid("OpenCode inline configuration must be an object".into()))?;
    let mcp = object
        .entry("mcp")
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Invalid("OpenCode mcp configuration must be an object".into()))?;
    for server in &options.execution.mcp_servers {
        if mcp.contains_key(&server.name) {
            return Err(Error::Invalid(format!(
                "duplicate OpenCode MCP server {}",
                server.name
            )));
        }
        let value = match &server.transport {
            McpTransport::Stdio { command, args, env } => {
                let mut command = vec![command.to_string_lossy().into_owned()];
                command.extend(args.clone());
                json!({"type":"local","command":command,"environment":env,"enabled":true})
            }
            McpTransport::Http { url, headers } | McpTransport::Sse { url, headers } => {
                json!({"type":"remote","url":url,"headers":headers,"enabled":true})
            }
        };
        mcp.insert(server.name.clone(), value);
    }
    env.insert(
        "OPENCODE_CONFIG_CONTENT".into(),
        serde_json::to_string(&config)?,
    );
    Ok(env)
}
struct OpenCodeBackend {
    http: Arc<Http>,
    id: String,
    process: Option<ManagedProcess>,
    task: tokio::task::JoinHandle<()>,
    requests: Arc<RwLock<BTreeMap<String, Value>>>,
    model: Arc<Mutex<Selection>>,
    context_windows: Arc<RwLock<BTreeMap<String, u64>>>,
    tx: mpsc::Sender<BackendEvent>,
    output_schema: Option<Value>,
    active: Arc<AtomicBool>,
    outcome: Arc<RwLock<String>>,
}
#[derive(Clone)]
struct Selection {
    model: Value,
    effort: Option<String>,
}
impl Drop for OpenCodeBackend {
    fn drop(&mut self) {
        self.http.diagnostics.turn(false);
        self.task.abort();
    }
}
#[async_trait]
impl AgentBackend for OpenCodeBackend {
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        if p.content
            .iter()
            .any(|part| matches!(part, PromptContent::Skill { .. }))
        {
            return Err(Error::Unsupported("OpenCode native skill input".into()));
        }
        let parts = p.content.iter().map(|c| match c {
            PromptContent::Text(t) => json!({"type":"text","text":t}),
            PromptContent::Image { data,mime_type } => json!({"type":"file","mime":mime_type,"url":format!("data:{mime_type};base64,{data}")}),
            PromptContent::Resource { uri } => json!({"type":"file","url":uri,"mime":"text/plain"}),
            PromptContent::Skill { .. } => unreachable!("skill inputs rejected before preparing parts"),
        }).collect::<Vec<_>>();
        let mut body = json!({"parts":parts});
        if let Some(schema) = &self.output_schema {
            body["format"] = json!({"type":"json_schema","schema":schema});
        }
        let selection = self.model.lock().await.clone();
        if !selection.model.is_null() {
            body["model"] = selection.model;
        }
        if let Some(effort) = selection.effort {
            body["variant"] = json!(effort);
        }
        *self.outcome.write().await = "completed".into();
        self.active.store(true, Ordering::SeqCst);
        self.http.diagnostics.turn(true);
        if let Err(e) = self
            .http
            .post(&format!("/session/{}/prompt_async", self.id), body)
            .await
        {
            self.active.store(false, Ordering::SeqCst);
            self.http.diagnostics.turn(false);
            return Err(e);
        }
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        *self.outcome.write().await = "cancelled".into();
        self.http
            .post(&format!("/session/{}/abort", self.id), json!({}))
            .await?;
        Ok(())
    }
    async fn respond_permission(&self, r: &PermissionResponse) -> Result<()> {
        let mut requests = self.requests.write().await;
        let f = requests
            .get(&r.id)
            .ok_or_else(|| Error::Invalid("unknown OpenCode request".into()))?;
        let path = if f["type"] == "question.asked" {
            format!(
                "/question/{}/{}",
                r.id,
                if r.option == "answer" {
                    "reply"
                } else {
                    "reject"
                }
            )
        } else {
            format!("/permission/{}/reply", r.id)
        };
        let body = if f["type"] == "question.asked" {
            if r.option == "answer" {
                let questions = &f["properties"]["questions"];
                let questions = if questions.is_array() {
                    questions
                } else {
                    &f["questions"]
                };
                let answers: Vec<_> = questions
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .enumerate()
                            .map(|(i, q)| {
                                let id = q["id"]
                                    .as_str()
                                    .map(String::from)
                                    .unwrap_or_else(|| i.to_string());
                                let answer = &r.data["answers"][&id];
                                if answer.is_array() {
                                    answer.clone()
                                } else {
                                    json!([answer.as_str().unwrap_or_default()])
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                json!({"answers":answers})
            } else {
                json!({})
            }
        } else {
            let reply = match r.option.as_str() {
                "once" => "once",
                "always" => "always",
                "reject" | "cancel" => "reject",
                _ => return Err(Error::Invalid("unknown OpenCode permission choice".into())),
            };
            let mut body = json!({"reply":reply});
            if reply == "reject" {
                for field in ["message", "reason"] {
                    if let Some(message) = r.data.get(field) {
                        let message = message.as_str().ok_or_else(|| {
                            Error::Invalid(format!("permission {field} must be a string"))
                        })?;
                        body["message"] = json!(message);
                        break;
                    }
                }
            }
            body
        };
        self.http.post(&path, body).await?;
        requests.remove(&r.id);
        Ok(())
    }
    async fn configure(&self, v: Value) -> Result<Value> {
        let fields = v
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        if let Some(key) = fields
            .keys()
            .find(|key| !["model", "effort"].contains(&key.as_str()))
        {
            return Err(Error::Invalid(format!(
                "unknown OpenCode configuration {key}"
            )));
        }
        let catalog = self.http.get("/config/providers").await?;
        let mut selection = self.model.lock().await;
        let model = if let Some(model) = fields.get("model") {
            parse_model(model)?
        } else {
            selection.model.clone()
        };
        if !model.is_null() {
            catalog_model(&catalog, &model)?;
        }
        let effort = if let Some(effort) = fields.get("effort") {
            selected_effort(&catalog, &model, Some(effort))?
        } else {
            selection
                .effort
                .clone()
                .filter(|effort| model_variants(&catalog, &model).contains(effort))
        };
        let window = model_windows(&catalog).get(&model_key(&model)).copied();
        let acknowledged = json!({"model":if model.is_null() { Value::Null } else { json!(model_key(&model)) },"effort":effort});
        *selection = Selection { model, effort };
        drop(selection);
        *self.context_windows.write().await = model_windows(&catalog);
        self.tx
            .send(BackendEvent::Content {
                kind: Box::new(EventKind::ConfigurationUpdated(acknowledged.clone())),
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)?;
        if fields.contains_key("model") {
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
        Ok(acknowledged)
    }
    async fn commands(&self) -> Result<Value> {
        self.http.get("/command").await
    }
    async fn rollback(&self, _turns: u64) -> Result<()> {
        Err(Error::Unsupported(
            "OpenCode native revert restores files; explicitly select conversation_and_files"
                .into(),
        ))
    }
    async fn rollback_with(&self, turns: u64, scope: RollbackScope) -> Result<()> {
        if scope != RollbackScope::ConversationAndFiles {
            return self.rollback(turns).await;
        }
        if turns == 0 {
            return Err(Error::Invalid(
                "rollback requires a positive turn count".into(),
            ));
        }
        let messages = self
            .http
            .get(&format!("/session/{}/message", self.id))
            .await?;
        let session = self.http.get(&format!("/session/{}", self.id)).await?;
        let boundary = session["revert"]["messageID"].as_str();
        let users = messages
            .as_array()
            .ok_or_else(|| Error::Provider("OpenCode omitted message list".into()))?
            .iter()
            .take_while(|m| boundary.is_none_or(|id| m["info"]["id"] != id))
            .filter(|m| m["info"]["role"] == "user")
            .collect::<Vec<_>>();
        let count = usize::try_from(turns)
            .map_err(|_| Error::Invalid("rollback count is too large".into()))?;
        let index = users
            .len()
            .checked_sub(count)
            .ok_or_else(|| Error::Invalid("rollback exceeds available turns".into()))?;
        let message = users[index]["info"]["id"]
            .as_str()
            .ok_or_else(|| Error::Provider("OpenCode omitted user message id".into()))?;
        self.http
            .post(
                &format!("/session/{}/revert", self.id),
                json!({"messageID":message}),
            )
            .await?;
        Ok(())
    }
    async fn compact(&self) -> Result<()> {
        let mut model = self.model.lock().await.model.clone();
        if model.is_null() {
            let messages = self
                .http
                .get(&format!("/session/{}/message", self.id))
                .await?;
            if let Some(info) = messages
                .as_array()
                .and_then(|a| {
                    a.iter().rev().find(|m| {
                        m["info"]["model"].is_object() || m["info"]["providerID"].is_string()
                    })
                })
                .map(|m| &m["info"])
            {
                model = if info["model"].is_object() {
                    info["model"].clone()
                } else {
                    json!({"providerID":info["providerID"],"modelID":info["modelID"]})
                };
            }
        }
        if !model["providerID"].is_string() || !model["modelID"].is_string() {
            return Err(Error::Unsupported(
                "OpenCode compaction needs a selected or previously used native model".into(),
            ));
        }
        self.http
            .post(&format!("/session/{}/summarize", self.id), model)
            .await?;
        Ok(())
    }
    async fn models(&self) -> Result<Value> {
        Ok(model_choices(&self.http.get("/config/providers").await?))
    }
    async fn configuration(&self) -> Result<Value> {
        let catalog = self.http.get("/config/providers").await?;
        let selection = self.model.lock().await.clone();
        let mut descriptors = vec![settings::select(
            "model",
            "Model",
            if selection.model.is_null() {
                Value::Null
            } else {
                json!(model_key(&selection.model))
            },
            model_choices(&catalog),
        )];
        let variants = model_variants(&catalog, &selection.model);
        if !variants.is_empty() {
            descriptors.push(settings::select(
                "effort",
                "Variant",
                json!(selection.effort),
                json!(
                    variants
                        .iter()
                        .map(|id| json!({"id":id,"label":id}))
                        .collect::<Vec<_>>()
                ),
            ));
        }
        Ok(json!(descriptors))
    }
    async fn close(&self) -> Result<()> {
        self.http.diagnostics.turn(false);
        let _ = self.cancel().await;
        self.task.abort();
        if let Some(p) = &self.process {
            p.kill().await?;
        }
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        self.http.diagnostics.turn(false);
        self.task.abort();
        if let Some(p) = &self.process {
            p.kill().await?;
        } else {
            self.cancel().await?;
        }
        Ok(())
    }
}

/// Native OpenCode fork excludes its anchor. Translate the inclusive runtime cut to the next user boundary.
struct ForkHistory {
    boundary: Option<String>,
    messages: Vec<Value>,
}
async fn fork_boundary(http: &Http, session: &str, assistant: Option<&str>) -> Result<ForkHistory> {
    let state = http.get(&format!("/session/{session}")).await?;
    let reverted = state["revert"]["messageID"].as_str();
    let history = http.get(&format!("/session/{session}/message")).await?;
    let messages = history
        .as_array()
        .ok_or_else(|| Error::Provider("OpenCode messages were not an array".into()))?;
    let visible = if let Some(reverted) = reverted {
        messages
            .iter()
            .position(|message| message["info"]["id"] == reverted)
            .ok_or_else(|| {
                Error::Provider("OpenCode omitted its revert boundary from history".into())
            })?
    } else {
        messages.len()
    };
    let Some(assistant) = assistant else {
        return Ok(ForkHistory {
            boundary: reverted.map(String::from),
            messages: messages[..visible].to_vec(),
        });
    };
    let index = messages[..visible]
        .iter()
        .position(|message| message["info"]["id"] == assistant)
        .ok_or_else(|| {
            Error::Invalid("OpenCode fork anchor is absent from visible native history".into())
        })?;
    let info = &messages[index]["info"];
    if info["role"] != "assistant" || !info["time"]["completed"].is_number() {
        return Err(Error::Invalid(
            "OpenCode fork anchor must be a completed assistant message".into(),
        ));
    }
    let next_user = messages[index + 1..visible]
        .iter()
        .position(|message| message["info"]["role"] == "user")
        .map(|offset| index + 1 + offset)
        .unwrap_or(visible);
    if messages[index + 1..next_user]
        .iter()
        .any(|message| message["info"]["role"] == "assistant")
    {
        return Err(Error::Invalid(
            "OpenCode fork anchor is not the final assistant of its turn".into(),
        ));
    }
    Ok(ForkHistory {
        boundary: messages
            .get(next_user)
            .and_then(|message| message["info"]["id"].as_str())
            .map(String::from),
        messages: messages[..next_user].to_vec(),
    })
}

/// The native fork copies the prefix in order and replaces IDs. Never identify messages by text.
fn fork_aliases(
    source: &[Value],
    cloned: &Value,
    inherited: &Value,
) -> Result<BTreeMap<String, String>> {
    let cloned = cloned
        .as_array()
        .ok_or_else(|| Error::Provider("OpenCode cloned messages were not an array".into()))?;
    if source.len() != cloned.len() {
        return Err(Error::Provider(
            "OpenCode fork did not preserve the selected native prefix".into(),
        ));
    }
    let mut aliases = BTreeMap::new();
    let mut child_ids = std::collections::BTreeSet::new();
    for (original, child) in source.iter().zip(cloned) {
        let original = &original["info"];
        let child = &child["info"];
        let original_id = original["id"].as_str().filter(|id| !id.is_empty());
        let child_id = child["id"].as_str().filter(|id| !id.is_empty());
        let (Some(original_id), Some(child_id)) = (original_id, child_id) else {
            return Err(Error::Provider(
                "OpenCode fork omitted message identity".into(),
            ));
        };
        if !matches!(original["role"].as_str(), Some("user" | "assistant"))
            || original["role"] != child["role"]
            || original["time"] != child["time"]
            || aliases.contains_key(original_id)
            || !child_ids.insert(child_id.to_string())
        {
            return Err(Error::Provider(
                "OpenCode fork returned an ambiguous native prefix".into(),
            ));
        }
        aliases.insert(original_id.to_string(), child_id.to_string());
    }
    if !inherited.is_null() {
        let inherited = inherited
            .as_object()
            .ok_or_else(|| Error::Invalid("runtime message aliases must be an object".into()))?;
        let direct = aliases.clone();
        for (canonical, source_id) in inherited {
            let source_id = source_id.as_str().ok_or_else(|| {
                Error::Invalid("runtime message aliases must contain string IDs".into())
            })?;
            if let Some(child_id) = direct.get(source_id) {
                if aliases
                    .get(canonical)
                    .is_some_and(|existing| existing != child_id)
                {
                    return Err(Error::Provider(
                        "OpenCode canonical message aliases conflict".into(),
                    ));
                }
                aliases.insert(canonical.clone(), child_id.clone());
            }
        }
    }
    Ok(aliases)
}

fn retained_aliases(
    history: &Value,
    reverted: Option<&str>,
    inherited: &Value,
) -> Result<BTreeMap<String, String>> {
    let messages = history
        .as_array()
        .ok_or_else(|| Error::Provider("OpenCode resumed messages were not an array".into()))?;
    let visible = if let Some(reverted) = reverted {
        messages
            .iter()
            .position(|message| message["info"]["id"] == reverted)
            .ok_or_else(|| {
                Error::Provider("OpenCode resumed history omitted its revert boundary".into())
            })?
    } else {
        messages.len()
    };
    let native_ids = messages[..visible]
        .iter()
        .filter_map(|message| message["info"]["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let inherited = inherited
        .as_object()
        .ok_or_else(|| Error::Invalid("runtime message aliases must be an object".into()))?;
    let mut aliases = BTreeMap::new();
    for (canonical, target) in inherited {
        let target = target.as_str().filter(|id| !id.is_empty()).ok_or_else(|| {
            Error::Invalid("runtime message aliases must contain nonempty string IDs".into())
        })?;
        if canonical.is_empty() || !native_ids.contains(target) {
            return Err(Error::Provider(
                "OpenCode resumed message aliases diverge from native history".into(),
            ));
        }
        aliases.insert(canonical.clone(), target.to_string());
    }
    Ok(aliases)
}
