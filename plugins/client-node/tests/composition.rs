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
                },
            )
            .unwrap();
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
            let catalog = Rc::new(
                host.get(
                    "swarmie.conversation.catalog",
                    vec!["main".into()],
                    "embedded-user",
                )
                .unwrap(),
            );
            for (title, accepted) in [("é".repeat(257), false), ("Confirmed title".into(), true)] {
                let reference = catalog.clone();
                let result: tembra::Result<()> = host
                    .root("embedded-user", move |context| {
                        async move {
                            reference
                                .call(
                                    &context,
                                    "update",
                                    ("indexed-fixture".to_owned(), title, true, false),
                                )
                                .await
                        }
                        .boxed_local()
                    })
                    .await;
                assert_eq!(result.is_ok(), accepted);
                if !accepted {
                    assert_eq!(std::fs::read(&index).unwrap(), initial);
                }
            }
            let stored: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            assert_eq!(stored[0]["title"], "Confirmed title");
            assert_eq!(stored[0]["pinned"], true);
            host.close().await.unwrap();
            assert_eq!(host.kernel().diagnostics().resources, 0);
        })
        .await;
}
