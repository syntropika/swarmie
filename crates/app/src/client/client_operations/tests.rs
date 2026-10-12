use super::*;
use std::path::Path;
use swarmie::mock::{MockHarness, MockScript};

struct Fixture {
    operations: ClientOperations,
    nodes: NodeGateway,
    target: RuntimeIdentity,
    harness: MockHarness,
}
impl Fixture {
    async fn new(root: &Path, resources: bool) -> Self {
        Self::build(root, resources, None).await
    }
    async fn build(
        root: &Path,
        resources: bool,
        fault: Option<Arc<std::sync::atomic::AtomicUsize>>,
    ) -> Self {
        let harness = MockHarness::new(MockScript {
            configuration: vec![
                json!({"id":"alpha","label":"Alpha","type":"string","application":"next_turn","value":"before"}),
            ],
            ..Default::default()
        });
        let mut builder = swarmie::RuntimeBuilder::default()
            .store(storage::SqliteSessionStore::new(root).await.unwrap())
            .extension(
                swarmie_workspace::WorkspaceExtension::new()
                    .with_storage_root(root.join("workspace")),
            )
            .extension(
                swarmie_git::GitExtension::new(root.join("trees"))
                    .await
                    .unwrap(),
            );
        builder = if let Some(attempts) = fault {
            builder.harness(FailingHarness {
                inner: harness.clone(),
                attempts,
            })
        } else {
            builder.harness(harness.clone())
        };
        if resources {
            builder = builder.extension(
                swarmie_resources::ResourcesExtension::open(root.join("resources")).unwrap(),
            );
        }
        let runtime = builder.build();
        let target = runtime.identity().await.unwrap();
        let nodes = NodeGateway::new(root, protocol::RuntimeService::new(runtime))
            .await
            .unwrap();
        let submissions = ClientSubmissions::new(
            nodes.clone(),
            storage::SqliteSessionStore::new(root.join("client"))
                .await
                .unwrap(),
        );
        Self {
            operations: ClientOperations::new(nodes.clone(), submissions),
            nodes,
            target,
            harness,
        }
    }
    async fn request(&self, method: &str, params: Value) -> Value {
        self.operations.handle(&self.target.identity.environment_id,
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params,"expectedRuntime":self.target})).await
    }
    async fn raw(&self, method: &str, params: Value) -> Value {
        self.nodes.forward(&self.target.identity.environment_id,
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params,"expectedRuntime":self.target})).await
    }
    async fn create(&self) -> Value {
        let response = self
            .request("client/sessionCreate", json!({"harness":"mock"}))
            .await;
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
}

#[tokio::test]
async fn exact_target_and_invalid_intent_produce_no_project_or_harness_effects() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let mut stale = f.target.clone();
    stale.incarnation_id = "stale".into();
    let response = f.operations.handle(&f.target.identity.environment_id,
        json!({"id":1,"method":"client/projectOpen","params":{"path":root.path()},"expectedRuntime":stale})).await;
    assert_eq!(response["error"]["data"]["effectKnowledge"], "none");
    let response = f
        .request("client/projectOpen", json!({"path":root.path(),"name":""}))
        .await;
    assert_eq!(response["error"]["data"]["effectKnowledge"], "none");
    let response = f
        .request(
            "client/sessionCreate",
            json!({"harness":"mock","cwd":root.path()}),
        )
        .await;
    assert_eq!(response["error"]["data"]["effectKnowledge"], "none");
    assert_eq!(f.raw("project/list", json!({})).await["result"], json!([]));
    assert!(f.harness.launches().await.is_empty());
}

#[tokio::test]
async fn retained_projects_reopen_and_unavailable_folders_remain_in_catalog() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir(&folder).unwrap();
    let f = Fixture::new(root.path(), false).await;
    let opened = f
        .request(
            "client/projectOpen",
            json!({"path":folder,"name":"Retained"}),
        )
        .await;
    assert!(opened.get("error").is_none(), "{opened}");
    let reopened = f
        .request("client/projectOpen", json!({"path":folder}))
        .await;
    assert_eq!(opened["result"]["id"], reopened["result"]["id"]);
    assert_eq!(reopened["result"]["name"], "Retained");
    assert_eq!(
        f.raw("project/list", json!({})).await["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_dir(&folder).unwrap();
    let catalog = f.request("client/projectCatalog", json!({})).await;
    assert_eq!(catalog["result"][0]["id"], opened["result"]["id"]);
    assert!(catalog["result"][0]["error"].is_string(), "{catalog}");
}

#[tokio::test]
async fn launch_checks_project_revision_and_worktree_ownership_before_connecting() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let project = f
        .request("client/projectOpen", json!({"path":root.path()}))
        .await["result"]
        .clone();
    let stale = f
        .request(
            "client/sessionCreate",
            json!({"harness":"mock","projectId":project["id"],"projectRevision":0}),
        )
        .await;
    assert!(stale.get("error").is_some(), "{stale}");
    let unrelated = f.request("client/sessionCreate", json!({"harness":"mock","projectId":project["id"],"projectRevision":project["revision"],"directoryPath":"/unrelated"})).await;
    assert!(unrelated.get("error").is_some(), "{unrelated}");
    assert!(f.harness.launches().await.is_empty());
    let launched = f.request("client/sessionCreate", json!({"harness":"mock","projectId":project["id"],"projectRevision":project["revision"]})).await;
    assert!(launched.get("error").is_none(), "{launched}");
    assert_eq!(
        f.harness.launches().await[0].cwd,
        root.path().canonicalize().unwrap()
    );
}

#[tokio::test]
async fn projectless_launch_uses_destination_default_directory() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("chats");
    std::fs::create_dir(&folder).unwrap();
    let f = Fixture::new(root.path(), false).await;
    let selected = f
        .request("client/chatDirectory", json!({"set":true,"path":folder}))
        .await;
    assert!(selected.get("error").is_none(), "{selected}");
    f.create().await;
    assert_eq!(
        f.harness.launches().await[0].cwd,
        folder.canonicalize().unwrap()
    );
}

#[tokio::test]
async fn partial_configuration_returns_observed_controls_and_preserves_error() {
    let root = tempfile::tempdir().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f = Fixture::build(root.path(), false, Some(attempts.clone())).await;
    let session = f.create().await;
    let response = f
        .request(
            "client/sessionConfigure",
            json!({"sessionId":session["id"],"config":{"alpha":"after"}}),
        )
        .await;
    assert!(response.get("error").is_some(), "{response}");
    assert_eq!(
        response["error"]["data"]["controls"]["values"]["alpha"],
        "after"
    );
    assert_eq!(response["error"]["data"]["effectKnowledge"], "unknown");
    let controls = f
        .request("client/sessionControls", json!({"sessionId":session["id"]}))
        .await;
    assert_eq!(controls["result"]["values"]["alpha"], "after");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn resources_resume_hydrates_post_attach_events_and_queue_without_prompting() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), true).await;
    let session = f.create().await;
    let response = f
        .request("client/sessionOpen", json!({"sessionId":session["id"]}))
        .await;
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["activation"]["status"], "attached");
    assert!(
        response["result"]["queue"]["items"].is_array(),
        "{response}"
    );
    let events = response["result"]["events"].as_array().unwrap();
    assert_eq!(
        events.last().unwrap()["sequence"],
        response["result"]["snapshot"]["sequence"]
    );
    assert!(f.harness.prompts().await.is_empty());
    assert_eq!(f.harness.launches().await.len(), 1);
}

#[tokio::test]
async fn git_commands_translate_retained_project_identity_and_manage_its_worktree() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("repository");
    std::fs::create_dir(&folder).unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&folder)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let f = Fixture::new(root.path(), false).await;
    let project = f
        .request("client/projectOpen", json!({"path":folder}))
        .await["result"]
        .clone();
    assert_eq!(project["git"], true);
    assert_ne!(project["id"], project["gitId"]);
    let listed = f.request("client/extensionInvoke", json!({"extensionId":"git","command":"worktree.list","arguments":{"projectId":project["id"]}})).await;
    assert!(listed["result"].is_array(), "{listed}");
    let created = f
        .request(
            "client/worktree",
            json!({"projectId":project["id"],"action":"create","branch":"feature"}),
        )
        .await;
    assert!(created.get("error").is_none(), "{created}");
    assert_eq!(created["result"]["worktrees"].as_array().unwrap().len(), 2);
    let launched = f.request("client/sessionCreate", json!({"harness":"mock","projectId":project["id"],"projectRevision":project["revision"],"directoryPath":created["result"]["tree"]["path"]})).await;
    assert!(launched.get("error").is_none(), "{launched}");
    assert_eq!(
        json!(f.harness.launches().await[0].cwd),
        created["result"]["tree"]["path"]
    );
    let busy = f.request("client/worktree", json!({"projectId":project["id"],"action":"remove","worktreeId":created["result"]["tree"]["id"]})).await;
    assert!(busy.get("error").is_some(), "{busy}");
    let closed = f
        .raw(
            "session/close",
            json!({"sessionId":launched["result"]["id"]}),
        )
        .await;
    assert!(closed.get("error").is_none(), "{closed}");
    let removed = f.request("client/worktree", json!({"projectId":project["id"],"action":"remove","worktreeId":created["result"]["tree"]["id"]})).await;
    assert!(removed.get("error").is_none(), "{removed}");
    assert_eq!(removed["result"]["worktrees"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn closed_sessions_are_hydrated_without_implicit_resume() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let session = f.create().await;
    let closed = f
        .raw("session/close", json!({"sessionId":session["id"]}))
        .await;
    assert!(closed.get("error").is_none(), "{closed}");
    let response = f
        .request("client/sessionOpen", json!({"sessionId":session["id"]}))
        .await;
    assert_eq!(response["result"]["activation"]["status"], "inactive");
    assert_eq!(response["result"]["snapshot"]["state"], "closed");
    let explicit = f
        .request(
            "client/sessionOpen",
            json!({"sessionId":session["id"],"resume":"explicit"}),
        )
        .await;
    assert_eq!(explicit["error"]["data"]["effectKnowledge"], "none");
    assert_eq!(f.harness.launches().await.len(), 1);
}

// Model a provider that applies an update but fails before acknowledging it.
struct FailingHarness {
    inner: MockHarness,
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}
#[async_trait::async_trait]
impl swarmie::Harness for FailingHarness {
    fn id(&self) -> &str {
        "mock"
    }
    async fn detect(&self) -> swarmie::Result<swarmie::Detection> {
        self.inner.detect().await
    }
    async fn connect(
        &self,
        options: &swarmie::SpawnOptions,
        native: Option<&swarmie::NativeSessionRef>,
        fork: bool,
    ) -> swarmie::Result<swarmie::BackendConnection> {
        let mut connection = self.inner.connect(options, native, fork).await?;
        connection.backend = Box::new(FailingBackend {
            inner: connection.backend,
            attempts: self.attempts.clone(),
        });
        Ok(connection)
    }
}
struct FailingBackend {
    inner: Box<dyn swarmie::AgentBackend>,
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}
#[async_trait::async_trait]
impl swarmie::AgentBackend for FailingBackend {
    async fn prompt(&self, prompt: &swarmie::Prompt) -> swarmie::Result<()> {
        self.inner.prompt(prompt).await
    }
    async fn cancel(&self) -> swarmie::Result<()> {
        self.inner.cancel().await
    }
    async fn respond_permission(
        &self,
        response: &swarmie::PermissionResponse,
    ) -> swarmie::Result<()> {
        self.inner.respond_permission(response).await
    }
    async fn close(&self) -> swarmie::Result<()> {
        self.inner.close().await
    }
    async fn kill(&self) -> swarmie::Result<()> {
        self.inner.kill().await
    }
    async fn configuration(&self) -> swarmie::Result<Vec<swarmie::SettingDescriptor>> {
        self.inner.configuration().await
    }
    async fn models(&self) -> swarmie::Result<Vec<swarmie::ModelDescriptor>> {
        self.inner.models().await
    }
    async fn configure(&self, config: Value) -> swarmie::Result<Value> {
        self.attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.configure(config).await?;
        Err(swarmie::Error::Provider(
            "connection lost after applying configuration".into(),
        ))
    }
}
