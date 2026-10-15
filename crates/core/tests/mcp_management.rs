use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use std::sync::{Arc, Mutex};
use swarmie::{
    mock::{MockHarness, MockScript},
    *,
};

#[derive(Clone, Default)]
struct McpHarness {
    launches: Arc<Mutex<Vec<SpawnOptions>>>,
    script: MockScript,
    native_reads: Arc<std::sync::atomic::AtomicUsize>,
    native_mutations: Arc<std::sync::atomic::AtomicUsize>,
}
#[async_trait]
impl Harness for McpHarness {
    fn id(&self) -> &str {
        "mock"
    }
    async fn detect(&self) -> Result<Detection> {
        let mut detection = MockHarness::new(MockScript::default()).detect().await?;
        detection
            .capabilities
            .metadata
            .insert("mcpTransports".into(), json!(["stdio", "http"]));
        detection.capabilities.resume = true.into();
        Ok(detection)
    }
    async fn mcp_capabilities(&self, options: &SpawnOptions) -> Result<Capabilities> {
        let mut caps = self.detect().await?.capabilities;
        if options.execution.ephemeral {
            caps.resume =
                Capability::unavailable("Ephemeral sessions do not retain native history");
        }
        if options.config["external"] == true {
            caps.metadata.insert("mcpTransports".into(), json!([]));
            caps.metadata
                .insert("mcpConfigurationScope".into(), json!("external"));
        }
        Ok(caps)
    }
    async fn mcp_catalog(&self, _: &McpQuery) -> Result<Vec<McpEntry>> {
        self.native_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(vec![])
    }
    async fn mcp_mutate(&self, _: &McpMutation) -> Result<Option<McpEntry>> {
        self.native_mutations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(Error::Unsupported("fixture native mutation".into()))
    }
    async fn connect(
        &self,
        o: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        _: bool,
    ) -> Result<BackendConnection> {
        self.launches.lock().unwrap().push(o.clone());
        let mut c = MockHarness::new(self.script.clone())
            .connect(o, None, false)
            .await?;
        c.capabilities = self.mcp_capabilities(o).await?;
        if let Some(saved) = saved {
            c.native = saved.clone();
        }
        Ok(c)
    }
}
fn mutation(action: &str, entry: Option<&McpEntry>) -> McpMutation {
    serde_json::from_value(json!({"action":action,"id":entry.map(|e|&e.id),"expectedRevision":entry.map(|e|&e.revision),"name":"fixture","definition":{"transport":"stdio","command":"fixture-server","envVars":["MCP_TOKEN"]}})).unwrap()
}

#[tokio::test]
async fn registry_changes_compile_fresh_and_gate_existing_native_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let harness = McpHarness::default();
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let entry = runtime
        .mcp_mutate(mutation("install", None))
        .await
        .unwrap()
        .entry
        .unwrap();
    let mut options = SpawnOptions::new("mock", temp.path());
    options
        .execution
        .env
        .insert("MCP_TOKEN".into(), "private-value".into());
    let agent = runtime.spawn(options).await.unwrap();
    let id = agent.snapshot().await.id;
    let launches = harness.launches.lock().unwrap().clone();
    assert_eq!(launches[0].execution.mcp_servers.len(), 1);
    assert!(
        !serde_json::to_string(&agent.snapshot().await)
            .unwrap()
            .contains("private-value")
    );
    assert!(
        !serde_json::to_string(&runtime.extension_state("swarmie.mcp").await.unwrap())
            .unwrap()
            .contains("private-value")
    );
    let receipt = runtime
        .mcp_mutate(mutation("disable", Some(&entry)))
        .await
        .unwrap();
    assert_eq!(receipt.restart_sessions, vec![id.clone()]);
    assert!(matches!(
        agent.prompt("must reopen").await,
        Err(Error::Rejected(_))
    ));
    let reopened = runtime.mcp_reopen(&id).await.unwrap();
    assert!(
        harness.launches.lock().unwrap()[1]
            .execution
            .mcp_servers
            .is_empty()
    );
    assert!(reopened.snapshot().await.queue.paused);
    assert!(
        runtime
            .mcp_catalog(McpQuery::default())
            .await
            .unwrap()
            .restart_sessions
            .is_empty()
    );
    assert!(matches!(
        runtime.mcp_mutate(mutation("enable", Some(&entry))).await,
        Err(Error::Rejected(_))
    ));
    let disabled = receipt.entry.unwrap();
    let enabled = runtime
        .mcp_mutate(mutation("enable", Some(&disabled)))
        .await
        .unwrap()
        .entry
        .unwrap();
    let again = runtime.mcp_reopen(&id).await.unwrap();
    assert_eq!(
        harness.launches.lock().unwrap()[2]
            .execution
            .mcp_servers
            .len(),
        1
    );
    runtime
        .mcp_mutate(mutation("uninstall", Some(&enabled)))
        .await
        .unwrap();
    assert!(temp.path().is_dir());
    again.close().await.unwrap();
}

#[tokio::test]
async fn non_resumable_sessions_keep_accepting_prompts_and_queued_work_after_mcp_changes() {
    let temp = tempfile::tempdir().unwrap();
    let harness = McpHarness {
        script: MockScript::default()
            .expect("direct follow-up", vec![])
            .expect("queued follow-up", vec![]),
        ..Default::default()
    };
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let entry = runtime
        .mcp_mutate(mutation("install", None))
        .await
        .unwrap()
        .entry
        .unwrap();
    let mut options = SpawnOptions::new("mock", temp.path());
    options.execution.ephemeral = true;
    options
        .execution
        .env
        .insert("MCP_TOKEN".into(), "fixture".into());
    let agent = runtime.spawn(options).await.unwrap();
    let id = agent.snapshot().await.id;
    assert!(!agent.snapshot().await.capabilities.resume.is_available());
    assert!(matches!(
        runtime.mcp_reopen(&id).await,
        Err(Error::Unsupported(_))
    ));

    let receipt = runtime
        .mcp_mutate(mutation("disable", Some(&entry)))
        .await
        .unwrap();
    assert!(receipt.restart_sessions.is_empty());
    assert!(
        runtime
            .mcp_catalog(McpQuery::default())
            .await
            .unwrap()
            .restart_sessions
            .is_empty()
    );

    let mut events = agent.events();
    agent.prompt("direct follow-up").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if let EventKind::TurnEnded { outcome, .. } = event.unwrap().kind {
                assert_eq!(outcome, TurnOutcome::Completed);
                return;
            }
        }
        panic!("session ended before the direct follow-up completed");
    })
    .await
    .unwrap();
    let queued = agent.enqueue("queued follow-up").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if let EventKind::TurnEnded { outcome, .. } = event.unwrap().kind {
                assert_eq!(outcome, TurnOutcome::Completed);
                return;
            }
        }
        panic!("session ended before the queued follow-up completed");
    })
    .await
    .unwrap();
    let receipt = agent
        .submission(format!("queue:{}", queued.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.delivery, DeliveryKnowledge::Accepted);
    assert!(agent.snapshot().await.queue.items.is_empty());
    assert_eq!(harness.launches.lock().unwrap().len(), 1);
    agent.close().await.unwrap();
    let fresh = runtime
        .spawn(SpawnOptions::new("mock", temp.path()))
        .await
        .unwrap();
    assert!(
        harness.launches.lock().unwrap()[1]
            .execution
            .mcp_servers
            .is_empty()
    );
    fresh.close().await.unwrap();
}

#[tokio::test]
async fn tools_disabled_do_not_resolve_credentials_or_connect_managed_mcps() {
    let temp = tempfile::tempdir().unwrap();
    let harness = McpHarness::default();
    let runtime = Runtime::builder().harness(harness.clone()).build();
    runtime.mcp_mutate(mutation("install", None)).await.unwrap();
    let mut options = SpawnOptions::new("mock", temp.path());
    options.execution.tools = ToolPolicy::Disabled;
    let agent = runtime.spawn(options).await.unwrap();
    assert!(
        harness.launches.lock().unwrap()[0]
            .execution
            .mcp_servers
            .is_empty()
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn shared_registry_is_node_local_and_projects_filter_injection() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let harness = McpHarness::default();
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let mut install = mutation("install", None);
    install.scope = Some("project".into());
    install.query.cwd = Some(project.clone());
    runtime.mcp_mutate(install).await.unwrap();
    let outside = runtime
        .spawn(SpawnOptions::new("mock", temp.path()))
        .await
        .unwrap();
    assert!(
        harness.launches.lock().unwrap()[0]
            .execution
            .mcp_servers
            .is_empty()
    );
    outside.close().await.unwrap();
    assert!(
        runtime
            .mcp_catalog(McpQuery {
                cwd: Some(temp.path().into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(
        Runtime::builder()
            .harness(McpHarness::default())
            .build()
            .mcp_catalog(McpQuery::default())
            .await
            .unwrap()
            .entries
            .is_empty()
    );
}

#[tokio::test]
async fn reopen_rejects_running_turns_without_cancelling_them() {
    let temp = tempfile::tempdir().unwrap();
    let harness = McpHarness {
        script: MockScript::default()
            .turn(vec![swarmie::mock::MockStep::Sleep { milliseconds: 2000 }]),
        ..Default::default()
    };
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let entry = runtime
        .mcp_mutate(mutation("install", None))
        .await
        .unwrap()
        .entry
        .unwrap();
    let mut options = SpawnOptions::new("mock", temp.path());
    options
        .execution
        .env
        .insert("MCP_TOKEN".into(), "fixture".into());
    let agent = runtime.spawn(options).await.unwrap();
    agent.prompt("running").await.unwrap();
    runtime
        .mcp_mutate(mutation("disable", Some(&entry)))
        .await
        .unwrap();
    assert!(matches!(
        runtime.mcp_reopen(&agent.snapshot().await.id).await,
        Err(Error::Busy)
    ));
    assert_eq!(agent.snapshot().await.state, SessionState::Running);
    assert_eq!(harness.launches.lock().unwrap().len(), 1);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn external_contexts_do_not_read_or_edit_node_native_files_or_require_reopening() {
    let root = tempfile::tempdir().unwrap();
    let harness = McpHarness::default();
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let entry = runtime
        .mcp_mutate(mutation("install", None))
        .await
        .unwrap()
        .entry
        .unwrap();
    let mut options = SpawnOptions::new("mock", root.path());
    options.config = json!({"external":true});
    let agent = runtime.spawn(options).await.unwrap();
    let query = McpQuery {
        session_id: Some(agent.snapshot().await.id.clone()),
        ..Default::default()
    };
    let catalog = runtime.mcp_catalog(query.clone()).await.unwrap();
    assert_eq!(catalog.entries[0].runtime_status, "unsupported");
    assert_eq!(
        harness
            .native_reads
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let mut native = mutation("install", None);
    native.origin = Some("native".into());
    native.query = query.clone();
    assert!(matches!(
        runtime.mcp_mutate(native).await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(
        harness
            .native_mutations
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let mut disable = mutation("disable", Some(&entry));
    disable.query = query;
    let receipt = runtime.mcp_mutate(disable).await.unwrap();
    assert!(receipt.restart_sessions.is_empty());
    agent.close().await.unwrap();
}
