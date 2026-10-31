mod support;

#[path = "streaming/mod.rs"]
mod streaming;

use futures::FutureExt;
use serde_json::json;
use std::{num::NonZeroU64, rc::Rc, time::Duration};
use support::*;
use swarmie_providers::ai::{self as provider, Provider};
use swarmie_providers::{Model, ModelMethod, ModelResponse, OperationId, Recovery};
use tembra::{Code, Host};
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn adjacent_user_messages_keep_their_original_boundaries_on_every_profile() {
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let server = Server::new(vec![Reply::ok(wire(kind, false))]).await;
                let connection = configuration(kind, &server.endpoint).connections.remove(0);
                let model = qualified_model(
                    connection,
                    key().model,
                    Rc::new(Secrets),
                    provider::ModelProtection::Unprotected,
                )
                .unwrap();
                let mut request = input(false);
                request.context.messages.insert(
                    0,
                    swarmie_providers::Message::User {
                        content: "Earlier explicit input".into(),
                    },
                );
                let host = Host::new(LocalExecutor);
                host.root("alice", move |context| {
                    async move { model.complete(&context, OperationId::new(), &request).await }
                        .boxed_local()
                })
                .await
                .unwrap();
                let calls = server.calls.borrow().clone();
                assert_eq!(calls.len(), 1);
                let messages = match kind {
                    Provider::OpenAiResponses => calls[0].body["input"].as_array().unwrap(),
                    Provider::Gemini => calls[0].body["contents"].as_array().unwrap(),
                    _ => calls[0].body["messages"].as_array().unwrap(),
                };
                let users: Vec<_> = messages
                    .iter()
                    .filter(|message| message["role"] == "user")
                    .collect();
                assert_eq!(users.len(), 2, "{kind:?}: {messages:?}");
                assert!(users[0].to_string().contains("Earlier explicit input"));
                assert!(users[1].to_string().contains("Add one."));
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn all_profiles_preserve_checkpoint_source_tool_adjacency_and_exact_prepared_bytes() {
    use sha2::{Digest, Sha256};
    use std::num::NonZeroU64;
    use swarmie_ai::catalog::{
        Capability, Catalog, Evidence, Layer, LimitClaim, Limits, Precision, Preparation,
        Requirements, SourceRecord,
    };
    use swarmie_context::{ArtifactRef, Configuration, Plan, Projection, Strategy, textual};
    use swarmie_conversations::{ConversationId, EntryId, Head, HistoryPoint};
    use swarmie_providers::{
        AssistantMessage, ComponentKind, Message, SubmissionProtection, ToolCall, ToolCallId,
        ToolName, ToolResult, Usage,
    };
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let server = Server::new(vec![Reply::ok(wire(kind, false))]).await;
                let connection = configuration(kind, &server.endpoint).connections.remove(0);
                let model_id = key().model;
                let evidence = Evidence {
                    source: "synthetic exact-route HTTP fixture".into(),
                    revision: "1".into(),
                    layer: Layer::Adapter,
                    observed_at_ms: 0,
                    expires_at_ms: None,
                };
                let claim = |tokens| LimitClaim {
                    tokens: NonZeroU64::new(tokens).unwrap(),
                    precision: Precision::Reported,
                    evidence: Evidence {
                        layer: Layer::Provider,
                        ..evidence.clone()
                    },
                };
                let identity = connection.model_identity(&model_id).unwrap();
                let mut catalog = Catalog::new(4);
                catalog
                    .replace_sources(vec![SourceRecord {
                        identity: identity.clone(),
                        limits: Limits {
                            context: vec![claim(100_000)],
                            input: vec![],
                            output: vec![claim(4_000)],
                        },
                        tools: Capability::Supported {
                            evidence: evidence.clone(),
                            value: (),
                        },
                        streaming: Capability::default(),
                        native_output_maximum: Capability::default(),
                        native_compaction: Capability::Unsupported {
                            evidence: evidence.clone(),
                            reason: "Message-only synthetic fixture".into(),
                        },
                    }])
                    .unwrap();
                let metadata = catalog
                    .prepare(
                        Preparation {
                            identity,
                            configuration_revision: connection
                                .configuration_revision(&model_id, None)
                                .unwrap(),
                            requirements: Requirements {
                                context: true,
                                input: true,
                                output: true,
                                tools: true,
                                ..Requirements::default()
                            },
                            allow_stale_for_ms: 0,
                        },
                        0,
                    )
                    .unwrap();
                let model = qualified_model(
                    connection,
                    model_id,
                    Rc::new(Secrets),
                    provider::ModelProtection::Unprotected,
                )
                .unwrap()
                .with_metadata(metadata.clone())
                .unwrap();
                let conversation = ConversationId::new();
                let entry = EntryId::new();
                let projection = Projection {
                    operation: OperationId::new(),
                    plan: Plan {
                        through: HistoryPoint {
                            conversation,
                            entry,
                        },
                        previous: None,
                        configuration: Configuration([1; 32]),
                        strategy: Strategy::Textual {
                            model: Configuration([2; 32]),
                        },
                    },
                    artifact: ArtifactRef::new(
                        Head {
                            conversation,
                            entry: Some(EntryId::new()),
                        },
                        Some(entry),
                    )
                    .unwrap(),
                };
                let source = textual::project(
                    &projection,
                    &textual::Summary {
                        content: "Synthetic checkpoint source 🙂\"\\".into(),
                        usage: Usage {
                            input_tokens: 12345,
                            output_tokens: 7,
                        },
                        protection: SubmissionProtection::Clean,
                    },
                )
                .unwrap();
                let source_bytes = serde_json::to_vec(&source).unwrap().len();
                let mut request = input(true);
                request.context.messages.insert(0, source);
                request
                    .context
                    .messages
                    .push(Message::Assistant(AssistantMessage {
                        provider: None,
                        content: String::new(),
                        tool_calls: vec![ToolCall {
                            id: ToolCallId::new("call_1").unwrap(),
                            name: ToolName::new("counter_add").unwrap(),
                            arguments: json!({"amount":1}),
                        }],
                    }));
                request.context.messages.push(Message::Tool {
                    call: ToolCallId::new("call_1").unwrap(),
                    result: ToolResult::Success(json!({"value":1})),
                });
                let host = Host::new(LocalExecutor);
                let captured = server.calls.clone();
                let generation = metadata.generation().to_owned();
                let description = host
                    .root("alice", move |context| {
                        async move {
                            let handle = model
                                .prepare(&context, OperationId::new(), request, None)
                                .await?;
                            assert!(captured.borrow().is_empty());
                            let description = handle.description().clone();
                            assert_eq!(handle.execute().await?.message.content, "Done");
                            Ok(description)
                        }
                        .boxed_local()
                    })
                    .await
                    .unwrap();
                let calls = server.calls.borrow().clone();
                assert_eq!(calls.len(), 1);
                let captured = &calls[0];
                assert_eq!(
                    description.measurement.final_payload_bytes,
                    captured.raw_body.len() as u64
                );
                let origin = reqwest::Url::parse(&server.endpoint)
                    .unwrap()
                    .origin()
                    .ascii_serialization();
                let mut digest = Sha256::new();
                digest.update(generation.as_bytes());
                digest.update([0]);
                digest.update(format!("{origin}{}", captured.path).as_bytes());
                digest.update([0]);
                digest.update(&captured.raw_body);
                let actual: [u8; 32] = digest.finalize().into();
                assert_eq!(description.fingerprint, actual, "{kind:?}");
                let body = std::str::from_utf8(&captured.raw_body).unwrap();
                assert_eq!(
                    body.matches("swarmie.context.textual_projection").count(),
                    1
                );
                assert!(
                    description
                        .measurement
                        .components
                        .iter()
                        .find(|part| part.kind == ComponentKind::Conversation)
                        .unwrap()
                        .bytes
                        >= source_bytes as u64
                );
                assert!(
                    description
                        .measurement
                        .components
                        .iter()
                        .find(|part| part.kind == ComponentKind::Tools)
                        .unwrap()
                        .bytes
                        > 0
                );
                match kind {
                    Provider::OpenAi | Provider::DeepSeek | Provider::Kimi | Provider::Ollama => {
                        assert_eq!(
                            captured.body["messages"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|message| message["role"].as_str().unwrap())
                                .collect::<Vec<_>>(),
                            ["system", "assistant", "user", "assistant", "tool"]
                        )
                    }
                    Provider::Anthropic => assert_eq!(
                        captured.body["messages"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|message| message["role"].as_str().unwrap())
                            .collect::<Vec<_>>(),
                        ["assistant", "user", "assistant", "user"]
                    ),
                    Provider::Gemini => {
                        assert_eq!(captured.body["contents"][0]["role"], "model");
                        assert_eq!(captured.body["contents"][1]["role"], "user");
                        assert!(
                            captured.body["contents"][2]["parts"][0]
                                .get("functionCall")
                                .is_some()
                        );
                        assert!(
                            captured.body["contents"][3]["parts"][0]
                                .get("functionResponse")
                                .is_some()
                        );
                    }
                    Provider::OpenAiResponses => {
                        assert_eq!(captured.body["input"][0]["role"], "assistant");
                        assert_eq!(captured.body["input"][1]["role"], "user");
                        assert_eq!(captured.body["input"][2]["type"], "function_call");
                        assert_eq!(captured.body["input"][3]["type"], "function_call_output");
                    }
                }
                server.finish().await;
                close(&host).await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn canonical_mcp_names_round_trip_through_all_provider_boundaries() {
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let canonical = "mcp.fixture.remote_tool";
                let first = serde_json::from_str(
                    &wire(kind, true)
                        .to_string()
                        .replace("counter_add", "mcp__fixture__remote_tool"),
                )
                .unwrap();
                let server =
                    Server::new(vec![Reply::ok(first), Reply::ok(wire(kind, false))]).await;
                let host = Host::new(LocalExecutor);
                let config = configuration(kind, &server.endpoint);
                let model = Rc::new(
                    qualified_model(
                        config.connections[0].clone(),
                        key().model,
                        Rc::new(Secrets),
                        provider::ModelProtection::Unprotected,
                    )
                    .unwrap(),
                );
                let mut request = input(true);
                request.tools[0].name = swarmie_providers::ToolName::new(canonical).unwrap();
                request.tools[0].description.clear();
                for turn in 0..2 {
                    let model = model.clone();
                    let copy = request.clone();
                    let output = host
                        .root("alice", move |ctx| {
                            async move { model.complete(&ctx, OperationId::new(), &copy).await }
                                .boxed_local()
                        })
                        .await
                        .unwrap();
                    if turn == 0 {
                        assert_eq!(
                            output.message.tool_calls[0].name.as_str(),
                            canonical,
                            "{kind:?}"
                        );
                        let call = output.message.tool_calls[0].id.clone();
                        request
                            .context
                            .messages
                            .push(swarmie_providers::Message::Assistant(output.message));
                        request
                            .context
                            .messages
                            .push(swarmie_providers::Message::Tool {
                                call,
                                result: swarmie_providers::ToolResult::Success(json!({"ok":true})),
                            });
                    } else {
                        assert_eq!(output.message.content, "Done");
                    }
                }
                let calls = server.calls.borrow().clone();
                assert_eq!(calls.len(), 2);
                for call in calls {
                    let body = call.body.to_string();
                    assert!(!body.contains(canonical));
                    assert!(body.contains("mcp__fixture__remote_tool"));
                }
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_model_suffix_is_not_an_implicit_reasoning_setting() {
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let mut body = wire(kind, false);
                if body.get("model").is_some() {
                    body["model"] = json!("fixture-model-max");
                }
                let server = Server::new(vec![Reply::ok(body)]).await;
                let mut config = configuration(kind, &server.endpoint);
                let id = provider::ModelId::new("fixture-model-max").unwrap();
                config.connections[0].models = vec![id.clone()];
                let model = Rc::new(
                    qualified_model(
                        config.connections[0].clone(),
                        id,
                        Rc::new(Secrets),
                        provider::ModelProtection::Unprotected,
                    )
                    .unwrap(),
                );
                let host = Host::new(LocalExecutor);
                host.root("alice", move |ctx| {
                    async move {
                        model
                            .complete(&ctx, OperationId::new(), &input(false))
                            .await
                    }
                    .boxed_local()
                })
                .await
                .unwrap();
                let calls = server.calls.borrow().clone();
                let call = &calls[0];
                if kind == Provider::Gemini {
                    assert!(call.path.contains("fixture-model-max:"), "{}", call.path);
                } else {
                    assert_eq!(call.body["model"], "fixture-model-max", "{kind:?}");
                }
                assert!(call.body.get("reasoning").is_none());
                assert!(call.body.get("reasoning_effort").is_none());
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn six_explicit_provider_routes_use_actual_http_and_normalize_usage() {
    LocalSet::new()
        .run_until(async {
            for provider in PROVIDERS {
                let server = Server::new(vec![Reply::ok(wire(provider, false))]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    unprotected_orb(configuration(provider, &server.endpoint), Rc::new(Secrets))
                        .unwrap(),
                ])
                .unwrap();
                let reference = Rc::new(
                    host.get(provider::SERVICE, key().segments(), "alice")
                        .unwrap(),
                );
                let response: ModelResponse = call(
                    &host,
                    reference,
                    "alice",
                    ModelMethod::Complete.as_str(),
                    (OperationId::new(), input(false)),
                )
                .await
                .unwrap();
                assert_eq!(response.message.content, "Done", "{provider:?}");
                assert!(response.message.tool_calls.is_empty());
                assert_eq!(response.usage.input_tokens, 7);
                assert_eq!(response.usage.output_tokens, 3);
                let calls = server.calls.borrow().clone();
                assert_eq!(calls.len(), 1);
                let captured = &calls[0];
                let lower = captured.headers.to_lowercase();
                match provider {
                    Provider::OpenAi | Provider::DeepSeek | Provider::Kimi => {
                        assert!(captured.path.starts_with("/v1/chat/completions"))
                    }
                    Provider::OpenAiResponses => {
                        assert!(captured.path.starts_with("/v1/responses"))
                    }
                    Provider::Anthropic => {
                        assert!(captured.path.starts_with("/v1/messages"));
                        assert!(lower.contains("x-api-key: fixture-secret"));
                    }
                    Provider::Gemini => assert!(captured.path.contains("generateContent")),
                    Provider::Ollama => assert!(captured.path.contains("chat")),
                }
                assert!(captured.body.to_string().contains("Add one."));
                assert!(captured.body.to_string().contains("Be concise."));
                drop(calls);
                close(&host).await;
                server.finish().await;
            }
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn the_same_native_agent_loop_executes_tool_cycles_through_all_adapters() {
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let server = Server::new(vec![
                    Reply::ok(wire(kind, true)),
                    Reply::ok(wire(kind, false)),
                ])
                .await;
                let host = Host::new(LocalExecutor);
                let counter = Rc::new(Counter::default());
                host.install(vec![
                    unprotected_orb(configuration(kind, &server.endpoint), Rc::new(Secrets))
                        .unwrap(),
                ])
                .unwrap();
                let binding = host.clone();
                let tools = counter.clone();
                host.install(vec![swarmie_agent_loop::orb(move |d| {
                    Ok(swarmie_agent_loop::Agent::new(
                        Rc::new(swarmie_providers::ModelService::bind(
                            &d,
                            binding.clone(),
                            provider::SERVICE,
                            key().segments(),
                        )?),
                        tools.clone(),
                    ))
                })])
                .unwrap();
                let reference = Rc::new(
                    host.get(swarmie_agent_loop::SERVICE, vec!["main".into()], "alice")
                        .unwrap(),
                );
                let model_request = input(true);
                let request = swarmie_agent_loop::Request {
                    context: model_request.context,
                    tools: model_request.tools,
                    max_turns: std::num::NonZeroU32::new(3).unwrap(),
                };
                let result: swarmie_agent_loop::ExecutionResult = call(
                    &host,
                    reference,
                    "alice",
                    swarmie_agent_loop::Method::Execute.as_str(),
                    request,
                )
                .await
                .unwrap();
                let output = result.unwrap();
                assert_eq!(output.messages.len(), 3);
                assert_eq!(counter.0.get(), 1);
                assert_eq!(output.usage.input_tokens, 14);
                assert_eq!(output.usage.output_tokens, 6);
                let calls = server.calls.borrow().clone();
                assert_eq!(calls.len(), 2);
                assert!(calls[1].body.to_string().contains("counter_add"));
                assert!(calls[1].body.to_string().contains('1'));
                drop(calls);
                close(&host).await;
                server.finish().await;
            }
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_requests_and_unadmitted_models_fail_before_network_dispatch() {
    LocalSet::new()
        .run_until(async {
            let server = Server::new(vec![]).await;
            let config = configuration(Provider::OpenAi, &server.endpoint);
            let connection = config.connections[0].clone();
            assert!(
                qualified_model(
                    connection.clone(),
                    provider::ModelId::new("other").unwrap(),
                    Rc::new(Secrets),
                    provider::ModelProtection::Unprotected
                )
                .is_err()
            );
            let model = Rc::new(
                qualified_model(
                    connection,
                    key().model,
                    Rc::new(Secrets),
                    provider::ModelProtection::Unprotected,
                )
                .unwrap(),
            );
            let host = Host::new(LocalExecutor);
            for case in 0..4 {
                let mut request = input(true);
                match case {
                    0 => request.tools.push(request.tools[0].clone()),
                    1 => request.tools[0].input_schema = json!({"type":"array"}),
                    2 => request
                        .context
                        .messages
                        .push(swarmie_providers::Message::Tool {
                            call: swarmie_providers::ToolCallId::new("unknown").unwrap(),
                            result: swarmie_providers::ToolResult::Success(json!(1)),
                        }),
                    _ => request.tools[0].input_schema = json!({"type":"array"}),
                }
                let model = model.clone();
                let error = host
                    .root("alice", move |ctx| {
                        async move { model.complete(&ctx, OperationId::new(), &request).await }
                            .boxed_local()
                    })
                    .await
                    .unwrap_err();
                assert_eq!(error.code, Code::Invalid);
            }
            assert!(server.calls.borrow().is_empty());
            close(&host).await;
            server.finish().await;
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn upstream_failures_are_redacted_and_never_imply_safe_replay() {
    LocalSet::new().run_until(async {
    for (status,code) in [(400,Code::Invalid),(403,Code::Forbidden),(429,Code::Exhausted),(500,Code::Unavailable)] {
        let server=Server::new(vec![Reply{status,body:json!({"error":{"message":"fixture-secret private-prompt private-provider-diagnostic"}}),delay:Duration::ZERO}]).await;let host=Host::new(LocalExecutor);host.install(vec![unprotected_orb(configuration(Provider::OpenAi,&server.endpoint),Rc::new(Secrets)).unwrap()]).unwrap();let reference=Rc::new(host.get(provider::SERVICE,key().segments(),"alice").unwrap());let operation=OperationId::new();
        let error=call::<_,ModelResponse>(&host,reference.clone(),"alice",ModelMethod::Complete.as_str(),(operation,input(false))).await.unwrap_err();assert_eq!(error.code,code);let diagnostic=format!("{error:?}");assert!(!diagnostic.contains("fixture-secret"));assert!(!diagnostic.contains("private-prompt"));assert!(!diagnostic.contains("private-provider-diagnostic"));
        let recovery:Recovery<ModelResponse>=call(&host,reference,"alice",ModelMethod::Recover.as_str(),(operation,input(false))).await.unwrap();assert_eq!(recovery,Recovery::Unknown);assert_eq!(server.calls.borrow().len(),1);close(&host).await;server.finish().await;
    }
}).await
}

#[tokio::test(flavor = "current_thread")]
async fn http_timeout_settles_without_retry_and_cleanup_releases_resources() {
    LocalSet::new()
        .run_until(async {
            let server = Server::new(vec![Reply {
                delay: Duration::from_millis(80),
                ..Reply::ok(wire(Provider::OpenAi, false))
            }])
            .await;
            let mut config = configuration(Provider::OpenAi, &server.endpoint);
            config.connections[0].timeout_ms = NonZeroU64::new(15).unwrap();
            let host = Host::new(LocalExecutor);
            host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()])
                .unwrap();
            let reference = Rc::new(
                host.get(provider::SERVICE, key().segments(), "alice")
                    .unwrap(),
            );
            let operation = OperationId::new();
            let error = call::<_, ModelResponse>(
                &host,
                reference.clone(),
                "alice",
                ModelMethod::Complete.as_str(),
                (operation, input(false)),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, Code::Unavailable);
            let recovered: Recovery<ModelResponse> = call(
                &host,
                reference,
                "alice",
                ModelMethod::Recover.as_str(),
                (operation, input(false)),
            )
            .await
            .unwrap();
            assert_eq!(recovered, Recovery::Unknown);
            close(&host).await;
            assert_eq!(server.calls.borrow().len(), 1);
            server.finish().await;
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn input_and_decoded_output_budgets_have_distinct_boundaries() {
    LocalSet::new()
        .run_until(async {
            let server = Server::new(vec![Reply::ok(wire(Provider::OpenAi, false))]).await;
            let mut config = configuration(Provider::OpenAi, &server.endpoint);
            config.connections[0].response_bytes = std::num::NonZeroUsize::new(8).unwrap();
            let host = Host::new(LocalExecutor);
            host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()])
                .unwrap();
            let reference = Rc::new(
                host.get(provider::SERVICE, key().segments(), "alice")
                    .unwrap(),
            );
            let error = call::<_, ModelResponse>(
                &host,
                reference,
                "alice",
                ModelMethod::Complete.as_str(),
                (OperationId::new(), input(false)),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, Code::Exhausted);
            assert_eq!(server.calls.borrow().len(), 1);
            close(&host).await;
            server.finish().await;
            let server = Server::new(vec![]).await;
            let mut config = configuration(Provider::OpenAi, &server.endpoint);
            config.connections[0].request_bytes = std::num::NonZeroUsize::new(8).unwrap();
            let host = Host::new(LocalExecutor);
            host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()])
                .unwrap();
            let reference = Rc::new(
                host.get(provider::SERVICE, key().segments(), "alice")
                    .unwrap(),
            );
            let error = call::<_, ModelResponse>(
                &host,
                reference,
                "alice",
                ModelMethod::Complete.as_str(),
                (OperationId::new(), input(false)),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, Code::Exhausted);
            assert!(server.calls.borrow().is_empty());
            close(&host).await;
            server.finish().await;
        })
        .await
}

#[test]
fn configuration_has_no_secret_fields_and_rejects_ambiguous_routing() {
    let mut config = configuration(Provider::OpenAi, "http://example.invalid/v1/");
    config.connections.push(config.connections[0].clone());
    assert!(unprotected_orb(config, Rc::new(Secrets)).is_err());
    let mut config = configuration(Provider::OpenAi, "http://user:secret@example.invalid/v1/");
    assert!(unprotected_orb(config.clone(), Rc::new(Secrets)).is_err());
    config.connections[0].endpoint = Some("http://example.invalid/v1/?key=secret".into());
    assert!(unprotected_orb(config, Rc::new(Secrets)).is_err());
    let connection = configuration(Provider::OpenAi, "http://example.invalid/v1/")
        .connections
        .remove(0);
    assert!(
        !serde_json::to_string(&connection)
            .unwrap()
            .contains("fixture-secret")
    );
    let error = qualified_model(
        connection,
        key().model,
        Rc::new(Secrets),
        provider::ModelProtection::Unprotected,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, Code::Unavailable);
}

#[cfg(feature = "mock")]
#[tokio::test(flavor = "current_thread")]
async fn optional_mock_recovers_a_lost_reply_and_checks_owner_and_exact_input() {
    LocalSet::new()
        .run_until(async {
            use provider::mock::{MockModel, Step};
            let response = ModelResponse {
                protection: swarmie_providers::SubmissionProtection::Unprotected,
                message: swarmie_providers::AssistantMessage {
                    provider: None,
                    content: "Done".into(),
                    tool_calls: vec![],
                },
                usage: swarmie_providers::Usage {
                    input_tokens: 7,
                    output_tokens: 3,
                },
            };
            let mock = Rc::new(MockModel::new([Step::LoseReply(response.clone())]));
            let host = Host::new(LocalExecutor);
            host.install(vec![provider::mock::orb("fixture.model", mock.clone())])
                .unwrap();
            let reference = Rc::new(
                host.get("fixture.model", vec!["main".into()], "alice")
                    .unwrap(),
            );
            let operation = OperationId::new();
            let first = call::<_, ModelResponse>(
                &host,
                reference.clone(),
                "alice",
                ModelMethod::Complete.as_str(),
                (operation, input(false)),
            )
            .await
            .unwrap_err();
            assert_eq!(first.code, Code::Unavailable);
            let recovered: Recovery<ModelResponse> = call(
                &host,
                reference.clone(),
                "alice",
                ModelMethod::Recover.as_str(),
                (operation, input(false)),
            )
            .await
            .unwrap();
            assert_eq!(recovered, Recovery::Completed(response));
            assert_eq!(mock.calls().len(), 1);
            assert_eq!(mock.remaining(), 0);
            let owner = call::<_, Recovery<ModelResponse>>(
                &host,
                reference.clone(),
                "bob",
                ModelMethod::Recover.as_str(),
                (operation, input(false)),
            )
            .await
            .unwrap_err();
            assert_eq!(owner.code, Code::Forbidden);
            let mut changed = input(false);
            changed.context.instructions = "different".into();
            let mismatch = call::<_, Recovery<ModelResponse>>(
                &host,
                reference,
                "alice",
                ModelMethod::Recover.as_str(),
                (operation, changed),
            )
            .await
            .unwrap_err();
            assert_eq!(mismatch.code, Code::Conflict);
            close(&host).await;
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_the_waiter_does_not_abandon_the_managed_http_call() {
    LocalSet::new()
        .run_until(async {
            let server = Server::new(vec![Reply {
                delay: Duration::from_millis(25),
                ..Reply::ok(wire(Provider::OpenAi, false))
            }])
            .await;
            let host = Host::new(LocalExecutor);
            host.install(vec![
                unprotected_orb(
                    configuration(Provider::OpenAi, &server.endpoint),
                    Rc::new(Secrets),
                )
                .unwrap(),
            ])
            .unwrap();
            let reference = Rc::new(
                host.get(provider::SERVICE, key().segments(), "alice")
                    .unwrap(),
            );
            let caller = host.clone();
            let waiter = tokio::task::spawn_local(async move {
                call::<_, ModelResponse>(
                    &caller,
                    reference,
                    "alice",
                    ModelMethod::Complete.as_str(),
                    (OperationId::new(), input(false)),
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(2), async {
                while server.calls.borrow().is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
            let calls = server.calls.clone();
            server.finish().await;
            close(&host).await;
            assert_eq!(calls.borrow().len(), 1);
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn real_http_models_share_the_existing_atomic_history_and_task_path() {
    LocalSet::new()
        .run_until(async {
            use swarmie_agent_loop::conversation as history;
            use swarmie_conversations as conversations;
            use swarmie_durable_tasks as tasks;
            use swarmie_storage::sqlite::SqliteOwner;
            for streaming in [false, true] {
                let server = if streaming {
                    Server::streaming(vec![
                        streaming::reply(Provider::OpenAi, true),
                        streaming::reply(Provider::OpenAi, false),
                    ])
                    .await
                } else {
                    Server::new(vec![
                        Reply::ok(wire(Provider::OpenAi, true)),
                        Reply::ok(wire(Provider::OpenAi, false)),
                    ])
                    .await
                };
                let directory = tempfile::tempdir().unwrap();
                let owner = SqliteOwner::new(directory.path().join("history.sqlite"));
                let host = Host::new(LocalExecutor);
                let counter = Rc::new(Counter::default());
                host.install(vec![
                    unprotected_orb(
                        configuration(Provider::OpenAi, &server.endpoint),
                        Rc::new(Secrets),
                    )
                    .unwrap(),
                    conversations::orb(owner.conversation_factory()),
                ])
                .unwrap();
                let binding = host.clone();
                let tools = counter.clone();
                host.install(vec![
                    tasks::orb(
                        host.clone(),
                        owner.task_factory(),
                        tasks::Definitions::new([history::durable::definition()]).unwrap(),
                        tasks::Options::new(
                            LocalExecutor,
                            NonZeroU64::new(60_000).unwrap(),
                            Rc::new(|duration| {
                                async move { tokio::time::sleep(duration).await }.boxed_local()
                            }),
                        ),
                    ),
                    history::durable::orb(move |declaration| {
                        let agent = swarmie_agent_loop::Agent::new(
                            Rc::new(swarmie_providers::ModelService::bind(
                                &declaration,
                                binding.clone(),
                                provider::SERVICE,
                                key().segments(),
                            )?),
                            tools.clone(),
                        );
                        let agent = if streaming {
                            agent.with_progress(swarmie_providers::Progress::bind(&declaration)?)
                        } else {
                            agent
                        };
                        Ok((
                            agent,
                            history::History::bind(
                                &declaration,
                                binding.clone(),
                                conversations::SERVICE,
                                vec![conversations::INSTANCE_KEY.into()],
                            )?,
                        ))
                    }),
                ])
                .unwrap();
                let conversation = Rc::new(
                    host.get(
                        conversations::SERVICE,
                        vec![conversations::INSTANCE_KEY.into()],
                        "host",
                    )
                    .unwrap(),
                );
                let scheduler = Rc::new(
                    host.get(tasks::SERVICE, vec![tasks::INSTANCE_KEY.into()], "host")
                        .unwrap(),
                );
                let created: conversations::WriteReceipt = call(
                    &host,
                    conversation.clone(),
                    "alice",
                    conversations::Method::Create.as_str(),
                    (conversations::ConversationId::new(),),
                )
                .await
                .unwrap();
                let model_request = input(true);
                let entries: Vec<_> = model_request
                    .context
                    .messages
                    .iter()
                    .map(|m| {
                        conversations::EntryDraft::encode(history::Record::Message(m.clone()))
                            .unwrap()
                    })
                    .collect();
                let appended: conversations::WriteReceipt = call(
                    &host,
                    conversation.clone(),
                    "alice",
                    conversations::Method::Append.as_str(),
                    (created.head, entries),
                )
                .await
                .unwrap();
                let request = history::Request::new(
                    appended.head,
                    model_request.context.instructions,
                    model_request.tools,
                    std::num::NonZeroU32::new(3).unwrap(),
                );
                let id: tasks::TaskId = call(
                    &host,
                    scheduler.clone(),
                    "alice",
                    tasks::Method::Create.as_str(),
                    (tasks::TaskSpec {
                        id: tasks::TaskId::new(),
                        definition: history::durable::definition(),
                        input: serde_json::to_value(request).unwrap(),
                    },),
                )
                .await
                .unwrap();
                call::<_, ()>(
                    &host,
                    scheduler.clone(),
                    "host",
                    tasks::Method::Resume.as_str(),
                    (),
                )
                .await
                .unwrap();
                let record = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let record: Option<tasks::TaskRecord> = call(
                            &host,
                            scheduler.clone(),
                            "alice",
                            tasks::Method::Get.as_str(),
                            (id,),
                        )
                        .await
                        .unwrap();
                        let record = record.unwrap();
                        if let tasks::TaskStatus::Completed(_) = record.status {
                            break record;
                        }
                        assert!(!matches!(record.status, tasks::TaskStatus::Failed(_)));
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert!(record.assignment.is_none());
                assert_eq!(counter.0.get(), 1);
                let checkpoint: history::Checkpoint =
                    serde_json::from_value(record.checkpoint.unwrap()).unwrap();
                let page: conversations::Page = call(
                    &host,
                    conversation,
                    "alice",
                    conversations::Method::Read.as_str(),
                    conversations::Read::new(checkpoint.head()),
                )
                .await
                .unwrap();
                let records: Vec<_> = page
                    .entries
                    .iter()
                    .map(|e| e.decode::<history::Record>().unwrap())
                    .collect();
                assert_eq!(
                    records
                        .iter()
                        .filter(|r| matches!(r, history::Record::Message(_)))
                        .count(),
                    4
                );
                assert_eq!(
                    records
                        .iter()
                        .filter(|r| matches!(r, history::Record::PreparedRequest { .. }))
                        .count(),
                    2
                );
                close(&host).await;
                assert_eq!(server.calls.borrow().len(), 2);
                server.finish().await;
            }
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn native_continuations_are_retained_while_incomplete_answers_and_foreign_tools_refuse() {
    LocalSet::new()
        .run_until(async {
            let mut truncated = wire(Provider::OpenAi, false);
            truncated["choices"][0]["finish_reason"] = json!("length");
            let mut reasoning = wire(Provider::OpenAi, false);
            reasoning["choices"][0]["message"]["reasoning_content"] =
                json!("private reasoning continuation");
            let mut signed = wire(Provider::Gemini, true);
            signed["candidates"][0]["content"]["parts"][0]["thoughtSignature"] =
                json!("opaque-signature");
            let mut unknown = wire(Provider::OpenAi, true);
            unknown["choices"][0]["message"]["tool_calls"][0]["function"]["name"] =
                json!("unadmitted_tool");
            for (kind, body, supported) in [
                (Provider::OpenAi, truncated, false),
                (Provider::OpenAi, reasoning, true),
                (Provider::Gemini, signed, true),
                (Provider::OpenAi, unknown, false),
            ] {
                let server = Server::new(vec![Reply::ok(body)]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    unprotected_orb(configuration(kind, &server.endpoint), Rc::new(Secrets))
                        .unwrap(),
                ])
                .unwrap();
                let reference = Rc::new(
                    host.get(provider::SERVICE, key().segments(), "alice")
                        .unwrap(),
                );
                let operation = OperationId::new();
                let result = call::<_, ModelResponse>(
                    &host,
                    reference.clone(),
                    "alice",
                    ModelMethod::Complete.as_str(),
                    (operation, input(true)),
                )
                .await;
                if supported {
                    let output = result.unwrap();
                    let retained = output.message.provider.unwrap().to_string();
                    assert!(retained.contains(if kind == Provider::Gemini {
                        "opaque-signature"
                    } else {
                        "private reasoning continuation"
                    }));
                } else {
                    assert_eq!(result.unwrap_err().code, Code::Invalid, "{kind:?}");
                }
                let recovery: Recovery<ModelResponse> = call(
                    &host,
                    reference,
                    "alice",
                    ModelMethod::Recover.as_str(),
                    (operation, input(true)),
                )
                .await
                .unwrap();
                assert_eq!(recovery, Recovery::Unknown);
                assert_eq!(server.calls.borrow().len(), 1);
                close(&host).await;
                server.finish().await;
            }
        })
        .await
}

mod protection {
    use super::*;
    use provider::{Credentials, ModelCredential, ModelDecisions, ModelProtection};
    use std::cell::{Cell, RefCell};
    use swarmie_permissions::*;
    use swarmie_providers::protection::{DecisionIntent, Detector, ModelGuard};
    use tembra::Context;
    use uuid::Uuid;
    struct Decisions {
        shell: HumanSession,
        count: Rc<Cell<usize>>,
        allow: bool,
    }
    impl ModelDecisions for Decisions {
        fn request<'a>(
            &'a self,
            _execution: &'a swarmie_permissions::ExecutionBinding,
            intent: &'a DecisionIntent,
        ) -> futures::future::LocalBoxFuture<'a, tembra::Result<HumanDecision>> {
            async move {
                self.count.set(self.count.get() + 1);
                if !self.allow {
                    return Err(tembra::Error::new(
                        tembra::Code::Cancelled,
                        "fixture",
                        "user cancelled",
                    ));
                }
                self.shell
                    .decide(&("model-send/v1", intent))
                    .map_err(|_| tembra::Error::invalid("fixture", "ended shell"))
            }
            .boxed_local()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn final_tool_description_detection_cancels_before_real_http_and_does_not_borrow_another_owner()
     {
        LocalSet::new()
            .run_until(async {
                let server = Server::new(vec![]).await;
                let host = Host::new(LocalExecutor);
                let authority = Authority::new();
                let shell = authority.authenticate_shell(ShellIdentity {
                    cloud: None,
                    account: None,
                    installation: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                });
                let binding = authority
                    .execution(
                        &shell,
                        ExecutionIdentity {
                            account: None,
                            execution: Uuid::new_v4(),
                            generation: Uuid::new_v4(),
                            project: None,
                            agent: None,
                            conversation: None,
                            main_agent: true,
                        },
                    )
                    .unwrap();
                let contexts = ExecutionContexts::new(authority.clone());
                let count = Rc::new(Cell::new(0));
                let protection = ModelProtection::Guarded {
                    guard: ModelGuard::new(
                        "detector".into(),
                        PluginGrant::bundled(CorePlugin::ModelProtection, "detector".into()),
                        Detector::bundled(),
                    ),
                    executions: contexts.clone(),
                    decisions: Rc::new(Decisions {
                        shell,
                        count: count.clone(),
                        allow: false,
                    }),
                };
                let model = qualified_model(
                    configuration(Provider::OpenAi, &server.endpoint)
                        .connections
                        .remove(0),
                    key().model,
                    Rc::new(Secrets),
                    protection,
                )
                .unwrap();
                let mut request = input(true);
                request.tools[0].description = "ghp_aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5".into();
                host.root("alice", move |context| {
                    async move {
                        let scoped = contexts.bind(&context, binding).unwrap();
                        let result = model
                            .complete(scoped.context(), OperationId::new(), &request)
                            .await;
                        assert!(result.is_err());
                        Ok(())
                    }
                    .boxed_local()
                })
                .await
                .unwrap();
                assert_eq!(count.get(), 1);
                assert!(server.calls.borrow().is_empty());
                close(&host).await;
                server.finish().await;
            })
            .await;
    }

    #[derive(Clone)]
    struct Waiting {
        entered: Rc<tokio::sync::Notify>,
        retired: Rc<Cell<bool>>,
        binding: Rc<RefCell<Option<ExecutionBinding>>>,
        physical: Option<(Rc<tokio::sync::Notify>, Rc<Cell<bool>>)>,
    }
    struct Retire(Rc<Cell<bool>>);
    impl Drop for Retire {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    impl Waiting {
        fn pending<T>(&self) -> futures::future::LocalBoxFuture<'_, tembra::Result<T>> {
            async move {
                let _retire = Retire(self.retired.clone());
                if let Some((release, settled)) = self.physical.clone() {
                    tokio::task::spawn_local(async move {
                        release.notified().await;
                        settled.set(true);
                    });
                }
                self.entered.notify_one();
                futures::future::pending().await
            }
            .boxed_local()
        }
    }
    impl Credentials for Waiting {
        fn acquire<'a>(
            &'a self,
            _: &'a Context,
            admitted: &'a provider::AdmittedAttempt,
            _: &'a str,
        ) -> futures::future::LocalBoxFuture<'a, tembra::Result<ModelCredential>> {
            *self.binding.borrow_mut() = admitted.execution().cloned();
            self.pending()
        }
    }
    impl ModelDecisions for Waiting {
        fn request<'a>(
            &'a self,
            execution: &'a ExecutionBinding,
            _: &'a DecisionIntent,
        ) -> futures::future::LocalBoxFuture<'a, tembra::Result<HumanDecision>> {
            *self.binding.borrow_mut() = Some(execution.clone());
            self.pending()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_stop_retires_pre_dispatch_waits_without_losing_physical_credential_reads() {
        LocalSet::new().run_until(async {
            for credential_wait in [false, true] {
                let server = Server::new(vec![Reply::ok(wire(Provider::OpenAi, false))]).await;
                let requests = server.calls.clone();
                let host = Host::new(LocalExecutor);
                let authority = Authority::new();
                let shell = authority.authenticate_shell(ShellIdentity { cloud: None, account: None, installation: Uuid::new_v4(), generation: Uuid::new_v4() });
                let binding = authority.execution(&shell, ExecutionIdentity { account: None, execution: Uuid::new_v4(), generation: Uuid::new_v4(), project: None, agent: None, conversation: None, main_agent: true }).unwrap();
                let contexts = ExecutionContexts::new(authority);
                let release = Rc::new(tokio::sync::Notify::new());
                let settled = Rc::new(Cell::new(false));
                let waiting = Rc::new(Waiting {
                    entered: Rc::new(tokio::sync::Notify::new()), retired: Rc::new(Cell::new(false)),
                    binding: Rc::new(RefCell::new(None)),
                    physical: credential_wait.then(|| (release.clone(), settled.clone())),
                });
                let decisions: Rc<dyn ModelDecisions> = if credential_wait { Rc::new(Decisions { shell, count: Rc::new(Cell::new(0)), allow: true }) } else { waiting.clone() };
                let credentials: Rc<dyn Credentials> = if credential_wait { waiting.clone() } else { Rc::new(Secrets) };
                let connection = configuration(Provider::OpenAi, &server.endpoint).connections.remove(0);
                let stop = swarmie_ai::StopHandle::new();
                let model = qualified_model(connection.clone(), key().model, credentials, ModelProtection::Guarded {
                    guard: ModelGuard::new("stop-wait".into(), PluginGrant::bundled(CorePlugin::ModelProtection, "stop-wait".into()), None),
                    executions: contexts.clone(), decisions,
                }).unwrap().with_stop(stop.clone());
                let next = qualified_model(connection, key().model, Rc::new(Secrets), ModelProtection::Unprotected).unwrap();
                host.root("alice", move |context| async move {
                    let scoped = contexts.bind(&context, binding.clone()).unwrap();
                    let request = input(false);
                    let call = model.complete(scoped.context(), OperationId::new(), &request);
                    futures::pin_mut!(call);
                    tokio::select! {
                        _ = waiting.entered.notified() => {},
                        result = &mut call => panic!("expected a pending pre-dispatch wait: {result:?}"),
                    }
                    assert!(requests.borrow().is_empty());
                    stop.cancel();
                    let error = tokio::time::timeout(Duration::from_secs(1), &mut call).await.expect("explicit Stop must settle a pending pre-dispatch wait").unwrap_err();
                    assert_eq!(error.code, Code::Cancelled);
                    assert_eq!(error.reason, "swarmie.models.ai.not_dispatched");
                    assert!(scoped.context().check().is_ok());
                    assert!(binding.is_live(), "Stop must not close the parent execution");
                    assert!(waiting.retired.get());
                    assert!(!waiting.binding.borrow().as_ref().unwrap().is_live(), "unanswered decisions must lose their scoped authority");
                    assert!(requests.borrow().is_empty());
                    if credential_wait {
                        assert!(!settled.get(), "a cancelled observer is not physical settlement");
                        release.notify_one();
                        tokio::time::timeout(Duration::from_secs(1), async { while !settled.get() { tokio::task::yield_now().await; } }).await.unwrap();
                    }
                    next.complete(scoped.context(), OperationId::new(), &request).await?;
                    Ok(())
                }.boxed_local()).await.unwrap();
                assert_eq!(server.calls.borrow().len(), 1, "only the fresh authorized operation may dispatch");
                close(&host).await;
                server.finish().await;
            }
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn every_dispatched_attempt_reports_its_actual_protection_status() {
        use swarmie_providers::SubmissionProtection;
        LocalSet::new()
            .run_until(async {
                let server = Server::new(
                    (0..4)
                        .map(|_| Reply::ok(wire(Provider::OpenAi, false)))
                        .collect(),
                )
                .await;
                let host = Host::new(LocalExecutor);
                let authority = Authority::new();
                let shell = authority.authenticate_shell(ShellIdentity {
                    cloud: None,
                    account: None,
                    installation: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                });
                let binding = authority
                    .execution(
                        &shell,
                        ExecutionIdentity {
                            account: None,
                            execution: Uuid::new_v4(),
                            generation: Uuid::new_v4(),
                            project: None,
                            agent: None,
                            conversation: None,
                            main_agent: true,
                        },
                    )
                    .unwrap();
                let contexts = ExecutionContexts::new(authority);
                let guard = ModelGuard::new(
                    "label-test".into(),
                    PluginGrant::bundled(CorePlugin::ModelProtection, "label-test".into()),
                    Detector::bundled(),
                );
                let decisions = Rc::new(Decisions {
                    shell: shell.clone(),
                    count: Rc::new(Cell::new(0)),
                    allow: true,
                });
                let model = qualified_model(
                    configuration(Provider::OpenAi, &server.endpoint)
                        .connections
                        .remove(0),
                    key().model,
                    Rc::new(Secrets),
                    ModelProtection::Guarded {
                        guard: guard.clone(),
                        executions: contexts.clone(),
                        decisions: decisions.clone(),
                    },
                )
                .unwrap();
                let absent = qualified_model(
                    configuration(Provider::OpenAi, &server.endpoint)
                        .connections
                        .remove(0),
                    key().model,
                    Rc::new(Secrets),
                    ModelProtection::Guarded {
                        guard: ModelGuard::new(
                            "absent-label-test".into(),
                            PluginGrant::bundled(
                                CorePlugin::ModelProtection,
                                "absent-label-test".into(),
                            ),
                            None,
                        ),
                        executions: contexts.clone(),
                        decisions,
                    },
                )
                .unwrap();
                host.root("label-owner", move |context| {
                    async move {
                        let scoped = contexts.bind(&context, binding).unwrap();
                        let clean = model
                            .complete(scoped.context(), OperationId::new(), &input(false))
                            .await?;
                        assert_eq!(clean.protection, SubmissionProtection::Clean);
                        let mut detected = input(true);
                        detected.tools[0].description =
                            "ghp_aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5".into();
                        let overridden = model
                            .complete(scoped.context(), OperationId::new(), &detected)
                            .await?;
                        assert_eq!(overridden.protection, SubmissionProtection::HumanOverride);
                        let decision = shell
                            .decide(&("model-protection-policy/v1", guard.generation(), false))
                            .unwrap();
                        guard.configure(&shell, decision, false).unwrap();
                        let disabled = model
                            .complete(scoped.context(), OperationId::new(), &detected)
                            .await?;
                        assert_eq!(disabled.protection, SubmissionProtection::Unprotected);
                        let cannot_inspect = absent
                            .complete(scoped.context(), OperationId::new(), &input(false))
                            .await?;
                        assert_eq!(
                            cannot_inspect.protection,
                            SubmissionProtection::CannotInspectOverride
                        );
                        Ok(())
                    }
                    .boxed_local()
                })
                .await
                .unwrap();
                assert_eq!(server.calls.borrow().len(), 4);
                close(&host).await;
                server.finish().await;
            })
            .await;
    }

    struct Rotating(Rc<RefCell<String>>);
    impl Credentials for Rotating {
        fn acquire<'a>(
            &'a self,
            _: &'a Context,
            _: &'a provider::AdmittedAttempt,
            _: &'a str,
        ) -> futures::future::LocalBoxFuture<'a, tembra::Result<ModelCredential>> {
            Box::pin(
                async move { ModelCredential::new(self.0.borrow().clone(), "[secret]".into()) },
            )
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn persistent_client_uses_next_credential_and_masks_reflected_values() {
        LocalSet::new()
            .run_until(async {
                let mut first = wire(Provider::OpenAi, false);
                first["choices"][0]["message"]["content"] = "echo first-credential".into();
                let server = Server::new(vec![
                    Reply::ok(first),
                    Reply::ok(wire(Provider::OpenAi, false)),
                ])
                .await;
                let value = Rc::new(RefCell::new("first-credential".into()));
                let model = qualified_model(
                    configuration(Provider::OpenAi, &server.endpoint)
                        .connections
                        .remove(0),
                    key().model,
                    Rc::new(Rotating(value.clone())),
                    provider::ModelProtection::Unprotected,
                )
                .unwrap();
                let host = Host::new(LocalExecutor);
                host.root("alice", move |context| {
                    async move {
                        let response = model
                            .complete(&context, OperationId::new(), &input(false))
                            .await?;
                        assert_eq!(response.message.content, "echo [secret]");
                        *value.borrow_mut() = "second-credential".into();
                        model
                            .complete(&context, OperationId::new(), &input(false))
                            .await?;
                        Ok(())
                    }
                    .boxed_local()
                })
                .await
                .unwrap();
                {
                    let calls = server.calls.borrow();
                    assert!(calls[0].headers.contains("first-credential"));
                    assert!(calls[1].headers.contains("second-credential"));
                    assert!(!calls[1].headers.contains("first-credential"));
                }
                close(&host).await;
                server.finish().await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn captured_project_sources_reach_all_seven_actual_provider_payloads_before_user_input() {
    use swarmie_providers::{ContextSources, SourceKind, SourcePart, SourceScope};
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let server = Server::new(vec![Reply::ok(wire(kind, false))]).await;
                let config = configuration(kind, &server.endpoint);
                let model = qualified_model(
                    config.connections[0].clone(),
                    key().model,
                    Rc::new(Secrets),
                    provider::ModelProtection::Unprotected,
                )
                .unwrap();
                let mut request = input(false);
                request.context.sources = Some(
                    ContextSources::new(
                        SourceScope {
                            node: "node".into(),
                            principal: "alice".into(),
                            directory: "project".into(),
                        },
                        vec![
                            SourcePart::new(
                                SourceKind::ProjectInstructions,
                                "id".into(),
                                "revision".into(),
                                "AGENTS.md".into(),
                                "project source marker".into(),
                            )
                            .unwrap(),
                        ],
                    )
                    .unwrap(),
                );
                let host = Host::new(LocalExecutor);
                host.root("alice", move |context| {
                    async move { model.complete(&context, OperationId::new(), &request).await }
                        .boxed_local()
                })
                .await
                .unwrap();
                let calls = server.calls.borrow().clone();
                assert_eq!(calls.len(), 1);
                let body = std::str::from_utf8(&calls[0].raw_body).unwrap();
                assert_eq!(
                    body.matches("project source marker").count(),
                    1,
                    "{kind:?}: {body}"
                );
                assert!(
                    body.find("project source marker").unwrap() < body.find("Add one.").unwrap(),
                    "{kind:?}: {body}"
                );
                server.finish().await;
                close(&host).await;
            }
        })
        .await;
}
