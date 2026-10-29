use futures::{
    FutureExt,
    channel::oneshot,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde_json::json;
use std::{rc::Rc, time::Duration};
use swarmie_fixture_mcp_server::{Counter, serve_http};
use swarmie_mcp::{
    self as mcp, Configuration, ConnectionStatus, Endpoint, Limits, McpMethod, Observation,
    Protocol, ServerBinding, ServerId,
};
use swarmie_tools::{
    self as tools, Catalog, CatalogMethod, OperationId, Recovery, Selection, ToolCall, ToolCallId,
    ToolDecision, ToolName, ToolPreparation, ToolProvider, ToolResult,
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

fn configuration(endpoint: Endpoint) -> Configuration {
    Configuration {
        id: ServerId::new("fixture").unwrap(),
        endpoint,
        protocol: Protocol::Current,
        limits: Limits::default(),
    }
}

fn local(mode: &str, path: &std::path::Path) -> Endpoint {
    Endpoint::Local {
        program: env!("CARGO_BIN_EXE_swarmie-fixture-mcp-server").into(),
        arguments: vec![mode.into(), path.to_str().unwrap().into()],
        directory: None,
    }
}

fn intent() -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call").unwrap(),
        name: ToolName::new("mcp.fixture.counter.add").unwrap(),
        arguments: json!({"amount":7}),
    }
}

async fn invoke<A: 'static, R: 'static>(
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: &str,
    args: A,
) -> tembra::Result<R> {
    let reference = reference.clone();
    let method = method.to_owned();
    host.root(principal, move |context| {
        async move { reference.call(&context, &method, args).await }.boxed_local()
    })
    .await
}

fn install(config: Configuration, catalog: &Catalog) -> Host {
    let host = Host::new(Executor);
    let selected = catalog.clone();
    host.install(vec![
        mcp::orb(vec![ServerBinding {
            configuration: config,
            principal: "alice".into(),
            catalog: catalog.clone(),
        }])
        .unwrap(),
        tools::orb_with_catalog(host.clone(), move |principal| {
            if principal == "alice" {
                Ok(selected.clone())
            } else {
                Ok(Catalog::default())
            }
        }),
    ])
    .unwrap();
    host
}

fn reference(host: &Host, service: &str, key: &str, principal: &str) -> Rc<Reference> {
    Rc::new(host.get(service, vec![key.into()], principal).unwrap())
}

#[tokio::test(flavor = "current_thread")]
async fn recovery_and_result_never_launch_a_fresh_or_previously_disabled_process() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let pid_path = directory.path().join("process-id");
            let catalog = Catalog::default();
            let host = install(configuration(local("--pid-file", &pid_path)), &catalog);
            let provider = ToolProvider::new(mcp::SERVICE, vec!["fixture".into()]).unwrap();
            let router = reference(&host, tools::SERVICE, tools::INSTANCE_KEY, "alice");
            let recovered: Recovery<ToolResult> = invoke(
                &host,
                &router,
                "alice",
                CatalogMethod::Recover.as_str(),
                (OperationId::new(), intent(), provider.clone()),
            )
            .await
            .unwrap();
            assert_eq!(recovered, Recovery::Unknown);
            assert!(!pid_path.exists());
            let server = reference(&host, mcp::SERVICE, "fixture", "alice");
            let result: Option<serde_json::Value> = invoke(
                &host,
                &server,
                "alice",
                McpMethod::Result.as_str(),
                (OperationId::new(), intent()),
            )
            .await
            .unwrap();
            assert!(result.is_none());
            let status: Observation =
                invoke(&host, &server, "alice", McpMethod::Describe.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(status.status, ConnectionStatus::Disconnected);
            assert!(status.protocol.is_none());
            assert!(!pid_path.exists());
            let connected: Observation =
                invoke(&host, &server, "alice", McpMethod::Connect.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(connected.status, ConnectionStatus::Connected);
            assert!(pid_path.exists());
            invoke::<_, ()>(&host, &server, "alice", McpMethod::Disable.as_str(), ())
                .await
                .unwrap();
            let disabled: Observation =
                invoke(&host, &server, "alice", McpMethod::Describe.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(disabled.status, ConnectionStatus::Disabled);
            server.release().unwrap();
            router.release().unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while host.kernel().diagnostics().resources != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            std::fs::remove_file(&pid_path).unwrap();
            let fresh_router = reference(&host, tools::SERVICE, tools::INSTANCE_KEY, "alice");
            let recovered: Recovery<ToolResult> = invoke(
                &host,
                &fresh_router,
                "alice",
                CatalogMethod::Recover.as_str(),
                (OperationId::new(), intent(), provider),
            )
            .await
            .unwrap();
            assert_eq!(recovered, Recovery::Unknown);
            assert!(
                !pid_path.exists(),
                "recovery after physical disposal must not reopen the process"
            );
            assert!(catalog.snapshot().unwrap().tools.is_empty());
            host.close().await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_connect_reenables_a_live_disabled_connection_and_preserves_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default();
            let (url, cancel, task) = serve_http(counter.clone()).await;
            let catalog = Catalog::default();
            let host = install(
                configuration(Endpoint::Http {
                    url,
                    headers: Default::default(),
                }),
                &catalog,
            );
            let server = reference(&host, mcp::SERVICE, "fixture", "alice");
            let _: Observation = invoke(&host, &server, "alice", McpMethod::Connect.as_str(), ())
                .await
                .unwrap();
            let operation = OperationId::new();
            let result: ToolResult = invoke(
                &host,
                &server,
                "alice",
                tools::ToolMethod::Execute.as_str(),
                (operation, intent()),
            )
            .await
            .unwrap();
            invoke::<_, ()>(&host, &server, "alice", McpMethod::Disable.as_str(), ())
                .await
                .unwrap();
            let reacquired = reference(&host, mcp::SERVICE, "fixture", "alice");
            let status: Observation = invoke(
                &host,
                &reacquired,
                "alice",
                McpMethod::Describe.as_str(),
                (),
            )
            .await
            .unwrap();
            assert_eq!(status.status, ConnectionStatus::Disabled);
            assert!(catalog.snapshot().unwrap().tools.is_empty());
            let enabled: Observation =
                invoke(&host, &reacquired, "alice", McpMethod::Connect.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(enabled.status, ConnectionStatus::Connected);
            assert_eq!(catalog.snapshot().unwrap().tools.len(), 3);
            assert_eq!(
                counter.catalog_reads(),
                3,
                "reenabling must retain the live connection"
            );
            let again: ToolResult = invoke(
                &host,
                &server,
                "alice",
                tools::ToolMethod::Execute.as_str(),
                (operation, intent()),
            )
            .await
            .unwrap();
            assert_eq!(again, result);
            assert_eq!(counter.effects(), 1);
            host.close().await.unwrap();
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn principal_catalogs_are_isolated_even_with_equal_server_and_tool_names() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (url, cancel, task) = serve_http(Counter::default()).await;
            let alice = Catalog::default();
            let bob = Catalog::default();
            let host = Host::new(Executor);
            let for_alice = alice.clone();
            let for_bob = bob.clone();
            host.install(vec![
                mcp::orb(vec![
                    ServerBinding {
                        configuration: configuration(Endpoint::Http {
                            url: url.clone(),
                            headers: Default::default(),
                        }),
                        principal: "alice".into(),
                        catalog: alice.clone(),
                    },
                    ServerBinding {
                        configuration: configuration(Endpoint::Http {
                            url,
                            headers: Default::default(),
                        }),
                        principal: "bob".into(),
                        catalog: bob.clone(),
                    },
                ])
                .unwrap(),
                tools::orb_with_catalog(host.clone(), move |principal| match principal {
                    "alice" => Ok(for_alice.clone()),
                    "bob" => Ok(for_bob.clone()),
                    _ => Err(tembra::Error::invalid(
                        "fixture.principal",
                        "unbound principal",
                    )),
                }),
            ])
            .unwrap();
            let alice_server = reference(&host, mcp::SERVICE, "fixture", "alice");
            let _: Observation = invoke(
                &host,
                &alice_server,
                "alice",
                McpMethod::Connect.as_str(),
                (),
            )
            .await
            .unwrap();
            let bob_router = reference(&host, tools::SERVICE, tools::INSTANCE_KEY, "bob");
            let empty: Selection =
                invoke(&host, &bob_router, "bob", CatalogMethod::List.as_str(), ())
                    .await
                    .unwrap();
            assert!(empty.tools.is_empty());
            let definition = alice.snapshot().unwrap().tools.remove(0);
            let blocked: ToolPreparation = invoke(
                &host,
                &bob_router,
                "bob",
                CatalogMethod::Prepare.as_str(),
                (
                    definition.spec.clone(),
                    ToolCall {
                        name: definition.spec.name,
                        ..intent()
                    },
                ),
            )
            .await
            .unwrap();
            assert!(matches!(blocked.decision, ToolDecision::Block(_)));
            let bob_server = reference(&host, mcp::SERVICE, "fixture", "bob");
            let _: Observation = invoke(&host, &bob_server, "bob", McpMethod::Connect.as_str(), ())
                .await
                .unwrap();
            assert_eq!(bob.snapshot().unwrap().tools.len(), 3);
            invoke::<_, ()>(
                &host,
                &alice_server,
                "alice",
                McpMethod::Disable.as_str(),
                (),
            )
            .await
            .unwrap();
            assert!(alice.snapshot().unwrap().tools.is_empty());
            assert_eq!(bob.snapshot().unwrap().tools.len(), 3);
            host.close().await.unwrap();
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_during_local_handshake_and_http_discovery_settles_promptly() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let pid_path = directory.path().join("process-id");
            let counter = Counter::default().with_catalog_delay(Duration::from_secs(30));
            let (url, cancel, task) = serve_http(counter.clone()).await;
            for endpoint in [
                local("--stall-before-handshake", &pid_path),
                Endpoint::Http {
                    url,
                    headers: Default::default(),
                },
            ] {
                let is_local = matches!(endpoint, Endpoint::Local { .. });
                let mut config = configuration(endpoint);
                config.limits.connect_timeout = Duration::from_secs(10);
                let catalog = Catalog::default();
                let host = install(config, &catalog);
                let server = reference(&host, mcp::SERVICE, "fixture", "alice");
                let (send, receive) = oneshot::channel();
                let connecting = host.root("alice", move |context| {
                    assert!(send.send(context.clone()).is_ok());
                    async move {
                        server
                            .call::<_, Observation>(&context, McpMethod::Connect.as_str(), ())
                            .await
                    }
                    .boxed_local()
                });
                let cancelling = async {
                    let context = receive.await.unwrap();
                    if is_local {
                        while !pid_path.exists() {
                            tokio::task::yield_now().await;
                        }
                    } else {
                        counter.wait_for_catalog().await;
                    }
                    context.cancel().unwrap();
                };
                let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
                    futures::join!(connecting, cancelling)
                })
                .await
                .unwrap();
                assert_eq!(result.unwrap_err().code, tembra::Code::Cancelled);
                tokio::time::timeout(Duration::from_secs(2), host.close())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(catalog.snapshot().unwrap().tools.is_empty());
                if is_local {
                    let pid = std::fs::read_to_string(&pid_path)
                        .unwrap()
                        .parse::<u32>()
                        .unwrap();
                    #[cfg(target_os = "linux")]
                    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
                } else {
                    assert_eq!(counter.catalog_reads(), 1);
                }
            }
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn one_connection_deadline_bounds_the_entire_paginated_discovery() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default().with_catalog_delay(Duration::from_millis(100));
            let (url, cancel, task) = serve_http(counter.clone()).await;
            let mut config = configuration(Endpoint::Http {
                url,
                headers: Default::default(),
            });
            config.limits.connect_timeout = Duration::from_millis(150);
            config.limits.request_timeout = Duration::from_secs(5);
            let host = Host::new(Executor);
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                host.root("alice", move |context| {
                    async move { mcp::Connection::connect(config, &context).await }.boxed_local()
                }),
            )
            .await
            .unwrap();
            assert!(result.is_err());
            assert!((1..=2).contains(&counter.catalog_reads()));
            host.close().await.unwrap();
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn disabling_during_connect_prevents_late_catalog_publication() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default().with_catalog_delay(Duration::from_millis(15));
            let (url, cancel, task) = serve_http(counter.clone()).await;
            let catalog = Catalog::default();
            let host = install(
                configuration(Endpoint::Http {
                    url,
                    headers: Default::default(),
                }),
                &catalog,
            );
            let server = reference(&host, mcp::SERVICE, "fixture", "alice");
            let connecting =
                invoke::<_, Observation>(&host, &server, "alice", McpMethod::Connect.as_str(), ());
            let disabling = async {
                counter.wait_for_catalog().await;
                invoke::<_, ()>(&host, &server, "alice", McpMethod::Disable.as_str(), ())
                    .await
                    .unwrap();
            };
            let (result, ()) = futures::join!(connecting, disabling);
            assert!(result.is_err());
            assert!(catalog.snapshot().unwrap().tools.is_empty());
            let observation: Observation =
                invoke(&host, &server, "alice", McpMethod::Describe.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(observation.status, ConnectionStatus::Disabled);
            host.close().await.unwrap();
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_one_dispatched_call_keeps_the_connection_usable_without_replay() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default();
            let (url, cancel, task) = serve_http(counter.clone()).await;
            let catalog = Catalog::default();
            let mut config = configuration(Endpoint::Http {
                url,
                headers: Default::default(),
            });
            config.limits.request_timeout = Duration::from_millis(100);
            let host = install(config, &catalog);
            let server = reference(&host, mcp::SERVICE, "fixture", "alice");
            let _: Observation = invoke(&host, &server, "alice", McpMethod::Connect.as_str(), ())
                .await
                .unwrap();
            let router = reference(&host, tools::SERVICE, tools::INSTANCE_KEY, "alice");
            let provider = ToolProvider::new(mcp::SERVICE, vec!["fixture".into()]).unwrap();
            let operation = OperationId::new();
            let lost = ToolCall {
                name: ToolName::new("mcp.fixture.counter.lose").unwrap(),
                ..intent()
            };
            let (send, receive) = oneshot::channel();
            let dispatching = host.root("alice", {
                let router = router.clone();
                let provider = provider.clone();
                let lost = lost.clone();
                move |context| {
                    assert!(send.send(context.clone()).is_ok());
                    async move {
                        router
                            .call::<_, ToolResult>(
                                &context,
                                CatalogMethod::Execute.as_str(),
                                (operation, lost, provider),
                            )
                            .await
                    }
                    .boxed_local()
                }
            });
            let cancelling = async {
                let context = receive.await.unwrap();
                while counter.effects() == 0 {
                    tokio::task::yield_now().await;
                }
                context.cancel().unwrap();
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
                futures::join!(dispatching, cancelling)
            })
            .await
            .unwrap();
            assert_eq!(result.unwrap_err().code, tembra::Code::Cancelled);
            let fresh: ToolResult = invoke(
                &host,
                &router,
                "alice",
                CatalogMethod::Execute.as_str(),
                (OperationId::new(), intent(), provider.clone()),
            )
            .await
            .unwrap();
            assert!(matches!(fresh, ToolResult::Success(_)));
            let recovered: Recovery<ToolResult> = invoke(
                &host,
                &router,
                "alice",
                CatalogMethod::Recover.as_str(),
                (operation, lost, provider),
            )
            .await
            .unwrap();
            assert_eq!(recovered, Recovery::Unknown);
            assert_eq!(
                counter.effects(),
                2,
                "cancelled effect remains real, with only one fresh execution"
            );
            assert_eq!(catalog.snapshot().unwrap().tools.len(), 3);
            let status: Observation =
                invoke(&host, &server, "alice", McpMethod::Describe.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(status.status, ConnectionStatus::Connected);
            host.close().await.unwrap();
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}
