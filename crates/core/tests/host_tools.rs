use async_trait::async_trait;
use futures_util::StreamExt;
use mpai::{
    mock::{MockHarness, MockScript, MockStep},
    *,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, Notify, mpsc};

#[derive(Clone)]
struct ToolHarness {
    mock: MockHarness,
    events: Arc<Mutex<Option<mpsc::Sender<BackendEvent>>>>,
}
#[async_trait]
impl Harness for ToolHarness {
    fn id(&self) -> &str {
        "mock"
    }
    async fn detect(&self) -> Result<Detection> {
        self.mock.detect().await
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        let mut connection = self.mock.connect(options, native, fork).await?;
        connection
            .capabilities
            .metadata
            .insert("mcpTransports".into(), json!(["stdio"]));
        let (tx, rx) = mpsc::channel(64);
        *self.events.lock().await = Some(tx.clone());
        let mut source = connection.events;
        tokio::spawn(async move {
            while let Some(event) = source.recv().await {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });
        connection.events = rx;
        Ok(connection)
    }
}
#[derive(Clone)]
struct GateTool {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    effects: Arc<AtomicUsize>,
    block: bool,
    invalid_result: bool,
}
#[async_trait]
impl RuntimeExtension for GateTool {
    fn id(&self) -> &str {
        "fixture"
    }
    fn tools(&self) -> Vec<HostTool> {
        vec![HostTool {
            id: "read".into(),
            description: "Read a controlled fixture".into(),
            mutates: false,
            open_world: false,
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
            result_schema: json!({"type":"object","properties":{"path":{"type":"string"},"bindingId":{"type":"string"}},"required":["path","bindingId"],"additionalProperties":false}),
        }]
    }
    async fn execute_tool(&self, _: &str, args: Value, context: &HostToolContext) -> Result<Value> {
        self.entered.notify_one();
        if self.block {
            self.release.notified().await;
        }
        context.check_cancelled()?;
        self.effects.fetch_add(1, Ordering::SeqCst);
        if self.invalid_result {
            Ok(json!({"unexpected":true}))
        } else {
            Ok(json!({"path":args["path"],"bindingId":context.working_context.binding_id}))
        }
    }
}
struct Fixture {
    root: PathBuf,
    runtime: Runtime,
    harness: ToolHarness,
    tool: GateTool,
}
impl Fixture {
    fn new(block: bool, invalid_result: bool) -> Self {
        let root = std::env::temp_dir().join(format!("host-tools-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let harness = ToolHarness {
            mock: MockHarness::new(
                MockScript::default()
                    .turn(vec![MockStep::Sleep {
                        milliseconds: 60_000,
                    }])
                    .turn(vec![MockStep::Sleep {
                        milliseconds: 60_000,
                    }]),
            ),
            events: Arc::default(),
        };
        let tool = GateTool {
            entered: Arc::default(),
            release: Arc::default(),
            effects: Arc::default(),
            block,
            invalid_result,
        };
        let runtime = Runtime::builder()
            .harness(harness.clone())
            .extension(tool.clone())
            .build();
        runtime
            .install_host_tool_transport(McpServer {
                name: "mpai_host".into(),
                transport: McpTransport::Stdio {
                    command: "unused".into(),
                    args: vec![],
                    env: Default::default(),
                },
            })
            .unwrap();
        Self {
            root,
            runtime,
            harness,
            tool,
        }
    }
    fn options(&self) -> SpawnOptions {
        let mut options = SpawnOptions::new("mock", &self.root);
        options.execution.host_tools = vec![HostToolGrant {
            extension_id: "fixture".into(),
            tool_id: "read".into(),
            argument_constraints: [("path".into(), vec![json!("allowed.txt")])].into(),
        }];
        options
    }
    async fn channel(&self) -> String {
        let launches = self.harness.mock.launches().await;
        match &launches.last().unwrap().execution.mcp_servers[0].transport {
            McpTransport::Stdio { env, .. } => env["MPAI_TOOL_CHANNEL"].clone(),
            _ => panic!(),
        }
    }
    async fn rpc(&self, token: &str, method: &str, params: Value) -> Result<Value> {
        self.runtime
            .invoke_host_tool_channel(token, method, params)
            .await
    }
    async fn prepare(&self, token: &str) -> String {
        self.rpc(
            token,
            "tools/prepare",
            json!({"name":"fixture__read","arguments":{"path":"allowed.txt"}}),
        )
        .await
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
async fn next_idle(agent: &Agent) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while agent.snapshot().await.turn.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn grants_validate_schema_authority_origin_and_private_channel_without_replay() {
    let f = Fixture::new(false, false);
    let agent = f.runtime.spawn(f.options()).await.unwrap();
    let token = f.channel().await;
    assert!(
        !serde_json::to_string(&agent.snapshot().await)
            .unwrap()
            .contains(&token)
    );
    assert!(
        agent
            .snapshot()
            .await
            .options
            .execution
            .mcp_servers
            .is_empty()
    );
    assert!(
        f.rpc(
            &token,
            "tools/prepare",
            json!({"name":"fixture__read","arguments":{"path":"allowed.txt"}})
        )
        .await
        .is_err()
    );
    assert!(f.rpc("foreign", "tools/list", json!({})).await.is_err());
    assert!(f.rpc(&token, "extension/invoke", json!({})).await.is_err());
    agent.prompt("hold").await.unwrap();
    for params in [
        json!({"name":"fixture__read","arguments":{"path":"outside.txt"}}),
        json!({"name":"fixture__read","arguments":{"path":"allowed.txt","sessionId":"foreign"}}),
        json!({"name":"fixture__read","arguments":{"path":7}}),
        json!({"name":"unknown","arguments":{"path":"allowed.txt"}}),
    ] {
        assert!(f.rpc(&token, "tools/prepare", params).await.is_err());
    }
    assert_eq!(f.tool.effects.load(Ordering::SeqCst), 0);
    let id = f.prepare(&token).await;
    let result = f
        .rpc(&token, "tools/execute", json!({"invocationId":id}))
        .await
        .unwrap();
    assert_eq!(
        result["origin"],
        json!({"extensionId":"fixture","toolId":"read"})
    );
    assert_eq!(result["invocation"]["sessionId"], agent.snapshot().await.id);
    assert_eq!(
        result["invocation"]["turnId"],
        agent.snapshot().await.turn.unwrap()
    );
    assert_eq!(
        result["result"]["bindingId"],
        agent.snapshot().await.working_context.unwrap().binding_id
    );
    assert_eq!(result["invocation"]["state"], "completed");
    assert!(
        f.rpc(&token, "tools/execute", json!({"invocationId":id}))
            .await
            .is_err()
    );
    assert_eq!(f.tool.effects.load(Ordering::SeqCst), 1);
    agent.close().await.unwrap();
    assert!(!f.runtime.host_tool_channel(&token));
}
#[tokio::test]
async fn cancel_stops_running_work_and_old_prepared_calls_cannot_enter_the_next_turn() {
    let f = Fixture::new(true, false);
    let agent = f.runtime.spawn(f.options()).await.unwrap();
    let token = f.channel().await;
    agent.prompt("first").await.unwrap();
    let prepared = f.prepare(&token).await;
    let running = f.prepare(&token).await;
    let runtime = f.runtime.clone();
    let channel = token.clone();
    let id = running.clone();
    let execution = tokio::spawn(async move {
        runtime
            .invoke_host_tool_channel(&channel, "tools/execute", json!({"invocationId":id}))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), f.tool.entered.notified())
        .await
        .unwrap();
    agent.cancel().await.unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), execution)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Cancelled(_))
    ));
    f.tool.release.notify_waiters();
    assert_eq!(f.tool.effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.rpc(&token, "tools/status", json!({"invocationId":running}))
            .await
            .unwrap()["state"],
        "cancelled"
    );
    next_idle(&agent).await;
    agent.prompt("second").await.unwrap();
    assert!(
        f.rpc(&token, "tools/execute", json!({"invocationId":prepared}))
            .await
            .is_err()
    );
    let fresh = f.prepare(&token).await;
    assert_eq!(
        f.rpc(&token, "tools/cancel", json!({"invocationId":fresh}))
            .await
            .unwrap()["state"],
        "cancelled"
    );
    assert!(
        f.rpc(&token, "tools/execute", json!({"invocationId":fresh}))
            .await
            .is_err()
    );
    agent.close().await.unwrap();
}
#[tokio::test]
async fn missing_directory_and_invalid_results_fail_without_false_completion() {
    for missing in [true, false] {
        let f = Fixture::new(false, !missing);
        let agent = f.runtime.spawn(f.options()).await.unwrap();
        let token = f.channel().await;
        agent.prompt("hold").await.unwrap();
        let id = f.prepare(&token).await;
        let moved = f.root.with_extension("missing");
        if missing {
            std::fs::rename(&f.root, &moved).unwrap();
        }
        assert!(
            f.rpc(&token, "tools/execute", json!({"invocationId":id}))
                .await
                .is_err()
        );
        assert_eq!(
            f.rpc(&token, "tools/status", json!({"invocationId":id}))
                .await
                .unwrap()["state"],
            "failed"
        );
        assert_eq!(f.tool.effects.load(Ordering::SeqCst), usize::from(!missing));
        if missing {
            std::fs::rename(&moved, &f.root).unwrap();
        }
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn native_replacement_revokes_channels_and_cross_session_calls_have_no_authority() {
    let f = Fixture::new(false, false);
    let first = f.runtime.spawn(f.options()).await.unwrap();
    let a = f.channel().await;
    let second = f.runtime.spawn(f.options()).await.unwrap();
    let b = f.channel().await;
    first.prompt("hold").await.unwrap();
    second.prompt("hold").await.unwrap();
    let id = f.prepare(&a).await;
    assert!(
        f.rpc(&b, "tools/execute", json!({"invocationId":id}))
            .await
            .is_err()
    );
    let mut events = second.events();
    f.harness
        .events
        .lock()
        .await
        .as_ref()
        .unwrap()
        .send(BackendEvent::NativeSessionChanged(NativeSessionRef {
            provider: "mock".into(),
            id: "replacement".into(),
        }))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while second.snapshot().await.native.id != "replacement" {
            events.next().await.unwrap().unwrap();
        }
    })
    .await
    .unwrap();
    assert!(!f.runtime.host_tool_channel(&b));
    assert!(f.runtime.host_tool_channel(&a));
    first.close().await.unwrap();
    second.close().await.unwrap();
}
#[tokio::test]
async fn unavailable_tools_or_conflicting_launch_policies_fail_before_native_launch() {
    let f = Fixture::new(false, false);
    let mut options = f.options();
    options.execution.tools = ToolPolicy::Disabled;
    assert!(f.runtime.spawn(options).await.is_err());
    let mut options = f.options();
    options.execution.host_tools[0].tool_id = "absent".into();
    assert!(f.runtime.spawn(options).await.is_err());
    let mut options = f.options();
    options
        .execution
        .host_tools
        .push(options.execution.host_tools[0].clone());
    assert!(f.runtime.spawn(options).await.is_err());
    assert!(f.harness.mock.launches().await.is_empty());
}
