use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use std::{path::PathBuf, rc::Rc};
use swarmie_client_node::{NodeConfiguration, install};
use swarmie_providers::settings::ProviderCatalog;
use swarmie_runtime::{NODE_KEY, NODE_SERVICE, NodeId, NodeIdentity};
use tembra::Host;

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

#[tokio::test]
async fn a_non_desktop_host_composes_services_with_its_own_identity_and_principal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let profile = tempfile::tempdir().unwrap();
            let host = Host::new(Executor);
            let invalid = install(
                &host,
                NodeConfiguration {
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
            install(
                &host,
                NodeConfiguration {
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
            install(
                &host,
                NodeConfiguration {
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
