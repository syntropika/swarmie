use async_trait::async_trait;
use protocol::RuntimeService;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use swarmie::*;
use tokio::sync::{Mutex, mpsc};

type Launch = (SessionExecutionOptions, Option<String>);
#[derive(Default)]
struct Setup(std::sync::atomic::AtomicBool);
#[async_trait]
impl AgentToolSetupPort for Setup {
    async fn descriptor(&self, harness: &str) -> Result<Value> {
        Ok(
            json!({"harness":harness,"installed":true,"authState":"authenticated","storage":{"persistent":false,"description":"Session only"},"providers":[]}),
        )
    }
    async fn start(&self, _: Value) -> Result<Value> {
        Ok(json!({"id":"flow","status":"connected"}))
    }
    async fn flow(&self, _: &str) -> Result<Value> {
        Ok(json!({"id":"flow","status":"connected"}))
    }
    async fn input(&self, _: &str, _: Value) -> Result<Value> {
        self.flow("flow").await
    }
    async fn cancel(&self, _: &str) -> Result<Value> {
        Ok(json!({"id":"flow","status":"cancelled"}))
    }
    async fn disconnect(&self, harness: &str) -> Result<Value> {
        self.0.store(true, std::sync::atomic::Ordering::Release);
        self.descriptor(harness).await
    }
    async fn execution(&self, _: &str) -> Result<AgentToolExecutionOverlay> {
        if self.0.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::Invalid("Agent tool is disconnected".into()));
        }
        Ok(AgentToolExecutionOverlay {
            env: BTreeMap::from([("TOKEN".into(), "account-secret".into())]),
            config: json!({"privateLaunchMarker":"runtime-only"}),
            ..Default::default()
        })
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn setup_credentials_cover_spawn_resume_fork_and_quota_without_public_persistence() {
    let peer = Peer::default();
    let runtime = Runtime::builder().harness(peer.clone()).build();
    let setup = Arc::new(Setup::default());
    let service = RuntimeService::new(runtime.clone()).with_agent_tools(setup);
    let created = call(
        &service,
        "session/create",
        json!({"harness":"fork-peer","cwd":std::env::current_dir().unwrap()}),
    )
    .await;
    assert!(created.get("error").is_none(), "{created}");
    let id = created["result"]["id"].as_str().unwrap();
    assert!(!created.to_string().contains("account-secret"));
    assert!(!created.to_string().contains("runtime-only"));
    let fork = call(&service, "session/fork", json!({"sessionId":id})).await;
    assert!(fork.get("error").is_none(), "{fork}");
    call(&service, "session/close", json!({"sessionId":id})).await;
    let resumed = call(&service, "session/resume", json!({"sessionId":id})).await;
    assert!(resumed.get("error").is_none(), "{resumed}");
    let quota = call(
        &service,
        "runtime/quotas",
        json!({"harness":"fork-peer","cwd":std::env::current_dir().unwrap()}),
    )
    .await;
    assert_eq!(quota["result"]["windows"][0]["usedPercent"], 25);
    assert_eq!(peer.0.lock().await.len(), 3);
    for (execution, _) in peer.0.lock().await.iter() {
        assert_eq!(execution.env["TOKEN"], "account-secret");
    }
    let conflict = call(&service,"session/create",json!({"harness":"fork-peer","cwd":std::env::current_dir().unwrap(),"execution":{"env":{"TOKEN":"other-private-secret"}}})).await;
    assert!(conflict.get("error").is_some());
    assert!(!conflict.to_string().contains("other-private-secret"));
    call(
        &service,
        "runtime/agentToolsDisconnect",
        json!({"harness":"fork-peer"}),
    )
    .await;
    assert!(
        call(&service, "session/resume", json!({"sessionId":id}))
            .await
            .get("error")
            .is_none()
    );
    assert!(
        call(&service, "session/fork", json!({"sessionId":id}))
            .await
            .get("error")
            .is_some()
    );
    assert!(
        !json!(runtime.sessions().await.unwrap())
            .to_string()
            .contains("account-secret")
    );
}
#[derive(Clone, Default)]
struct Peer(Arc<Mutex<Vec<Launch>>>);
struct Backend(mpsc::Sender<BackendEvent>);
#[async_trait]
impl Harness for Peer {
    fn id(&self) -> &str {
        "fork-peer"
    }
    async fn detect(&self) -> Result<Detection> {
        Err(Error::Unsupported("discovery".into()))
    }
    async fn account_quotas(&self, query: &AccountQuery) -> Result<Value> {
        if query.execution.env.get("TOKEN").map(String::as_str) != Some("account-secret") {
            return Err(Error::Provider("account credential missing".into()));
        }
        Ok(json!({"windows":[{"usedPercent":25}]}))
    }
    async fn connect(
        &self,
        o: &SpawnOptions,
        _: Option<&NativeSessionRef>,
        _: bool,
    ) -> Result<BackendConnection> {
        if !o.execution.env.contains_key("TOKEN") {
            return Err(Error::Provider("private launch credential missing".into()));
        }
        let mut launches = self.0.lock().await;
        launches.push((o.execution.clone(), None));
        let (tx, events) = mpsc::channel(8);
        Ok(BackendConnection {
            native: NativeSessionRef {
                provider: self.id().into(),
                id: format!("native-{}", launches.len()),
            },
            events,
            backend: Box::new(Backend(tx)),
            capabilities: Capabilities {
                fork_at_message: true.into(),

                fork: true.into(),
                resume: true.into(),
                metadata: BTreeMap::from([("mcpTransports".into(), json!(["stdio", "http"]))]),
                ..Default::default()
            },
        })
    }
    async fn fork_at(
        &self,
        o: &SpawnOptions,
        n: &NativeSessionRef,
        anchor: &str,
    ) -> Result<BackendConnection> {
        let connection = self.connect(o, Some(n), true).await?;
        self.0.lock().await.last_mut().unwrap().1 = Some(anchor.into());
        Ok(connection)
    }
}
#[async_trait]
impl AgentBackend for Backend {
    async fn prompt(&self, _: &Prompt) -> Result<()> {
        self.0
            .send(BackendEvent::content(
                EventKind::AssistantMessage(
                    json!({"id":"anchor","content":[{"type":"text","text":"authenticated"}]}),
                ),
                Value::Null,
            ))
            .await
            .map_err(|_| Error::Closed)?;
        self.0
            .send(BackendEvent::TurnFinished {
                outcome: TurnOutcome::Completed,

                native_reason: None,
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)
    }
    async fn cancel(&self) -> Result<()> {
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Err(Error::Unsupported("permission".into()))
    }
    async fn close(&self) -> Result<()> {
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        Ok(())
    }
}
fn execution(prefix: &str) -> Value {
    json!({"env":{"TOKEN":format!("{prefix}-process-secret")},"mcpServers":[
        {"name":"stdio","transport":{"type":"stdio","command":"peer","env":{"TOKEN":format!("{prefix}-stdio-secret")}}},
        {"name":"http","transport":{"type":"http","url":"https://example.test/mcp","headers":{"Authorization":format!("Bearer {prefix}-http-secret")}}}
    ]})
}
#[tokio::test]
async fn account_rpc_is_sessionless_and_preserves_independent_aggregate_errors() {
    let peer = Peer::default();
    let runtime = Runtime::builder().harness(peer.clone()).build();
    let service = RuntimeService::new(runtime.clone());
    let query = json!({"harness":"fork-peer","cwd":std::env::current_dir().unwrap(),"execution":{"env":{"TOKEN":"account-secret"}}});
    let result = call(&service, "runtime/quotas", query.clone()).await;
    assert_eq!(result["result"]["windows"][0]["usedPercent"], 25);
    assert!(!result.to_string().contains("account-secret"));
    let mut invalid = query.clone();
    invalid["execution"]["mcpServers"] = json!([]);
    assert_eq!(
        call(&service, "runtime/quotas", invalid).await["error"]["code"],
        -32602
    );
    let result = call(&service, "runtime/quotas/all", json!({"queries":[query,{"harness":"fork-peer","cwd":std::env::current_dir().unwrap()},{"harness":"missing","cwd":std::env::current_dir().unwrap()}]})).await;
    assert_eq!(result["result"][0]["status"], "available");
    assert_eq!(result["result"][1]["status"], "error");
    assert_eq!(result["result"][2]["harness"], "missing");
    assert!(peer.0.lock().await.is_empty());
    assert!(runtime.sessions().await.unwrap().is_empty());
}
async fn call(service: &RuntimeService, method: &str, params: Value) -> Value {
    service
        .handle(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .await
        .unwrap()
}

#[tokio::test]
async fn rpc_controls_extension_created_handles_and_uses_reopened_runtime_handles() {
    let runtime = Runtime::builder().harness(Peer::default()).build();
    let service = RuntimeService::new(runtime.clone());
    let mut options = SpawnOptions::new("fork-peer", std::env::current_dir().unwrap());
    options.execution = serde_json::from_value(execution("external")).unwrap();
    let external = runtime.spawn(options.clone()).await.unwrap();
    let id = external.snapshot().await.id;
    let prompted = call(
        &service,
        "session/prompt",
        json!({"sessionId":id,"text":"Extension-created session","idempotencyKey":"Extension-created session"}),
    )
    .await;
    assert!(prompted.get("error").is_none(), "{prompted}");
    tokio::time::timeout(Duration::from_secs(2), async {
        while runtime.snapshot(&id).await.unwrap().turns_completed != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let closed = call(&service, "session/close", json!({"sessionId":id})).await;
    assert!(closed.get("error").is_none(), "{closed}");
    assert!(
        call(
            &service,
            "session/prompt",
            json!({"sessionId":id,"text":"Closed","idempotencyKey":"Closed"})
        )
        .await
        .get("error")
        .is_some()
    );

    let created = call(
        &service,
        "session/create",
        json!({"harness":"fork-peer", "cwd":options.cwd, "execution":execution("external")}),
    )
    .await;
    assert!(created.get("error").is_none(), "{created}");
    let cached_id = created["result"]["id"].as_str().unwrap();
    runtime
        .agent(cached_id)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    let reopened = runtime
        .resume_with(cached_id, options.execution)
        .await
        .unwrap();
    let prompted = call(
        &service,
        "session/prompt",
        json!({"sessionId":cached_id,"text":"Fresh reopened handle","idempotencyKey":"Fresh reopened handle"}),
    )
    .await;
    assert!(prompted.get("error").is_none(), "{prompted}");
    let killed = call(&service, "agent/kill", json!({"sessionId":cached_id})).await;
    assert!(killed.get("error").is_none(), "{killed}");
    assert!(runtime.agent(&reopened.snapshot().await.id).await.is_none());
    runtime.shutdown().await.unwrap();
}
fn assert_credentials(execution: &SessionExecutionOptions, prefix: &str) {
    assert_eq!(execution.env["TOKEN"], format!("{prefix}-process-secret"));
    match &execution.mcp_servers[0].transport {
        McpTransport::Stdio { env, .. } => {
            assert_eq!(env["TOKEN"], format!("{prefix}-stdio-secret"))
        }
        _ => panic!("stdio transport changed"),
    }
    match &execution.mcp_servers[1].transport {
        McpTransport::Http { headers, .. } => assert_eq!(
            headers["Authorization"],
            format!("Bearer {prefix}-http-secret")
        ),
        _ => panic!("HTTP transport changed"),
    }
}

#[tokio::test]
async fn rpc_fork_inherits_live_secrets_and_accepts_replacement_execution_for_message_or_closed_forks()
 {
    let peer = Peer::default();
    let runtime = Runtime::builder().harness(peer.clone()).build();
    let service = RuntimeService::new(runtime.clone());
    let created=call(&service,"session/create",json!({"harness":"fork-peer","cwd":std::env::current_dir().unwrap(),"execution":execution("original")})).await;
    let id = created["result"]["id"].as_str().unwrap();
    assert!(
        call(
            &service,
            "session/prompt",
            json!({"sessionId":id,"text":"turn","idempotencyKey":"turn"})
        )
        .await
        .get("error")
        .is_none()
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while runtime.snapshot(id).await.unwrap().turns_completed != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let fork = call(&service, "session/fork", json!({"sessionId":id})).await;
    assert!(fork.get("error").is_none(), "{fork}");
    let at = call(
        &service,
        "session/fork",
        json!({"sessionId":id,"messageId":"anchor","execution":execution("replacement")}),
    )
    .await;
    assert!(at.get("error").is_none(), "{at}");
    {
        let launches = peer.0.lock().await;
        assert_credentials(&launches[1].0, "original");
        assert_credentials(&launches[2].0, "replacement");
        assert_eq!(launches[2].1.as_deref(), Some("anchor"));
    }
    call(&service, "session/close", json!({"sessionId":id})).await;
    assert!(
        call(&service, "session/fork", json!({"sessionId":id}))
            .await
            .get("error")
            .is_some()
    );
    let closed = call(
        &service,
        "session/fork",
        json!({"sessionId":id,"execution":execution("fresh")}),
    )
    .await;
    assert!(closed.get("error").is_none(), "{closed}");
    assert_credentials(&peer.0.lock().await.last().unwrap().0, "fresh");
    runtime.shutdown().await.unwrap();
    for response in [created, fork, at, closed] {
        let encoded = response.to_string();
        assert!(
            !encoded.contains("-process-secret")
                && !encoded.contains("-stdio-secret")
                && !encoded.contains("-http-secret")
        );
    }
    for snapshot in runtime.sessions().await.unwrap() {
        let journal = runtime.transcript(&snapshot.id, 0, 4096).await.unwrap();
        assert!(!serde_json::to_string(&journal).unwrap().contains("-secret"));
    }
}

#[tokio::test]
async fn fork_execution_input_errors_are_invalid_params_and_schema_keeps_optional_contract() {
    let service = RuntimeService::new(Runtime::default());
    for execution in [Value::Null, json!("invalid"), json!({"env":{"TOKEN":1}})] {
        assert_eq!(
            call(
                &service,
                "session/fork",
                json!({"sessionId":"missing","execution":execution})
            )
            .await["error"]["code"],
            -32602
        );
    }
    let schema = call(&service, "runtime/schema", json!({})).await["result"].clone();
    let properties = &schema["x-methods"]["session/fork"]["params"]["allOf"][1]["properties"];
    assert!(properties.get("execution").is_some());
    assert!(properties.get("messageId").is_some());
    assert_eq!(
        schema["x-methods"].as_object().unwrap().len(),
        protocol::METHODS.len()
    );
}
