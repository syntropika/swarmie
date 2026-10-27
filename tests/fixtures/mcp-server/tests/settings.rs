use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde_json::json;
use std::rc::Rc;
use swarmie_mcp::{
    Endpoint, Protocol, ServerId,
    settings::{
        Definition, MemoryDefinitions, SETTINGS_SERVICE, SaveDefinition, Scope, ServerAction,
        ServerStatus, settings_orb,
    },
};
use swarmie_tools::{
    self as tools, Catalog, CatalogMethod, OperationId, Recovery, ToolCall, ToolCallId, ToolName,
    ToolProvider, ToolResult,
};
use tembra::{Host, Reference};

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}
async fn call<A: 'static, R: 'static>(
    host: &Host,
    reference: &Rc<Reference>,
    method: &str,
    args: A,
) -> tembra::Result<R> {
    let reference = reference.clone();
    let method = method.to_owned();
    host.root("alice", move |context| {
        async move { reference.call(&context, &method, args).await }.boxed_local()
    })
    .await
}
fn definition(endpoint: Endpoint) -> Definition {
    Definition {
        id: ServerId::new("documents").unwrap(),
        name: "Documents".into(),
        scope: Scope::User,
        endpoint,
        protocol: Protocol::Current,
        enabled: false,
        revision: 1,
    }
}
fn action(definition: &Definition) -> (ServerAction,) {
    (ServerAction {
        id: definition.id.clone(),
        revision: definition.revision,
    },)
}

#[tokio::test(flavor = "current_thread")]
async fn settings_save_enable_edit_and_remove_use_real_stdio_and_http_and_preserve_old_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let headers =
                std::collections::BTreeMap::from([("x-project".into(), "documents".into())]);
            let (url, stop, serving) = swarmie_fixture_mcp_server::serve_http_with_headers(
                Default::default(),
                headers.clone(),
            )
            .await;
            for endpoint in [
                Endpoint::Local {
                    program: env!("CARGO_BIN_EXE_swarmie-fixture-mcp-server").into(),
                    arguments: vec![],
                    directory: None,
                },
                Endpoint::Http { url, headers },
            ] {
                let host = Host::new(Executor);
                let catalog = Catalog::default();
                let selected = catalog.clone();
                let store = Rc::new(MemoryDefinitions::default());
                host.install(vec![
                    settings_orb(host.clone(), store, move |_| Ok(selected.clone())),
                    tools::orb(host.clone(), catalog.clone()),
                ])
                .unwrap();
                let admin = Rc::new(
                    host.get(SETTINGS_SERVICE, vec!["main".into()], "alice")
                        .unwrap(),
                );
                let saved: Definition = call(
                    &host,
                    &admin,
                    "save",
                    (SaveDefinition {
                        definition: definition(endpoint),
                        expected_revision: None,
                    },),
                )
                .await
                .unwrap();
                assert!(
                    catalog.snapshot().unwrap().tools.is_empty(),
                    "saving performs no discovery"
                );
                let list: Vec<ServerStatus> = call(&host, &admin, "list", ()).await.unwrap();
                assert_eq!(list.len(), 1);
                assert_eq!(
                    list[0].observation.status,
                    swarmie_mcp::ConnectionStatus::Disconnected
                );
                let enabled: ServerStatus =
                    call(&host, &admin, "enable", action(&saved)).await.unwrap();
                assert_eq!(
                    enabled.observation.status,
                    swarmie_mcp::ConnectionStatus::Connected
                );
                assert_eq!(catalog.snapshot().unwrap().tools.len(), 3);
                let provider = catalog.snapshot().unwrap().tools[0].provider.clone();
                assert_eq!(provider.key, vec!["documents", "1"]);
                let tools = Rc::new(
                    host.get(tools::SERVICE, vec![tools::INSTANCE_KEY.into()], "alice")
                        .unwrap(),
                );
                let operation = OperationId::new();
                let intent = ToolCall {
                    id: ToolCallId::new("call").unwrap(),
                    name: ToolName::new("mcp.documents.counter.add").unwrap(),
                    arguments: json!({"amount": 2}),
                };
                let result: ToolResult = call(
                    &host,
                    &tools,
                    CatalogMethod::Execute.as_str(),
                    (operation, intent.clone(), provider.clone()),
                )
                .await
                .unwrap();
                assert!(matches!(result, ToolResult::Success(_)));
                let mut edited = saved.clone();
                edited.name = "Updated documents".into();
                let updated: Definition = call(
                    &host,
                    &admin,
                    "save",
                    (SaveDefinition {
                        definition: edited,
                        expected_revision: Some(1),
                    },),
                )
                .await
                .unwrap();
                assert_eq!(updated.revision, 2);
                assert!(!updated.enabled);
                assert!(catalog.snapshot().unwrap().tools.is_empty());
                let recovered: Recovery<ToolResult> = call(
                    &host,
                    &tools,
                    CatalogMethod::Recover.as_str(),
                    (operation, intent, provider),
                )
                .await
                .unwrap();
                assert_eq!(recovered, Recovery::Completed(result));
                let stale: tembra::Result<ServerStatus> =
                    call(&host, &admin, "enable", action(&saved)).await;
                assert!(stale.is_err());
                let _: ServerStatus = call(&host, &admin, "enable", action(&updated))
                    .await
                    .unwrap();
                let new_provider: ToolProvider =
                    catalog.snapshot().unwrap().tools[0].provider.clone();
                assert_eq!(new_provider.key, vec!["documents", "2"]);
                let _: () = call(&host, &admin, "remove", action(&updated))
                    .await
                    .unwrap();
                assert!(catalog.snapshot().unwrap().tools.is_empty());
                let list: Vec<ServerStatus> = call(&host, &admin, "list", ()).await.unwrap();
                assert!(list.is_empty());
                host.close().await.unwrap();
            }
            stop.cancel();
            serving.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn settings_are_principal_private_and_failed_saves_do_not_replace_current_definitions() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let host = Host::new(Executor);
            host.install(vec![settings_orb(
                host.clone(),
                Rc::new(MemoryDefinitions::default()),
                |_| Ok(Catalog::default()),
            )])
            .unwrap();
            let admin = Rc::new(
                host.get(SETTINGS_SERVICE, vec!["main".into()], "alice")
                    .unwrap(),
            );
            let original = definition(Endpoint::Http {
                url: "http://127.0.0.1:9/mcp".into(),
                headers: Default::default(),
            });
            let _: Definition = call(
                &host,
                &admin,
                "save",
                (SaveDefinition {
                    definition: original.clone(),
                    expected_revision: None,
                },),
            )
            .await
            .unwrap();
            let stale: tembra::Result<Definition> = call(
                &host,
                &admin,
                "save",
                (SaveDefinition {
                    definition: original.clone(),
                    expected_revision: None,
                },),
            )
            .await;
            assert!(stale.is_err());
            let mut invalid = original;
            invalid.endpoint = Endpoint::Http {
                url: "http://user:password@example.com/".into(),
                headers: Default::default(),
            };
            let result: tembra::Result<Definition> = call(
                &host,
                &admin,
                "save",
                (SaveDefinition {
                    definition: invalid,
                    expected_revision: Some(1),
                },),
            )
            .await;
            assert!(result.is_err());
            let list: Vec<ServerStatus> = call(&host, &admin, "list", ()).await.unwrap();
            assert_eq!(list[0].definition.revision, 1);
            let bob = Rc::new(
                host.get(SETTINGS_SERVICE, vec!["main".into()], "bob")
                    .unwrap(),
            );
            let list = host
                .root("bob", move |context| {
                    async move { bob.call::<_, Vec<ServerStatus>>(&context, "list", ()).await }
                        .boxed_local()
                })
                .await
                .unwrap();
            assert!(list.is_empty());
            host.close().await.unwrap();
        })
        .await;
}

#[test]
fn custom_headers_reject_credentials_reserved_headers_duplicates_and_line_injection() {
    for headers in [
        std::collections::BTreeMap::from([("Authorization".into(), "Bearer secret".into())]),
        std::collections::BTreeMap::from([("X-Test".into(), "a\r\nb".into())]),
        std::collections::BTreeMap::from([
            ("X-Test".into(), "a".into()),
            ("x-test".into(), "b".into()),
        ]),
    ] {
        assert!(
            definition(Endpoint::Http {
                url: "http://example.com/mcp".into(),
                headers
            })
            .validate()
            .is_err()
        );
    }
    assert!(
        definition(Endpoint::Http {
            url: "http://example.com/mcp".into(),
            headers: std::collections::BTreeMap::from([("X-Project".into(), "documents".into())])
        })
        .validate()
        .is_ok()
    );
}
