use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use std::{path::PathBuf, rc::Rc};
use swarmie_client_node::NodeConfiguration;
use swarmie_providers::settings::ProviderCatalog;
use swarmie_runtime::{NODE_KEY, NODE_SERVICE, NodeId, NodeIdentity};
use tembra::Host;

fn install_fixture(host: &Host, configuration: NodeConfiguration) -> tembra::Result<()> {
    let identity = swarmie_runtime::node_orb(configuration.node.clone());
    let mut definitions = swarmie_client_node::orbs(host, configuration, None)?;
    definitions.push(identity);
    host.install(definitions)
}

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

#[tokio::test]
async fn storage_opens_only_when_declared_capabilities_activate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let profile = tempfile::tempdir().unwrap();
            let host = Host::new(Executor);
            let definitions = swarmie_client_node::orbs(
                &host,
                NodeConfiguration {
                    environment: None,
                    state: profile.path().into(),
                    node: NodeId::new("embedded-fixture").unwrap(),
                    oauth_host: uuid::Uuid::new_v4(),
                    principal: "embedded-user".into(),
                    directory_root: None,
                    git: None,
                    agent_git_authority: None,
                },
                None,
            )
            .unwrap();
            assert_eq!(std::fs::read_dir(profile.path()).unwrap().count(), 0);
            host.install(definitions).unwrap();
            assert_eq!(std::fs::read_dir(profile.path()).unwrap().count(), 0);
            swarmie_client_node::start(&host).await.unwrap();
            assert!(profile.path().join("conversations.sqlite").is_file());
            let session = host
                .get(
                    "swarmie.conversation.session",
                    vec!["lazy".into()],
                    "embedded-user",
                )
                .unwrap();
            host.root("embedded-user", move |context| {
                async move { session.call::<_, ()>(&context, "snapshot", ()).await }.boxed_local()
            })
            .await
            .unwrap();
            host.close().await.unwrap();
            assert_eq!(host.kernel().diagnostics().resources, 0);
        })
        .await;
}

#[tokio::test]
async fn invalid_storage_fails_in_managed_activation_and_preserves_existing_bytes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let profile = tempfile::tempdir().unwrap();
            let path = profile.path().join("conversations.sqlite");
            let original = b"existing invalid database; preserve this file";
            std::fs::write(&path, original).unwrap();
            let host = Host::new(Executor);
            let definitions = swarmie_client_node::orbs(
                &host,
                NodeConfiguration {
                    environment: None,
                    state: profile.path().into(),
                    node: NodeId::new("embedded-fixture").unwrap(),
                    oauth_host: uuid::Uuid::new_v4(),
                    principal: "embedded-user".into(),
                    directory_root: None,
                    git: None,
                    agent_git_authority: None,
                },
                None,
            )
            .unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), original);
            host.install(definitions).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), original);
            assert!(swarmie_client_node::start(&host).await.is_err());
            host.close().await.unwrap();
            assert_eq!(host.kernel().diagnostics().resources, 0);
            assert_eq!(std::fs::read(&path).unwrap(), original);
        })
        .await;
}

#[tokio::test]
async fn a_non_desktop_host_composes_services_with_its_own_identity_and_principal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let profile = tempfile::tempdir().unwrap();
            let host = Host::new(Executor);
            let invalid = install_fixture(
                &host,
                NodeConfiguration { environment: None,
                    state: PathBuf::from(profile.path()),
                    node: NodeId::new("embedded-fixture").unwrap(),
                    oauth_host: uuid::Uuid::nil(),
                    principal: "embedded-user".into(),
                    directory_root: None,
                    git: None,
                    agent_git_authority: None,
                },
            );
            assert!(invalid.is_err());
            assert!(
                host.get(NODE_SERVICE, vec![NODE_KEY.into()], "embedded-user")
                    .is_err()
            );
            install_fixture(
                &host,
                NodeConfiguration { environment: None,
                    state: PathBuf::from(profile.path()),
                    node: NodeId::new("embedded-fixture").unwrap(),
                    oauth_host: uuid::Uuid::new_v4(),
                    principal: "embedded-user".into(),
                    directory_root: None,
                    git: None,
                    agent_git_authority: None,
                },
            )
            .unwrap();
            swarmie_client_node::start(&host).await.unwrap();
            let identity = host
                .get(NODE_SERVICE, vec![NODE_KEY.into()], "embedded-user")
                .unwrap();
            let value: NodeIdentity = host
                .root("embedded-user", move |context| {
                    async move { identity.call(&context, "identify", ()).await }.boxed_local()
                })
                .await
                .unwrap();
            assert_eq!(value.node_id.as_str(), "embedded-fixture");
            assert_eq!(value.principal, "embedded-user");
            let settings = host
                .get(
                    "swarmie.providers.settings",
                    vec!["main".into()],
                    "embedded-user",
                )
                .unwrap();
            let catalog: ProviderCatalog = host
                .root("embedded-user", move |context| {
                    async move { settings.call(&context, "list", (None::<String>,)).await }
                        .boxed_local()
                })
                .await
                .unwrap();
            assert!(catalog.accounts.is_empty());
            assert_eq!(
                catalog
                    .choices
                    .iter()
                    .map(|choice| choice.label.as_str())
                    .collect::<Vec<_>>(),
                vec!["ChatGPT", "Claude", "Kimi", "Local"]
            );
            let denied = host
                .get(
                    "swarmie.providers.settings",
                    vec!["main".into()],
                    "other-user",
                )
                .unwrap();
            let result: tembra::Result<ProviderCatalog> = host
                .root("other-user", move |context| {
                    async move { denied.call(&context, "list", (None::<String>,)).await }
                        .boxed_local()
                })
                .await;
            assert!(result.is_err());
            assert!(
                host.get("swarmie.directories", vec!["main".into()], "embedded-user")
                    .is_err()
            );
            let index = profile.path().join("conversations.json");
            let initial = serde_json::to_vec(&serde_json::json!([{
                "id": "indexed-fixture", "title": "Confirmed title",
                "principal": "embedded-user", "node": "embedded-fixture",
                "account": "synthetic-account", "model": "synthetic-model", "effort": "",
                "archived": false, "pinned": false, "receipts": [], "phase": "completed"
            }]))
            .unwrap();
            std::fs::write(&index, &initial).unwrap();
            let replies = Rc::new(std::cell::RefCell::new(Vec::<serde_json::Value>::new()));
            let outgoing = replies.clone();
            let rpc = tembra::rpc::serve(host.clone(), tembra::rpc::Options::new("embedded-user".into(), |_, _| true), move |frame| {
                outgoing.borrow_mut().push(serde_json::from_slice(&frame).unwrap()); Ok(())
            });
            rpc.receive(&serde_json::to_vec(&serde_json::json!({"id":1,"op":"get","payload":{"service":"swarmie.conversation.catalog","key":["main"]}})).unwrap()).unwrap();
            let catalog = rpc_reply(&replies, 1).await;
            for (id, title, accepted) in [(2, "é".repeat(257), false), (3, "Confirmed title".into(), true)] {
                rpc.receive(&serde_json::to_vec(&serde_json::json!({"id":id,"op":"call","payload":{"reference":catalog["ok"]["reference"],"method":"update","arguments":["indexed-fixture",title,true,false]}})).unwrap()).unwrap();
                let result = rpc_reply(&replies, id).await;
                assert_eq!(result.get("error").is_none(), accepted, "{result}");
                if accepted {
                    assert_eq!(result["ok"]["title"], "Confirmed title");
                    assert_eq!(result["ok"]["pinned"], true);
                    assert!(result["ok"]["lifecycleWarning"].is_null());
                } else {
                    assert_eq!(std::fs::read(&index).unwrap(), initial);
                }
            }
            rpc.close();
            let stored: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            assert_eq!(stored[0]["title"], "Confirmed title");
            assert_eq!(stored[0]["pinned"], true);
            host.close().await.unwrap();
            assert_eq!(host.kernel().diagnostics().resources, 0);
        })
        .await;
}

#[tokio::test]
async fn an_embedding_without_agent_mutation_policy_contributes_only_git_reads() {
    struct Granted;
    impl swarmie_git::Authority for Granted {
        fn authorize_discovery(&self, _: &str, _: &std::path::Path) -> swarmie_git::Result<()> {
            Ok(())
        }
        fn authorize(&self, _: &str, _: &swarmie_git::Intent) -> swarmie_git::Result<()> {
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let profile = tempfile::tempdir().unwrap();
            let git = swarmie_git::GitService::new(
                swarmie_git::Git::new(
                    std::sync::Arc::new(swarmie_git::NativeProcess),
                    std::sync::Arc::new(
                        swarmie_git::DirectoryJournal::open(profile.path().join("operations"))
                            .unwrap(),
                    ),
                    std::sync::Arc::new(Granted),
                ),
                profile.path().join("git"),
            )
            .unwrap();
            let host = Host::new(Executor);
            install_fixture(
                &host,
                NodeConfiguration {
                    environment: None,
                    state: profile.path().into(),
                    node: NodeId::new("read-only-embedding").unwrap(),
                    oauth_host: uuid::Uuid::new_v4(),
                    principal: "embedded-user".into(),
                    directory_root: None,
                    git: Some(git),
                    agent_git_authority: None,
                },
            )
            .unwrap();
            swarmie_client_node::start(&host).await.unwrap();
            let tools = host
                .get(swarmie_tools::SERVICE, vec!["main".into()], "embedded-user")
                .unwrap();
            let selection: swarmie_tools::Selection = host
                .root("embedded-user", move |context| {
                    async move { tools.call(&context, "list", ()).await }.boxed_local()
                })
                .await
                .unwrap();
            assert_eq!(
                selection
                    .tools
                    .iter()
                    .map(|tool| tool.spec.name.as_str())
                    .collect::<Vec<_>>(),
                ["git_inspect"]
            );
            assert_eq!(selection.tools[0].provider.service, swarmie_git::SERVICE);
            host.close().await.unwrap();
        })
        .await;
}

async fn rpc_reply(
    replies: &Rc<std::cell::RefCell<Vec<serde_json::Value>>>,
    id: u64,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let index = replies.borrow().iter().position(|reply| reply["id"] == id);
        if let Some(index) = index {
            return replies.borrow_mut().remove(index);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture RPC response timed out"
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_non_desktop_host_observes_and_stops_a_held_turn_without_provider_dispatch() {
    use serde_json::json;
    use swarmie_durable_tasks::{
        self as tasks, Method, TaskId, TaskObservation, TaskRecord, TaskSpec, TaskStatus,
    };
    tokio::task::LocalSet::new().run_until(async {
        let profile = tempfile::tempdir().unwrap();
        let host = Host::new(Executor);
        install_fixture(&host, NodeConfiguration { environment: None,
            state: profile.path().into(), node: NodeId::new("embedded-fixture").unwrap(),
            oauth_host: uuid::Uuid::new_v4(), principal: "embedded-user".into(),
            directory_root: None, git: None, agent_git_authority: None,
        }).unwrap();
        let id = TaskId::new();
        let history_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"embedded-fixture/embedded-user/held-turn");
        let reference = Rc::new(host.get(tasks::SERVICE, vec![tasks::INSTANCE_KEY.into()], tasks::HOST_PRINCIPAL).unwrap());
        let scheduler = reference.clone();
        host.root("embedded-user", move |context| async move {
            scheduler.call::<_, TaskId>(&context, Method::Create.as_str(), (TaskSpec {
                id, definition: tasks::TaskDefinition::new("swarmie.conversation.turn", std::num::NonZeroU32::new(1).unwrap()).unwrap(),
                input: json!({"session":"held-turn","account":"unconfigured-account","model":"unconfigured-model","effort":"","binding":null,"context_fence":uuid::Uuid::new_v4(),
                    "request":{"at":{"conversation":history_id,"entry":null},"instructions":"","tools":[],"max_turns":1}}),
            },)).await
        }.boxed_local()).await.unwrap();
        let index = profile.path().join("conversations.json");
        // Refuse the handler's index read before model lookup. This is a retryable
        // storage error, not a terminal domain refusal or permission to replay.
        std::fs::create_dir(&index).unwrap();
        let scheduler = reference.clone();
        let mut waiting = Box::pin(host.root("embedded-user", move |context| async move {
            scheduler.call::<_, TaskObservation>(&context, Method::Wait.as_str(), (id,)).await
        }.boxed_local()));
        assert!(futures::poll!(&mut waiting).is_pending());
        swarmie_client_node::start(&host).await.unwrap();
        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), waiting).await.unwrap().unwrap();
        assert_eq!(observed, TaskObservation::Interrupted);
        std::fs::remove_dir(&index).unwrap();
        std::fs::write(&index, serde_json::to_vec(&json!([{
            "id":"held-turn","title":"Held turn","principal":"embedded-user","node":"embedded-fixture",
            "account":"unconfigured-account","model":"unconfigured-model","effort":"","archived":false,"pinned":false,
            "receipts":[],"phase":"working","task":id,"stopRequested":false
        }])).unwrap()).unwrap();
        let session = Rc::new(host.get("swarmie.conversation.session", vec!["held-turn".into()], "embedded-user").unwrap());
        for method in ["snapshot", "stop"] {
            let session = session.clone();
            let result = host.root("embedded-user", move |context| async move {
                session.call::<_, ()>(&context, method, ()).await
            }.boxed_local());
            tokio::time::timeout(std::time::Duration::from_secs(5), result).await.unwrap().unwrap();
            let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            assert_eq!(metadata[0]["phase"], if method == "snapshot" { "interrupted" } else { "stopped" });
        }
        let scheduler = reference.clone();
        let record: Option<TaskRecord> = host.root("embedded-user", move |context| async move {
            scheduler.call(&context, Method::Get.as_str(), (id,)).await
        }.boxed_local()).await.unwrap();
        let record = record.unwrap();
        assert_eq!(record.status, TaskStatus::Completed(json!({"status":"stop_requested"})));
        assert!(record.assignment.is_none());
        assert!(record.checkpoint.is_none(), "stopping the held turn must not enter the agent loop");
        host.close().await.unwrap();
        assert_eq!(host.kernel().diagnostics().resources, 0);
    }).await;
}

#[tokio::test]
async fn an_embedding_cannot_bind_a_user_to_the_reserved_host_principal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for principal in ["", swarmie_durable_tasks::HOST_PRINCIPAL] {
                let profile = tempfile::tempdir().unwrap();
                let host = Host::new(Executor);
                let result = install_fixture(
                    &host,
                    NodeConfiguration {
                        environment: None,
                        state: profile.path().into(),
                        node: NodeId::new("embedded-fixture").unwrap(),
                        oauth_host: uuid::Uuid::new_v4(),
                        principal: principal.into(),
                        directory_root: None,
                        git: None,
                        agent_git_authority: None,
                    },
                );
                assert_eq!(result.unwrap_err().code, tembra::Code::Invalid);
                assert!(!profile.path().join("conversations.sqlite").exists());
                assert!(
                    host.get(NODE_SERVICE, vec![NODE_KEY.into()], principal)
                        .is_err()
                );
                host.close().await.unwrap();
            }
        })
        .await;
}

#[tokio::test]
async fn execution_owner_refusal_settles_the_durable_turn_without_retry_or_model_dispatch() {
    use serde_json::json;
    use swarmie_client_node::{ConversationExecution, ExecutionOwner, ExecutionScope};
    use swarmie_durable_tasks::{
        self as tasks, Method, TaskId, TaskObservation, TaskRecord, TaskSpec, TaskStatus,
    };
    struct Refused(std::cell::Cell<usize>);
    impl ExecutionOwner for Refused {
        fn conversation(
            self: Rc<Self>,
            _: &str,
            _: uuid::Uuid,
        ) -> tembra::Result<Rc<dyn ConversationExecution>> {
            Ok(self)
        }
        fn tools(&self) -> tembra::Result<Vec<swarmie_tools::ToolDefinition>> {
            Ok(vec![])
        }
    }
    impl ConversationExecution for Refused {
        fn enter(&self, _: &tembra::Context) -> tembra::Result<ExecutionScope> {
            self.0.set(self.0.get() + 1);
            Err(tembra::Error::new(
                tembra::Code::Unavailable,
                "fixture.owner",
                "The native owner is unavailable.",
            ))
        }
    }
    tokio::task::LocalSet::new().run_until(async {
        let profile = tempfile::tempdir().unwrap();
        let host = Host::new(Executor);
        let owner = Rc::new(Refused(std::cell::Cell::new(0)));
        host.install(swarmie_client_node::orbs(&host, NodeConfiguration { environment: None,
            state: profile.path().into(), node: NodeId::new("embedded-fixture").unwrap(),
            oauth_host: uuid::Uuid::new_v4(), principal: "embedded-user".into(),
            directory_root: None, git: None, agent_git_authority: None,
        }, Some(owner.clone())).unwrap()).unwrap();
        let id = TaskId::new();
        let history_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"embedded-fixture/embedded-user/refused-turn");
        let scheduler = Rc::new(host.get(tasks::SERVICE, vec![tasks::INSTANCE_KEY.into()], tasks::HOST_PRINCIPAL).unwrap());
        let reference = scheduler.clone();
        host.root("embedded-user", move |context| async move {
            reference.call::<_, TaskId>(&context, Method::Create.as_str(), (TaskSpec {
                id, definition: tasks::TaskDefinition::new("swarmie.conversation.turn", std::num::NonZeroU32::new(1).unwrap()).unwrap(),
                input: json!({"session":"refused-turn","account":"unconfigured-account","model":"unconfigured-model","effort":"","binding":null,"context_fence":uuid::Uuid::new_v4(),
                    "request":{"at":{"conversation":history_id,"entry":null},"instructions":"","tools":[],"max_turns":1}}),
            },)).await
        }.boxed_local()).await.unwrap();
        let index = profile.path().join("conversations.json");
        std::fs::write(&index, serde_json::to_vec(&json!([{
            "id":"refused-turn","title":"Refused turn","principal":"embedded-user","node":"embedded-fixture",
            "account":"unconfigured-account","model":"unconfigured-model","effort":"","archived":false,"pinned":false,
            "receipts":[],"phase":"working","task":id,"stopRequested":false
        }])).unwrap()).unwrap();
        swarmie_client_node::start(&host).await.unwrap();
        let reference = scheduler.clone();
        let observation: TaskObservation = host.root("embedded-user", move |context| async move {
            reference.call(&context, Method::Wait.as_str(), (id,)).await
        }.boxed_local()).await.unwrap();
        assert_eq!(observation, TaskObservation::Settled);
        let mut metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
        assert_eq!(metadata[0]["phase"], "failed");
        let record: Option<TaskRecord> = host.root("embedded-user", move |context| async move {
            scheduler.call(&context, Method::Get.as_str(), (id,)).await
        }.boxed_local()).await.unwrap();
        assert_eq!(record.unwrap().status, TaskStatus::Completed(json!({"status":"settled","phase":"failed"})));
        swarmie_client_node::start(&host).await.unwrap();
        assert_eq!(owner.0.get(), 1, "a refused native owner must not become an automatic retry");
        let storage = Rc::new(swarmie_storage::sqlite::SqliteOwner::new(profile.path().join("conversations.sqlite")).conversations().unwrap());
        let history = swarmie_conversations::Conversations::new(storage.clone());
        host.root("embedded-user", move |context| async move {
            let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"embedded-fixture/embedded-user/unknown-turn").to_string().parse().unwrap();
            history.create(&context, id).await?;
            let at = history.get(&context, id).await?.unwrap().head;
            let record = swarmie_agent_loop::conversation::Record::Message(swarmie_agent_loop::Message::Assistant(swarmie_agent_loop::AssistantMessage {
            provider: None,
                content: String::new(),
                tool_calls: vec![swarmie_tools::ToolCall { id: swarmie_tools::ToolCallId::new("unrecognized-call")?, name: swarmie_tools::ToolName::new("vault_list")?, arguments: json!({}) }],
            }));
            history.append(&context, at, vec![swarmie_conversations::EntryDraft::encode(record)?]).await?;
            Ok(())
        }.boxed_local()).await.unwrap();
        metadata.as_array_mut().unwrap().push(json!({
            "id":"unknown-turn","title":"Unknown turn","principal":"embedded-user","node":"embedded-fixture",
            "account":"unconfigured-account","model":"unconfigured-model","effort":"","archived":false,"pinned":false,
            "receipts":[],"phase":"unknown"
        }));
        std::fs::write(&index, serde_json::to_vec(&metadata).unwrap()).unwrap();
        for method in ["snapshot", "acknowledge"] {
            let session = host.get("swarmie.conversation.session", vec!["unknown-turn".into()], "embedded-user").unwrap();
            host.root("embedded-user", move |context| async move {
                session.call::<_, ()>(&context, method, ()).await
            }.boxed_local()).await.unwrap();
            let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            assert_eq!(metadata[1]["phase"], if method == "snapshot" { "unknown" } else { "stopped" });
        }
        assert_eq!(owner.0.get(), 1, "history-only acknowledgement must not require native execution authority");
        swarmie_conversations::ConversationStorage::close(storage.as_ref()).await.unwrap();
        host.close().await.unwrap();
    }).await;
}
