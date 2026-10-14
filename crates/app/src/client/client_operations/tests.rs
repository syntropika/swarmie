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
            turns: vec![swarmie::mock::MockTurn {
                expected_text: None,
                steps: vec![swarmie::mock::MockStep::End {
                    outcome: swarmie::TurnOutcome::Completed,
                }],
            }],
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
        // Keep the large dispatcher future off each scenario's stack while preserving real dispatch.
        Box::pin(self.operations.handle(&self.target.identity.environment_id,
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params,"expectedRuntime":self.target}))).await
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
        let mut connection = if fork {
            self.inner.connect(options, None, false).await?
        } else {
            self.inner.connect(options, native, false).await?
        };
        connection.capabilities.fork = true.into();
        connection.capabilities.rollback = true.into();
        connection.capabilities.native_skill_inputs = true.into();
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
    async fn rollback_with(
        &self,
        _turns: u64,
        _scope: swarmie::RollbackScope,
    ) -> swarmie::Result<()> {
        self.attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
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

async fn settled(f: &Fixture, session: &Value) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot = f
                .raw("session/get", json!({"sessionId":session["id"]}))
                .await;
            if snapshot["result"]["sequence"].as_u64().unwrap()
                > session["sequence"].as_u64().unwrap()
                && snapshot["result"]["state"] == "idle"
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn authored_inputs_survive_view_loss_and_duplicate_keys_do_not_upload_or_resend() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let session = f.create().await;
    std::fs::write(root.path().join("sentinel"), "owned").unwrap();
    let params = json!({"sessionId":session["id"],"text":"Inspect originals","idempotencyKey":"authored",
        "contextItems":[{"id":"context","kind":"terminal","label":"Output","text":"selected"}],
        "attachments":[{"id":"image","name":"image.png","mime":"image/png","image":true,"data":"aW1hZ2U="},
            {"id":"binary","name":"document.pdf","mime":"application/pdf","image":false,"binary":true,"data":"JVBERi0="},
            {"id":"text","name":"notes.txt","mime":"text/plain","image":false,"data":"original notes"}]});
    let sent = f.request("client/sessionInput", params.clone()).await;
    assert_eq!(sent["result"]["receipt"]["delivery"], "accepted", "{sent}");
    let input = sent["result"]["input"].clone();
    assert_eq!(
        input["content"][0]["data"],
        "Inspect originals\n\nContext (terminal): Output\nselected"
    );
    assert_eq!(input["content"][1]["type"], "image");
    let asset = input["attachments"][1]["path"].as_str().unwrap();
    assert_eq!(std::fs::read(asset).unwrap(), b"%PDF-");
    assert_eq!(input["attachments"][1]["data"], "");
    let duplicate = f.request("client/sessionInput", params.clone()).await;
    assert_eq!(duplicate["result"]["input"], input);
    assert_eq!(
        duplicate["result"]["receipt"]["id"],
        sent["result"]["receipt"]["id"]
    );
    let mut changed = params;
    changed["text"] = "different".into();
    assert!(
        f.request("client/sessionInput", changed)
            .await
            .get("error")
            .is_some()
    );
    settled(&f, &session).await;
    assert_eq!(f.harness.prompts().await.len(), 1);
    let restarted = ClientOperations::new(
        f.nodes.clone(),
        ClientSubmissions::new(
            f.nodes.clone(),
            storage::SqliteSessionStore::new(root.path().join("client"))
                .await
                .unwrap(),
        ),
    );
    let restored = restarted.dispatch(&f.target.identity.environment_id,json!({"id":1,"method":"client/sessionProjection","expectedRuntime":f.target,"params":{"sessionId":session["id"]}})).await;
    assert_eq!(restored["result"]["entries"][0]["input"], input);
    assert_eq!(
        restored["result"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "message.user")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("sentinel")).unwrap(),
        "owned"
    );
}
#[tokio::test]
async fn rejected_input_is_validated_before_any_asset_write_or_provider_input() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let session = f.create().await;
    for (key, attachment, context) in [
        (
            "invalid-image",
            json!({"id":"a","name":"image.png","mime":"image/png","image":true,"data":"???"}),
            json!([]),
        ),
        (
            "invalid-name",
            json!({"id":"a","name":"../file.pdf","mime":"application/pdf","image":false,"binary":true,"data":"ZmlsZQ=="}),
            json!([]),
        ),
        (
            "unsupported-skill",
            json!({"id":"a","name":"file.pdf","mime":"application/pdf","image":false,"binary":true,"data":"ZmlsZQ=="}),
            json!([{"id":"skill","skill":{"name":"selected","path":"/skills/selected"}}]),
        ),
    ] {
        let response = f.request("client/sessionInput",json!({"sessionId":session["id"],"text":"input","attachments":[attachment],"contextItems":context,"idempotencyKey":key})).await;
        assert_eq!(
            response["error"]["data"]["effectKnowledge"], "none",
            "{response}"
        );
    }
    assert!(!root.path().join("workspace/assets").exists());
    assert!(f.harness.prompts().await.is_empty());
}
#[tokio::test]
async fn native_queue_edits_preserve_nontext_parts_from_the_authoritative_queue() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let session = f.create().await;
    let paused = f
        .request(
            "client/sessionQueue",
            json!({"sessionId":session["id"],"action":"pause","paused":true}),
        )
        .await;
    assert_eq!(paused["result"]["paused"], true);
    let queued = f.request("client/sessionInput",json!({"sessionId":session["id"],"delivery":"queue","text":"first","attachments":[{"id":"image","name":"image.png","mime":"image/png","image":true,"data":"aW1hZ2U="}]})).await;
    let id = queued["result"]["queue"]["items"][0]["id"].clone();
    assert!(id.is_string(), "{queued}");
    let edited = f
        .request(
            "client/sessionQueue",
            json!({"sessionId":session["id"],"action":"update","id":id,"text":"changed"}),
        )
        .await;
    assert_eq!(
        edited["result"]["items"][0]["prompt"]["content"],
        json!([{"type":"text","data":"changed"},{"type":"image","data":{"mime_type":"image/png","data":"aW1hZ2U="}}])
    );
    assert!(f.harness.prompts().await.is_empty());
    let removed = f
        .request(
            "client/sessionQueue",
            json!({"sessionId":session["id"],"action":"remove","id":id}),
        )
        .await;
    assert_eq!(removed["result"]["items"], json!([]));
    assert!(
        f.request(
            "client/sessionQueue",
            json!({"sessionId":session["id"],"action":"update","id":id,"text":"late"})
        )
        .await
        .get("error")
        .is_some()
    );
}
#[tokio::test]
async fn editing_recovers_assets_before_rollback_and_preserves_working_files() {
    let root = tempfile::tempdir().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f = Fixture::build(root.path(), false, Some(attempts.clone())).await;
    let session = f.create().await;
    let sent = f.request("client/sessionInput",json!({"sessionId":session["id"],"text":"original","idempotencyKey":"edit","attachments":[{"id":"file","name":"file.pdf","mime":"application/pdf","image":false,"binary":true,"data":"ZmlsZQ=="}]})).await;
    assert_eq!(sent["result"]["receipt"]["delivery"], "accepted", "{sent}");
    settled(&f, &session).await;
    let events = f
        .raw("session/transcript", json!({"sessionId":session["id"]}))
        .await;
    let sequence = events["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "message.user")
        .unwrap()["sequence"]
        .clone();
    let path = sent["result"]["input"]["attachments"][0]["path"]
        .as_str()
        .unwrap();
    let backup = format!("{path}.saved");
    std::fs::rename(path, &backup).unwrap();
    let intent =
        json!({"sessionId":session["id"],"action":"edit","messageId":format!("user-{sequence}")});
    let failure = f.request("client/sessionHistory", intent.clone()).await;
    assert!(failure.get("error").is_some(), "{failure}");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
    std::fs::rename(backup, path).unwrap();
    let success = f.request("client/sessionHistory", intent).await;
    assert_eq!(success["result"]["draft"]["draft"], "original", "{success}");
    assert_eq!(
        success["result"]["draft"]["attachments"][0]["data"],
        "ZmlsZQ=="
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(path).unwrap(), b"file");
    let stale = f
        .raw(
            "session/rollback",
            json!({"sessionId":session["id"],"turns":1,"expectedSequence":0}),
        )
        .await;
    assert!(stale.get("error").is_some());
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    let removed = f.request("client/sessionHistory",json!({"sessionId":session["id"],"action":"edit","messageId":format!("user-{sequence}")})).await;
    assert!(removed.get("error").is_some());
}

#[tokio::test]
async fn project_creation_and_file_saves_need_no_view_owned_registration() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), false).await;
    let project=f.request("client/projectCreate",json!({"source":"create","arguments":{"parent":root.path(),"name":"folder","initializeGit":false},"name":"Retained name"})).await;
    assert_eq!(project["result"]["name"], "Retained name", "{project}");
    let path = root.path().join("folder/notes.txt");
    std::fs::write(&path, "owned").unwrap();
    let arguments =
        json!({"projectId":root.path().join("folder"),"path":"notes.txt","content":"edited"});
    let rejected = f
        .request(
            "client/fileSave",
            json!({"extensionId":"workspace","arguments":arguments,"baseline":"old"}),
        )
        .await;
    assert_eq!(
        rejected["error"]["data"]["effectKnowledge"], "none",
        "{rejected}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "owned");
    let saved = f
        .request(
            "client/fileSave",
            json!({"extensionId":"workspace","arguments":arguments,"baseline":"owned"}),
        )
        .await;
    assert_eq!(saved["result"]["written"], true, "{saved}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "edited");
}
#[tokio::test]
async fn schedules_validate_before_launch_and_reuse_the_retained_session() {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture::new(root.path(), true).await;
    let invalid=f.request("client/schedulePut",json!({"launch":{"harness":"mock"},"schedule":{"text":"scheduled","nextRunAtMs":"invalid"}})).await;
    assert_eq!(invalid["error"]["data"]["effectKnowledge"], "none");
    assert!(f.harness.launches().await.is_empty());
    let next = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        + 60_000;
    let saved=f.request("client/schedulePut",json!({"launch":{"harness":"mock"},"schedule":{"id":"scheduled-task","text":"scheduled","nextRunAtMs":next,"enabled":true}})).await;
    assert_eq!(saved["result"]["created"], true, "{saved}");
    let updated=f.request("client/schedulePut",json!({"schedule":{"id":"scheduled-task","sessionId":saved["result"]["session"]["id"],"text":"updated","nextRunAtMs":next,"enabled":true}})).await;
    assert_eq!(updated["result"]["created"], false, "{updated}");
    assert_eq!(f.harness.launches().await.len(), 1);
}
#[tokio::test]
async fn history_without_authored_metadata_restores_native_images_and_skills() {
    let root = tempfile::tempdir().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f = Fixture::build(root.path(), false, Some(attempts)).await;
    let session = f.create().await;
    let sent=f.raw("session/prompt",json!({"sessionId":session["id"],"idempotencyKey":"outside-client","content":[{"type":"text","data":"native original"},{"type":"image","data":{"mime_type":"image/png","data":"aW1hZ2U="}},{"type":"skill","data":{"name":"selected","path":"/skills/selected"}}]})).await;
    assert_eq!(sent["result"]["delivery"], "accepted", "{sent}");
    settled(&f, &session).await;
    let transcript = f
        .raw("session/transcript", json!({"sessionId":session["id"]}))
        .await;
    let message = transcript["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "message.user")
        .unwrap();
    let edited=f.request("client/sessionHistory",json!({"sessionId":session["id"],"action":"edit","messageId":format!("user-{}",message["sequence"])})).await;
    assert_eq!(
        edited["result"]["draft"]["draft"], "native original",
        "{edited}"
    );
    assert_eq!(
        edited["result"]["draft"]["attachments"][0]["data"],
        "aW1hZ2U="
    );
    assert_eq!(
        edited["result"]["draft"]["contextItems"][0]["skill"]["path"],
        "/skills/selected"
    );
}

#[tokio::test]
async fn fork_retains_authored_context_and_attachments_for_later_native_editing() {
    let root = tempfile::tempdir().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f = Fixture::build(root.path(), false, Some(attempts)).await;
    let session = f.create().await;
    let sent = f.request("client/sessionInput", json!({"sessionId":session["id"],"text":"authored original","idempotencyKey":"fork-input",
        "contextItems":[{"id":"selection","kind":"file","label":"Original selection","text":"selected content"}],
        "attachments":[{"id":"file","name":"file.pdf","mime":"application/pdf","image":false,"binary":true,"data":"ZmlsZQ=="}]})).await;
    assert_eq!(sent["result"]["receipt"]["delivery"], "accepted", "{sent}");
    settled(&f, &session).await;
    let fork = f
        .request(
            "client/sessionHistory",
            json!({"sessionId":session["id"],"action":"fork"}),
        )
        .await;
    let projection = &fork["result"]["projection"];
    assert_eq!(projection["inputs"].as_array().unwrap().len(), 1, "{fork}");
    assert_eq!(projection["inputs"][0]["text"], "authored original");
    let reopened = f
        .request(
            "client/sessionOpen",
            json!({"sessionId":projection["snapshot"]["id"]}),
        )
        .await;
    assert_eq!(
        reopened["result"]["inputs"], projection["inputs"],
        "{reopened}"
    );
    let sequence = projection["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "message.user")
        .unwrap()["sequence"]
        .clone();
    let edited = f.request("client/sessionHistory",json!({"sessionId":projection["snapshot"]["id"],"action":"edit","messageId":format!("user-{sequence}")})).await;
    assert_eq!(
        edited["result"]["draft"]["draft"], "authored original",
        "{edited}"
    );
    assert_eq!(
        edited["result"]["draft"]["contextItems"],
        sent["result"]["input"]["contextItems"]
    );
    assert_eq!(
        edited["result"]["draft"]["attachments"][0]["data"],
        "ZmlsZQ=="
    );
}
