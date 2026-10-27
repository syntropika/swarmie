mod support;

#[path = "streaming/mod.rs"]
mod streaming;

use futures::FutureExt;
use serde_json::json;
use std::{num::NonZeroU64, rc::Rc, time::Duration};
use support::*;
use swarmie_model_provider_genai::{self as provider, GenaiModel, Provider};
use swarmie_models::{Model, ModelMethod, ModelResponse, OperationId, Recovery};
use tembra::{Code, Host};
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn six_explicit_provider_routes_use_actual_http_and_normalize_usage() {
    LocalSet::new()
        .run_until(async {
            for provider in PROVIDERS {
                let server = Server::new(vec![Reply::ok(wire(provider, false))]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    provider::orb(configuration(provider, &server.endpoint), Rc::new(Secrets))
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
                    Provider::OpenAi | Provider::DeepSeek => {
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
async fn the_same_native_agent_loop_executes_tool_cycles_through_all_six_adapters() {
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
                    provider::orb(configuration(kind, &server.endpoint), Rc::new(Secrets)).unwrap(),
                ])
                .unwrap();
                let binding = host.clone();
                let tools = counter.clone();
                host.install(vec![swarmie_agent_loop::orb(move |d| {
                    Ok(swarmie_agent_loop::Agent::new(
                        Rc::new(swarmie_models::ModelService::bind(
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
                GenaiModel::new(
                    connection.clone(),
                    provider::ModelId::new("other").unwrap(),
                    &Secrets
                )
                .is_err()
            );
            let model = Rc::new(GenaiModel::new(connection, key().model, &Secrets).unwrap());
            let host = Host::new(LocalExecutor);
            for case in 0..4 {
                let mut request = input(true);
                match case {
                    0 => request.tools.push(request.tools[0].clone()),
                    1 => request.tools[0].input_schema = json!({"type":"array"}),
                    2 => request
                        .context
                        .messages
                        .push(swarmie_models::Message::Tool {
                            call: swarmie_models::ToolCallId::new("unknown").unwrap(),
                            result: swarmie_models::ToolResult::Success(json!(1)),
                        }),
                    _ => {
                        request.tools[0].name =
                            swarmie_models::ToolName::new("not.portable").unwrap()
                    }
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
        let server=Server::new(vec![Reply{status,body:json!({"error":{"message":"fixture-secret private-prompt private-provider-diagnostic"}}),delay:Duration::ZERO}]).await;let host=Host::new(LocalExecutor);host.install(vec![provider::orb(configuration(Provider::OpenAi,&server.endpoint),Rc::new(Secrets)).unwrap()]).unwrap();let reference=Rc::new(host.get(provider::SERVICE,key().segments(),"alice").unwrap());let operation=OperationId::new();
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
            host.install(vec![provider::orb(config, Rc::new(Secrets)).unwrap()])
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
            host.install(vec![provider::orb(config, Rc::new(Secrets)).unwrap()])
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
            host.install(vec![provider::orb(config, Rc::new(Secrets)).unwrap()])
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
    assert!(provider::orb(config, Rc::new(Secrets)).is_err());
    let mut config = configuration(Provider::OpenAi, "http://user:secret@example.invalid/v1/");
    assert!(provider::orb(config.clone(), Rc::new(Secrets)).is_err());
    config.connections[0].endpoint = Some("http://example.invalid/v1/?key=secret".into());
    assert!(provider::orb(config, Rc::new(Secrets)).is_err());
    let connection = configuration(Provider::OpenAi, "http://example.invalid/v1/")
        .connections
        .remove(0);
    assert!(
        !serde_json::to_string(&connection)
            .unwrap()
            .contains("fixture-secret")
    );
    let error = GenaiModel::new(connection, key().model, &Secrets)
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
                message: swarmie_models::AssistantMessage {
                    content: "Done".into(),
                    tool_calls: vec![],
                },
                usage: swarmie_models::Usage {
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
                provider::orb(
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
            use swarmie_storage_sqlite::SqliteOwner;
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
                    provider::orb(
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
                            Rc::new(swarmie_models::ModelService::bind(
                                &declaration,
                                binding.clone(),
                                provider::SERVICE,
                                key().segments(),
                            )?),
                            tools.clone(),
                        );
                        let agent = if streaming {
                            agent.with_progress(swarmie_models::Progress::bind(&declaration)?)
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
                let request = history::Request {
                    at: appended.head,
                    instructions: model_request.context.instructions,
                    tools: model_request.tools,
                    max_turns: std::num::NonZeroU32::new(3).unwrap(),
                };
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
async fn unrepresentable_continuations_and_incomplete_answers_are_not_silently_accepted() {
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
            for (kind, body) in [
                (Provider::OpenAi, truncated),
                (Provider::OpenAi, reasoning),
                (Provider::Gemini, signed),
                (Provider::OpenAi, unknown),
            ] {
                let server = Server::new(vec![Reply::ok(body)]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    provider::orb(configuration(kind, &server.endpoint), Rc::new(Secrets)).unwrap(),
                ])
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
                    (operation, input(true)),
                )
                .await
                .unwrap_err();
                assert_eq!(error.code, Code::Invalid, "{kind:?}");
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
