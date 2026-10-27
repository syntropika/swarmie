use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde_json::{Value, json};
use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use swarmie_mcp::{Configuration, Connection, Endpoint, Limits, Protocol, ServerId};
use swarmie_tools::{OperationId, Recovery, ToolCall, ToolCallId, ToolExecution, ToolName, Tools};
use tembra::Host;

#[derive(Clone, Copy)]
struct Executor;

impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

#[derive(Clone)]
struct WireServer {
    initializes: Arc<AtomicUsize>,
    effects: Arc<AtomicUsize>,
    version: &'static str,
}

/// Independent wire fixture: acknowledge an effect with an expired-session HTTP
/// status instead of its result. A transparent SDK replay would execute it twice.
async fn wire(State(state): State<WireServer>, Json(request): Json<Value>) -> Response {
    let result = match request["method"].as_str().unwrap() {
        "initialize" => {
            state.initializes.fetch_add(1, Ordering::SeqCst);
            json!({"protocolVersion":state.version,"capabilities":{"tools":{}},"serverInfo":{"name":"wire-fixture","version":"1"}})
        }
        "notifications/initialized" | "notifications/cancelled" => {
            return StatusCode::ACCEPTED.into_response();
        }
        "tools/list" => json!({"tools":[{"name":"effect","inputSchema":{"type":"object"}}]}),
        "tools/call" => {
            state.effects.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let mut response =
        Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response();
    if request["method"] == "initialize" {
        response
            .headers_mut()
            .insert("mcp-session-id", "synthetic-session".parse().unwrap());
    }
    response
}

async fn start(
    version: &'static str,
) -> (
    Configuration,
    WireServer,
    tokio_util::sync::CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let state = WireServer {
        initializes: Arc::default(),
        effects: Arc::default(),
        version,
    };
    let router = axum::Router::new()
        .route("/mcp", post(wire))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let shutdown = cancel.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    (
        Configuration {
            id: ServerId::new("wire").unwrap(),
            endpoint: Endpoint::Http {
                url: format!("http://{address}/mcp"),
                headers: Default::default(),
            },
            protocol: Protocol::Previous,
            limits: Limits::default(),
        },
        state,
        cancel,
        task,
    )
}

#[tokio::test(flavor = "current_thread")]
async fn session_expiration_after_an_effect_does_not_reinitialize_or_replay_the_call() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (config, state, cancel, server) = start("2025-11-25").await;
            let host = Host::new(Executor);
            let connection = Rc::new(
                host.root("alice", move |context| {
                    async move { Connection::connect(config, &context).await }.boxed_local()
                })
                .await
                .unwrap(),
            );
            let operation = OperationId::new();
            let intent = ToolCall {
                id: ToolCallId::new("effect-call").unwrap(),
                name: ToolName::new("mcp.wire.effect").unwrap(),
                arguments: json!({}),
            };
            for _ in 0..2 {
                let connection = connection.clone();
                let intent = intent.clone();
                assert!(
                    host.root("alice", move |context| async move {
                        connection
                            .execute(&context, ToolExecution::new(operation, &intent))
                            .await
                    }
                    .boxed_local())
                        .await
                        .is_err()
                );
            }
            let recovered = host
                .root("alice", {
                    let connection = connection.clone();
                    move |context| {
                        async move {
                            connection
                                .recover(&context, ToolExecution::new(operation, &intent))
                                .await
                        }
                        .boxed_local()
                    }
                })
                .await
                .unwrap();
            assert_eq!(recovered, Recovery::Unknown);
            assert_eq!(
                connection.observation().status,
                swarmie_mcp::ConnectionStatus::Unavailable
            );
            let fresh = host
                .root("alice", {
                    let connection = connection.clone();
                    move |context| {
                        async move {
                            connection
                                .execute(
                                    &context,
                                    ToolExecution::new(
                                        OperationId::new(),
                                        &ToolCall {
                                            id: ToolCallId::new("fresh-call").unwrap(),
                                            name: ToolName::new("mcp.wire.effect").unwrap(),
                                            arguments: json!({}),
                                        },
                                    ),
                                )
                                .await
                        }
                        .boxed_local()
                    }
                })
                .await
                .unwrap();
            assert!(matches!(fresh, swarmie_tools::ToolResult::Blocked(_)));
            assert_eq!(state.effects.load(Ordering::SeqCst), 1);
            assert_eq!(state.initializes.load(Ordering::SeqCst), 1);
            connection.close().await.unwrap();
            host.close().await.unwrap();
            cancel.cancel();
            server.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_negotiated_revision_is_rejected_before_tools_are_called() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (config, state, cancel, server) = start("2025-03-26").await;
            let host = Host::new(Executor);
            let result = host
                .root("alice", move |context| {
                    async move { Connection::connect(config, &context).await }.boxed_local()
                })
                .await;
            let Err(error) = result else {
                panic!("unsupported version was accepted");
            };
            assert_eq!(error.code, tembra::Code::Invalid);
            assert_eq!(state.effects.load(Ordering::SeqCst), 0);
            assert_eq!(state.initializes.load(Ordering::SeqCst), 1);
            host.close().await.unwrap();
            cancel.cancel();
            server.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_transport_failure_withdraws_catalog_admission_but_preserves_recovery() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use swarmie_mcp::{self as mcp, McpMethod, Observation, ServerBinding};
            use swarmie_tools::{
                self as tools, Catalog, CatalogMethod, Selection, ToolProvider, ToolResult,
            };
            let (config, state, cancel, task) = start("2025-11-25").await;
            let catalog = Catalog::default();
            let selected = catalog.clone();
            let host = Host::new(Executor);
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
            let server = Rc::new(
                host.get(mcp::SERVICE, vec!["wire".into()], "alice")
                    .unwrap(),
            );
            host.root("alice", {
                let server = server.clone();
                move |context| {
                    async move {
                        server
                            .call::<_, Observation>(&context, McpMethod::Connect.as_str(), ())
                            .await
                    }
                    .boxed_local()
                }
            })
            .await
            .unwrap();
            assert_eq!(catalog.snapshot().unwrap().tools.len(), 1);
            let router = Rc::new(
                host.get(tools::SERVICE, vec![tools::INSTANCE_KEY.into()], "alice")
                    .unwrap(),
            );
            let operation = OperationId::new();
            let intent = ToolCall {
                id: ToolCallId::new("effect-call").unwrap(),
                name: ToolName::new("mcp.wire.effect").unwrap(),
                arguments: json!({}),
            };
            let provider = ToolProvider::new(mcp::SERVICE, vec!["wire".into()]).unwrap();
            assert!(
                host.root("alice", {
                    let router = router.clone();
                    let intent = intent.clone();
                    let provider = provider.clone();
                    move |context| {
                        async move {
                            router
                                .call::<_, ToolResult>(
                                    &context,
                                    CatalogMethod::Execute.as_str(),
                                    (operation, intent, provider),
                                )
                                .await
                        }
                        .boxed_local()
                    }
                })
                .await
                .is_err()
            );
            let remaining: Selection = host
                .root("alice", {
                    let router = router.clone();
                    move |context| {
                        async move {
                            router
                                .call(&context, CatalogMethod::List.as_str(), ())
                                .await
                        }
                        .boxed_local()
                    }
                })
                .await
                .unwrap();
            assert!(remaining.tools.is_empty());
            let recovery: Recovery<ToolResult> = host
                .root("alice", {
                    let router = router.clone();
                    move |context| {
                        async move {
                            router
                                .call(
                                    &context,
                                    CatalogMethod::Recover.as_str(),
                                    (operation, intent, provider),
                                )
                                .await
                        }
                        .boxed_local()
                    }
                })
                .await
                .unwrap();
            assert_eq!(recovery, Recovery::Unknown);
            assert_eq!(state.effects.load(Ordering::SeqCst), 1);
            assert_eq!(state.initializes.load(Ordering::SeqCst), 1);
            host.close().await.unwrap();
            cancel.cancel();
            task.await.unwrap();
        })
        .await;
}
