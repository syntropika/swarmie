use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde_json::json;
use std::{num::NonZeroU32, rc::Rc, time::Duration};
use swarmie_agent_loop::{self as agent, Agent, ExecutionResult, FinishReason};
use swarmie_fixture_mcp_server::{Counter, serve_http};
use swarmie_mcp::{
    self as mcp, Configuration, Endpoint, Limits, Observation, Protocol, ServerBinding, ServerId,
};
use swarmie_providers::{
    AgentContext, AssistantMessage, Message, ModelResponse, OperationId, Recovery, ToolCall,
    ToolCallId, ToolName, ToolResult, Usage,
    mock::{MockModel, Step},
};
use swarmie_tools::{
    self as tools, Catalog, CatalogMethod, CatalogService, ToolExecution, ToolPreparation, Tools,
};
use tembra::{Context, Host, Reference};

#[derive(Clone, Copy)]
struct Executor;

impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

fn config(endpoint: Endpoint, protocol: Protocol) -> Configuration {
    Configuration {
        id: ServerId::new("fixture").unwrap(),
        endpoint,
        protocol,
        limits: Limits::default(),
    }
}

fn stdio() -> Endpoint {
    Endpoint::Local {
        program: env!("CARGO_BIN_EXE_swarmie-fixture-mcp-server").into(),
        arguments: vec![],
        directory: None,
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

fn tool_call(name: &str) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        name: ToolName::new(format!("mcp.fixture.{name}")).unwrap(),
        arguments: json!({"amount":7}),
    }
}

async fn mounted(configuration: Configuration, catalog: &Catalog) -> (Host, Rc<Reference>) {
    let host = Host::new(Executor);
    let private_catalog = catalog.clone();
    host.install(vec![
        mcp::orb(vec![ServerBinding {
            configuration,
            principal: "alice".into(),
            catalog: catalog.clone(),
        }])
        .unwrap(),
        tools::orb_with_catalog(host.clone(), move |principal| {
            if principal == "alice" {
                Ok(private_catalog.clone())
            } else {
                Ok(Catalog::default())
            }
        }),
    ])
    .unwrap();
    assert!(
        catalog.snapshot().unwrap().tools.is_empty(),
        "passive catalog inspection must not connect"
    );
    let reference = Rc::new(
        host.get(mcp::SERVICE, vec!["fixture".into()], "alice")
            .unwrap(),
    );
    reference.ready().await.unwrap();
    assert!(catalog.snapshot().unwrap().tools.is_empty());
    let connected: Observation = call(&host, &reference, mcp::McpMethod::Connect.as_str(), ())
        .await
        .unwrap();
    assert_eq!(connected.status, mcp::ConnectionStatus::Connected);
    (host, reference)
}

async fn qualify_agent(configuration: Configuration) {
    let expected_protocol = configuration.protocol;
    let catalog = Catalog::default();
    let (host, reference) = mounted(configuration, &catalog).await;
    let observation: Observation = call(&host, &reference, mcp::McpMethod::Describe.as_str(), ())
        .await
        .unwrap();
    assert_eq!(observation.protocol, Some(expected_protocol));
    assert_eq!(
        observation.tools.len(),
        3,
        "all three catalog pages must be read"
    );
    let selection = catalog.snapshot().unwrap();
    let mut original_call = tool_call("counter.add");
    original_call.arguments = json!({"amount":99});
    let script = Rc::new(MockModel::new([
        Step::Reply(ModelResponse {
            protection: swarmie_providers::SubmissionProtection::Unprotected,
            message: AssistantMessage {
                provider: None,
                content: String::new(),
                tool_calls: vec![original_call],
            },
            usage: Usage::default(),
        }),
        Step::Reply(ModelResponse {
            protection: swarmie_providers::SubmissionProtection::Unprotected,
            message: AssistantMessage {
                provider: None,
                content: "Completed".into(),
                tool_calls: vec![],
            },
            usage: Usage::default(),
        }),
    ]));
    let model = script.clone();
    let binding = host.clone();
    host.install(vec![agent::orb(move |declaration| {
        Ok(Agent::new(
            model.clone(),
            Rc::new(CatalogService::bind(
                &declaration,
                binding.clone(),
                tools::SERVICE,
                vec![tools::INSTANCE_KEY.into()],
            )?),
        ))
    })])
    .unwrap();
    let hooks = agent::hooks::Hooks::default();
    host.install(vec![
        tembra::Orb::new("fixture.mcp-arguments", 1)
            .requires(tembra::core::Requirement {
                orb: agent::SERVICE.into(),
                api_version: 1,
                methods: Default::default(),
                hooks: [agent::hooks::HookName::BeforeToolCall.as_str().into()].into(),
            })
            .contribute(
                agent::SERVICE,
                &hooks.before_tool_call,
                0,
                |_, mut value| {
                    value.call.arguments = json!({"amount":7});
                    async { Ok(value) }
                },
            ),
    ])
    .unwrap();
    let execution = Rc::new(
        host.get(agent::SERVICE, vec!["main".into()], "alice")
            .unwrap(),
    );
    let output: ExecutionResult = call(
        &host,
        &execution,
        agent::Method::Execute.as_str(),
        agent::Request {
            context: AgentContext {
                sources: None,
                instructions: "Use the selected MCP tool.".into(),
                messages: vec![Message::User {
                    content: "Add seven.".into(),
                }],
            },
            tools: selection.specs(),
            max_turns: NonZeroU32::new(3).unwrap(),
        },
    )
    .await
    .unwrap();
    let output = output.unwrap();
    assert_eq!(output.reason, FinishReason::Complete);
    assert!(
        matches!(&output.messages[0], Message::Assistant(message) if message.tool_calls[0].arguments == json!({"amount":99}))
    );
    let Message::Tool {
        call: id,
        result: ToolResult::Success(result),
    } = &output.messages[1]
    else {
        panic!("expected correlated MCP result");
    };
    assert_eq!(id.as_str(), "call-1");
    assert_eq!(result["structuredContent"]["amount"], 7);
    assert_eq!(result["structuredContent"]["effect_count"], 1);
    assert!(result.get("_meta").is_none());
    assert!(
        !serde_json::to_string(&output.messages)
            .unwrap()
            .contains("mcp-private-result")
    );
    assert!(
        !serde_json::to_string(&output.messages)
            .unwrap()
            .contains("nested-private-metadata")
    );
    assert_eq!(
        result["structuredContent"]["business"]["_meta"],
        "public-business-value"
    );
    #[cfg(target_os = "linux")]
    let pid = result["structuredContent"]["pid"].as_u64().unwrap();
    let model_calls = script.calls();
    assert_eq!(model_calls.len(), 2);
    assert!(model_calls[1].request.context.messages.iter().any(|message| matches!(message, Message::Tool { result: ToolResult::Success(value), .. } if value == result)));
    host.close().await.unwrap();
    assert!(catalog.snapshot().unwrap().tools.is_empty());
    let diagnostics = host.kernel().diagnostics();
    assert_eq!(
        (diagnostics.roots, diagnostics.calls, diagnostics.resources),
        (0, 0, 0)
    );
    #[cfg(target_os = "linux")]
    if pid != u64::from(std::process::id()) {
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "stdio child must be reaped before cleanup completes"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn local_process_and_http_tools_run_through_agent_loop_for_both_profiles() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for protocol in [Protocol::Current, Protocol::Previous] {
                qualify_agent(config(stdio(), protocol)).await;
                let counter = Counter::default();
                let (url, shutdown, server) = serve_http(counter.clone()).await;
                qualify_agent(config(
                    Endpoint::Http {
                        url,
                        headers: Default::default(),
                    },
                    protocol,
                ))
                .await;
                assert_eq!(counter.effects(), 1);
                shutdown.cancel();
                server.await.unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn routed_preparation_disable_and_known_failure_preserve_provider_and_outcomes() {
    tokio::task::LocalSet::new().run_until(async {
        let counter = Counter::default();
        let (url, shutdown, server) = serve_http(counter.clone()).await;
        let catalog = Catalog::default();
        let (host, reference) = mounted(config(Endpoint::Http { url, headers: Default::default() }, Protocol::Current), &catalog).await;
        let router = Rc::new(host.get(tools::SERVICE, vec![tools::INSTANCE_KEY.into()], "alice").unwrap());
        let definition = catalog.snapshot().unwrap().tools.into_iter().find(|tool| tool.spec.name.as_str().ends_with("counter.fail")).unwrap();
        let fail = tool_call("counter.fail");
        let prepared: ToolPreparation = call(&host, &router, CatalogMethod::Prepare.as_str(), (definition.spec, fail.clone())).await.unwrap();
        assert_eq!(prepared.provider, Some(definition.provider.clone()));
        let operation = OperationId::new();
        let result: ToolResult = call(&host, &router, CatalogMethod::Execute.as_str(), (operation, fail.clone(), definition.provider.clone())).await.unwrap();
        assert!(matches!(&result, ToolResult::Failed(message) if message.contains("synthetic known failure")));
        assert!(!serde_json::to_string(&result).unwrap().contains("nested-private-metadata"));
        call::<_, ()>(&host, &reference, mcp::McpMethod::Disable.as_str(), ()).await.unwrap();
        assert!(catalog.snapshot().unwrap().tools.is_empty());
        let restored: Recovery<ToolResult> = call(&host, &router, CatalogMethod::Recover.as_str(), (operation, fail.clone(), definition.provider.clone())).await.unwrap();
        assert_eq!(restored, Recovery::Completed(result));
        let blocked: ToolResult = call(&host, &router, CatalogMethod::Execute.as_str(), (OperationId::new(), fail, definition.provider)).await.unwrap();
        assert!(matches!(blocked, ToolResult::Blocked(_)));
        assert_eq!(counter.effects(), 0);
        host.close().await.unwrap();
        shutdown.cancel();
        server.await.unwrap();
    }).await;
}

async fn direct<R>(
    host: &Host,
    connection: Rc<mcp::Connection>,
    run: impl FnOnce(
        Rc<mcp::Connection>,
        Context,
    ) -> futures::future::LocalBoxFuture<'static, tembra::Result<R>>
    + 'static,
) -> tembra::Result<R> {
    host.root("alice", move |context| run(connection, context))
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn timed_out_effect_remains_unknown_and_the_same_operation_is_never_replayed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default();
            let (url, shutdown, server) = serve_http(counter.clone()).await;
            let mut configuration = config(
                Endpoint::Http {
                    url,
                    headers: Default::default(),
                },
                Protocol::Current,
            );
            configuration.limits.request_timeout = Duration::from_millis(80);
            let host = Host::new(Executor);
            let connection = Rc::new(
                host.root("alice", move |context| {
                    async move { mcp::Connection::connect(configuration, &context).await }
                        .boxed_local()
                })
                .await
                .unwrap(),
            );
            let operation = OperationId::new();
            for _ in 0..2 {
                let error = direct(&host, connection.clone(), move |connection, context| {
                    async move {
                        connection
                            .execute(
                                &context,
                                ToolExecution::new(operation, &tool_call("counter.lose")),
                            )
                            .await
                    }
                    .boxed_local()
                })
                .await
                .unwrap_err();
                assert_eq!(error.code, tembra::Code::Unavailable);
                assert_eq!(counter.effects(), 1);
            }
            let recovered = direct(&host, connection.clone(), move |connection, context| {
                async move {
                    connection
                        .recover(
                            &context,
                            ToolExecution::new(operation, &tool_call("counter.lose")),
                        )
                        .await
                }
                .boxed_local()
            })
            .await
            .unwrap();
            assert_eq!(recovered, Recovery::Unknown);
            let error = host
                .root("bob", {
                    let connection = connection.clone();
                    move |context| {
                        async move {
                            connection
                                .recover(
                                    &context,
                                    ToolExecution::new(operation, &tool_call("counter.lose")),
                                )
                                .await
                        }
                        .boxed_local()
                    }
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, tembra::Code::Invalid);
            assert_eq!(counter.effects(), 1);
            connection.close().await.unwrap();
            host.close().await.unwrap();
            shutdown.cancel();
            server.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn partial_registration_is_rolled_back_and_metadata_keeps_its_authority() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default();
            let (url, shutdown, server) = serve_http(counter.clone()).await;
            let configuration = config(
                Endpoint::Http {
                    url,
                    headers: Default::default(),
                },
                Protocol::Current,
            );
            let catalog = Catalog::default();
            let old = catalog
                .register(
                    swarmie_tools::ToolDefinition::new(
                        swarmie_tools::ToolSpec {
                            name: ToolName::new("mcp.fixture.counter.lose").unwrap(),
                            description: "Existing owner".into(),
                            input_schema: json!({"type":"object"}),
                        },
                        swarmie_tools::ToolProvider::new("fixture.original", vec!["main".into()])
                            .unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();
            let host = Host::new(Executor);
            host.install(vec![
                mcp::orb(vec![ServerBinding {
                    configuration,
                    principal: "alice".into(),
                    catalog: catalog.clone(),
                }])
                .unwrap(),
            ])
            .unwrap();
            assert!(
                host.get(mcp::SERVICE, vec!["fixture".into()], "bob")
                    .is_err()
            );
            let reference = host
                .get(mcp::SERVICE, vec!["fixture".into()], "alice")
                .unwrap();
            reference.ready().await.unwrap();
            assert!(
                call::<_, Observation>(
                    &host,
                    &Rc::new(reference),
                    mcp::McpMethod::Connect.as_str(),
                    ()
                )
                .await
                .is_err()
            );
            let remaining = catalog.snapshot().unwrap();
            assert_eq!(remaining.tools.len(), 1);
            assert_eq!(remaining.tools[0].provider.service, "fixture.original");
            assert_eq!(counter.effects(), 0);
            host.close().await.unwrap();
            drop(old);
            assert!(catalog.snapshot().unwrap().tools.is_empty());
            shutdown.cancel();
            server.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn private_result_metadata_is_available_only_with_the_original_intent() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let catalog = Catalog::default();
            let (host, reference) = mounted(config(stdio(), Protocol::Current), &catalog).await;
            let operation = OperationId::new();
            let intent = tool_call("counter.add");
            let result: ToolResult = call(
                &host,
                &reference,
                tools::ToolMethod::Execute.as_str(),
                (operation, intent.clone()),
            )
            .await
            .unwrap();
            assert!(
                !serde_json::to_string(&result)
                    .unwrap()
                    .contains("mcp-private-result")
            );
            let receipt: Option<serde_json::Value> = call(
                &host,
                &reference,
                mcp::McpMethod::Result.as_str(),
                (operation, intent.clone()),
            )
            .await
            .unwrap();
            assert!(
                serde_json::to_string(&receipt)
                    .unwrap()
                    .contains("mcp-private-result")
            );
            let mut changed = intent.clone();
            changed.arguments = json!({"amount":8});
            assert!(
                call::<_, Option<serde_json::Value>>(
                    &host,
                    &reference,
                    mcp::McpMethod::Result.as_str(),
                    (operation, changed)
                )
                .await
                .is_err()
            );
            let again: ToolResult = call(
                &host,
                &reference,
                tools::ToolMethod::Execute.as_str(),
                (operation, intent),
            )
            .await
            .unwrap();
            assert_eq!(again, result);
            host.close().await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_negotiation_and_incomplete_discovery_reap_the_local_process() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for mode in ["--stall-before-handshake", "--pid-file"] {
                let directory = tempfile::tempdir().unwrap();
                let pid_path = directory.path().join("process-id");
                let mut configuration = config(
                    Endpoint::Local {
                        program: env!("CARGO_BIN_EXE_swarmie-fixture-mcp-server").into(),
                        arguments: vec![mode.into(), pid_path.to_str().unwrap().into()],
                        directory: None,
                    },
                    Protocol::Current,
                );
                configuration.limits.connect_timeout = Duration::from_millis(200);
                configuration.limits.max_catalog_pages = 2;
                let host = Host::new(Executor);
                assert!(
                    host.root("alice", move |context| async move {
                        mcp::Connection::connect(configuration, &context).await
                    }
                    .boxed_local())
                        .await
                        .is_err()
                );
                host.close().await.unwrap();
                #[cfg(target_os = "linux")]
                {
                    let pid = std::fs::read_to_string(pid_path)
                        .unwrap()
                        .parse::<u32>()
                        .unwrap();
                    assert!(
                        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                        "failed connection must reap its process before returning"
                    );
                }
            }
        })
        .await;
}
