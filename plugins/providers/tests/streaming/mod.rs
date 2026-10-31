use super::support::*;
use serde_json::{Value, json};
use std::{cell::RefCell, num::NonZeroU32, rc::Rc, time::Duration};
use swarmie_agent_loop::{Agent, ExecutionResult, Method, Request};
use swarmie_providers::ai::{self as provider, Provider};
use swarmie_providers::{
    ModelMethod, ModelProgress, ModelResponse, ModelService, OperationId, Progress,
};
use tembra::Host;
use tokio::task::LocalSet;

fn data(value: Value) -> String {
    format!("data: {value}\n\n")
}
fn event(name: &str, value: Value) -> String {
    format!("event: {name}\ndata: {value}\n\n")
}

#[tokio::test(flavor = "current_thread")]
async fn responses_reasoning_items_preserve_native_continuation() {
    LocalSet::new()
        .run_until(async {
            for (encrypted, tool) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut frames = reply(Provider::OpenAiResponses, tool);
                for frame in &mut frames.parts {
                    *frame = frame.replace("\"output_index\":0", "\"output_index\":1");
                }
                let mut item = json!({"type":"reasoning","id":"rs_fixture","summary":[]});
                if encrypted {
                    item["encrypted_content"] = json!("opaque-fixture");
                }
                frames.parts.insert(
                    0,
                    event(
                        "response.output_item.done",
                        json!({"type":"response.output_item.done","output_index":0,"item":item}),
                    ),
                );
                let last = frames.parts.last_mut().unwrap();
                let mut response = wire(Provider::OpenAiResponses, tool);
                response["output"].as_array_mut().unwrap().insert(0, item);
                *last = event(
                    "response.completed",
                    json!({"type":"response.completed","response":response}),
                );
                let server = Server::streaming(vec![frames]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    qualified_orb(
                        configuration(Provider::OpenAiResponses, &server.endpoint),
                        Rc::new(Secrets),
                        provider::ModelProtection::Unprotected,
                    )
                    .unwrap(),
                ])
                .unwrap();
                let reference = Rc::new(
                    host.get(provider::SERVICE, key().segments(), "alice")
                        .unwrap(),
                );
                let result: ModelResponse = call(
                    &host,
                    reference,
                    "alice",
                    ModelMethod::Stream.as_str(),
                    (OperationId::new(), input(tool)),
                )
                .await
                .unwrap();
                if tool {
                    assert_eq!(result.message.tool_calls.len(), 1);
                } else {
                    assert_eq!(result.message.content, "Done");
                }
                let body = server.calls.borrow()[0].body.clone();
                assert_eq!(body["store"], false);
                assert_eq!(body["stream"], true);
                assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
                assert!(
                    body.get("reasoning")
                        .and_then(|r| r.get("summary"))
                        .is_none()
                );
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn streaming_timeout_limits_idle_reads_not_the_complete_answer() {
    LocalSet::new()
        .run_until(async {
            for (interval, succeeds) in [(20, true), (100, false)] {
                let mut frames = reply(Provider::OpenAi, false);
                frames.interval = Duration::from_millis(interval);
                let server = Server::streaming(vec![frames]).await;
                let mut config = configuration(Provider::OpenAi, &server.endpoint);
                config.connections[0].timeout_ms = std::num::NonZeroU64::new(50).unwrap();
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    qualified_orb(
                        config,
                        Rc::new(Secrets),
                        provider::ModelProtection::Unprotected,
                    )
                    .unwrap(),
                ])
                .unwrap();
                let reference = Rc::new(
                    host.get(provider::SERVICE, key().segments(), "alice")
                        .unwrap(),
                );
                let result = call::<_, ModelResponse>(
                    &host,
                    reference,
                    "alice",
                    ModelMethod::Stream.as_str(),
                    (OperationId::new(), input(false)),
                )
                .await;
                assert_eq!(result.is_ok(), succeeds, "interval {interval}: {result:?}");
                assert_eq!(server.calls.borrow().len(), 1);
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_stream_has_a_bounded_settlement_wait_even_with_continuous_bytes() {
    use futures::FutureExt;
    use swarmie_providers::Model;
    LocalSet::new()
        .run_until(async {
            let mut frames = reply(Provider::OpenAi, false);
            frames.parts = std::iter::once(frames.parts.remove(0))
                .chain(std::iter::repeat_n(": heartbeat\n\n".into(), 100))
                .collect();
            frames.interval = Duration::from_millis(10);
            let server = Server::streaming(vec![frames]).await;
            let mut config = configuration(Provider::OpenAi, &server.endpoint);
            config.connections[0].timeout_ms = std::num::NonZeroU64::new(50).unwrap();
            let model = Rc::new(
                qualified_model(
                    config.connections.remove(0),
                    key().model,
                    Rc::new(Secrets),
                    provider::ModelProtection::Unprotected,
                )
                .unwrap(),
            );
            let host = Host::new(LocalExecutor);
            let error = tokio::time::timeout(
                Duration::from_millis(250),
                host.root("alice", move |ctx| {
                    async move {
                        let progress = Progress::new(|ctx, value| {
                            if !value.content.is_empty() {
                                ctx.cancel().unwrap();
                            }
                        });
                        model
                            .stream(&ctx, OperationId::new(), &input(false), &progress)
                            .await
                    }
                    .boxed_local()
                }),
            )
            .await
            .expect("continuous heartbeats cannot retain cancelled work indefinitely")
            .unwrap_err();
            assert_eq!(error.code, tembra::Code::Cancelled);
            assert_eq!(server.calls.borrow().len(), 1);
            close(&host).await;
            server.finish().await;
        })
        .await;
}

pub(super) fn reply(kind: Provider, tool: bool) -> StreamReply {
    let parts = match kind {
        Provider::OpenAi | Provider::DeepSeek | Provider::Kimi => {
            let chunk = |delta: Value, finish: Value| {
                data(
                    json!({"id":"chat_fixture","object":"chat.completion.chunk","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":finish}]}),
                )
            };
            let mut frames = if tool {
                vec![
                    chunk(
                        json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"counter_add","arguments":"{\"amount\":"}}]}),
                        Value::Null,
                    ),
                    chunk(
                        json!({"tool_calls":[{"index":0,"function":{"arguments":"1}"}}]}),
                        Value::Null,
                    ),
                ]
            } else {
                vec![
                    chunk(json!({"role":"assistant","content":"Do"}), Value::Null),
                    chunk(json!({"content":"ne"}), Value::Null),
                ]
            };
            frames.push(data(json!({"id":"chat_fixture","object":"chat.completion.chunk","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":if tool {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}})));
            frames.push("data: [DONE]\n\n".into());
            frames
        }
        Provider::OpenAiResponses => {
            let mut frames = if tool {
                vec![
                    event(
                        "response.output_item.added",
                        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_fixture","call_id":"call_1","name":"counter_add","arguments":""}}),
                    ),
                    event(
                        "response.function_call_arguments.delta",
                        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_fixture","delta":"{\"amount\":"}),
                    ),
                    event(
                        "response.function_call_arguments.delta",
                        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_fixture","delta":"1}"}),
                    ),
                ]
            } else {
                vec![
                    event(
                        "response.output_text.delta",
                        json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg_fixture","content_index":0,"delta":"Do"}),
                    ),
                    event(
                        "response.output_text.delta",
                        json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg_fixture","content_index":0,"delta":"ne"}),
                    ),
                ]
            };
            frames.push(event(
                "response.completed",
                json!({"type":"response.completed","response":wire(kind, tool)}),
            ));
            frames
        }
        Provider::Anthropic => {
            let mut frames = vec![event(
                "message_start",
                json!({"type":"message_start","message":{"id":"msg_fixture","model":"fixture-model","type":"message","role":"assistant","content":[],"usage":{"input_tokens":7,"output_tokens":0}}}),
            )];
            if tool {
                frames.push(event("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_1","name":"counter_add","input":{}}})));
                for partial in ["{\"amount\":", "1}"] {
                    frames.push(event("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":partial}})));
                }
            } else {
                frames.push(event("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})));
                for text in ["Do", "ne"] {
                    frames.push(event("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})));
                }
            }
            frames.push(event(
                "content_block_stop",
                json!({"type":"content_block_stop","index":0}),
            ));
            frames.push(event("message_delta", json!({"type":"message_delta","delta":{"stop_reason":if tool {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":3}})));
            frames.push(event("message_stop", json!({"type":"message_stop"})));
            frames
        }
        Provider::Gemini => {
            let mut frames = if tool {
                vec![data(
                    json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"counter_add","args":{"amount":1}}}]}}]}),
                )]
            } else {
                ["Do", "ne"].into_iter().map(|text| data(json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]}}]}))).collect()
            };
            frames.push(data(json!({"candidates":[{"content":{"role":"model","parts":[]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3,"totalTokenCount":10}})));
            frames
        }
        Provider::Ollama => {
            let mut frames = if tool {
                vec![format!(
                    "{}\n",
                    json!({"model":"fixture-model","message":{"role":"assistant","content":"","tool_calls":[{"id":"call_1","function":{"name":"counter_add","arguments":{"amount":1}}}]},"done":false})
                )]
            } else {
                ["Do", "ne"]
                    .into_iter()
                    .map(|text| {
                        format!(
                            "{}\n",
                            json!({"model":"fixture-model","message":{"role":"assistant","content":text},"done":false})
                        )
                    })
                    .collect()
            };
            frames.push(format!("{}\n",json!({"model":"fixture-model","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":7,"eval_count":3})));
            frames
        }
    };
    StreamReply {
        parts,
        interval: Duration::from_millis(4),
        content_type: if kind == Provider::Ollama {
            "application/x-ndjson"
        } else {
            "text/event-stream"
        },
    }
}

fn install_agent(host: &Host, counter: Rc<Counter>) -> Rc<tembra::Reference> {
    let binding = host.clone();
    host.install(vec![swarmie_agent_loop::orb(move |d| {
        Ok(Agent::new(
            Rc::new(ModelService::bind(
                &d,
                binding.clone(),
                provider::SERVICE,
                key().segments(),
            )?),
            counter.clone(),
        )
        .with_progress(Progress::bind(&d)?))
    })])
    .unwrap();
    Rc::new(
        host.get(swarmie_agent_loop::SERVICE, vec!["main".into()], "alice")
            .unwrap(),
    )
}

fn request(tool: bool) -> Request {
    let input = input(tool);
    Request {
        context: input.context,
        tools: input.tools,
        max_turns: NonZeroU32::new(3).unwrap(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn all_profiles_publish_text_before_the_final_native_loop_response() {
    LocalSet::new().run_until(async {
        for kind in PROVIDERS {
            let server = Server::streaming(vec![reply(kind, false)]).await;
            let host = Host::new(LocalExecutor);
            host.install(vec![unprotected_orb(configuration(kind, &server.endpoint), Rc::new(Secrets)).unwrap()]).unwrap();
            let reference = install_agent(&host, Rc::new(Counter::default()));
            reference.ready().await.unwrap();
            let previews = Rc::new(RefCell::new(vec![]));
            let collect = previews.clone();
            let (send, receive) = futures::channel::oneshot::channel();
            let send = RefCell::new(Some(send));
            let observation = reference.state::<Option<ModelProgress>>().unwrap().subscribe(move |value| {
                if let Some(value) = value.as_ref() {
                    collect.borrow_mut().push(value.clone());
                    if value.content == "Do" && let Some(send) = send.borrow_mut().take() { let _ = send.send(()); }
                }
                Ok(())
            }).unwrap();
            let running_host = host.clone();
            let running = tokio::task::spawn_local(async move { call::<_,ExecutionResult>(&running_host, reference, "alice", Method::Execute.as_str(), request(false)).await.unwrap().unwrap() });
            tokio::time::timeout(Duration::from_secs(2), receive).await.unwrap_or_else(|error| panic!("{kind:?}: {error}")).unwrap();
            assert!(!running.is_finished(), "{kind:?}: preview must precede settlement");
            let output = running.await.unwrap();
            assert!(matches!(&output.messages[0], swarmie_providers::Message::Assistant(m) if m.content == "Done"));
            assert_eq!(output.usage.input_tokens, 7, "{kind:?}");
            assert_eq!(output.usage.output_tokens, 3, "{kind:?}");
            assert_eq!(previews.borrow().iter().map(|p| p.content.as_str()).collect::<Vec<_>>(), ["", "", "Do", "Done"], "{kind:?}");
            let calls = server.calls.borrow().clone();
            assert_eq!(calls.len(), 1);
            if kind == Provider::Gemini {
                assert!(calls[0].path.contains("streamGenerateContent"));
            } else {
                assert_eq!(calls[0].body["stream"], true);
            }
            drop(calls);
            drop(observation);
            close(&host).await;
            server.finish().await;
        }
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn all_profiles_never_dispatch_a_tool_from_partial_stream_arguments() {
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let first = reply(kind, true);
                let prefixes = first.parts.len() - 1;
                let server = Server::streaming(vec![first, reply(kind, false)]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    unprotected_orb(configuration(kind, &server.endpoint), Rc::new(Secrets))
                        .unwrap(),
                ])
                .unwrap();
                let counter = Rc::new(Counter::default());
                let reference = install_agent(&host, counter.clone());
                let running_host = host.clone();
                let running = tokio::task::spawn_local(async move {
                    call::<_, ExecutionResult>(
                        &running_host,
                        reference,
                        "alice",
                        Method::Execute.as_str(),
                        request(true),
                    )
                    .await
                    .unwrap()
                    .unwrap()
                });
                tokio::time::timeout(Duration::from_secs(2), async {
                    while server.sent.get() < prefixes {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(
                    counter.0.get(),
                    0,
                    "{kind:?}: pending arguments cannot execute"
                );
                let output = running.await.unwrap();
                assert_eq!(counter.0.get(), 1, "{kind:?}");
                assert_eq!(output.messages.len(), 3);
                assert_eq!(output.usage.output_tokens, 6);
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn truncated_streams_stay_pending_and_refuse_automatic_replay_on_all_routes() {
    LocalSet::new()
        .run_until(async {
            for kind in PROVIDERS {
                let mut truncated = reply(kind, false);
                // No stop reason, terminal frame or usage: a visible prefix is not a receipt.
                truncated
                    .parts
                    .truncate(if kind == Provider::Anthropic { 3 } else { 1 });
                let server = Server::streaming(vec![truncated]).await;
                let host = Host::new(LocalExecutor);
                host.install(vec![
                    unprotected_orb(configuration(kind, &server.endpoint), Rc::new(Secrets))
                        .unwrap(),
                ])
                .unwrap();
                let reference = install_agent(&host, Rc::new(Counter::default()));
                let failure = call::<_, ExecutionResult>(
                    &host,
                    reference.clone(),
                    "alice",
                    Method::Execute.as_str(),
                    request(false),
                )
                .await
                .unwrap()
                .unwrap_err();
                let state = *failure.state.unwrap();
                assert!(
                    matches!(
                        state.phase(),
                        swarmie_agent_loop::Phase::ModelPending { .. }
                    ),
                    "{kind:?}"
                );
                assert_eq!(state.context().messages.len(), 1);
                let resumed = call::<_, ExecutionResult>(
                    &host,
                    reference,
                    "alice",
                    Method::Resume.as_str(),
                    state,
                )
                .await
                .unwrap()
                .unwrap_err();
                assert_eq!(
                    resumed.error.reason,
                    swarmie_agent_loop::FailureReason::RecoveryRequired.as_str()
                );
                assert_eq!(server.calls.borrow().len(), 1);
                close(&host).await;
                server.finish().await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_or_oversized_streams_never_commit_or_execute_tools() {
    LocalSet::new().run_until(async {
        let mut malformed = reply(Provider::OpenAi, true);
        malformed.parts[1] = malformed.parts[1].replace("1}", "oops");
        let mut cut_off = reply(Provider::OpenAi, false);
        cut_off.parts[2] = cut_off.parts[2].replace("\"stop\"", "\"length\"");
        let mut oversized = reply(Provider::OpenAi, false);
        oversized.parts[0] = oversized.parts[0].replace("Do", &"x".repeat(300));
        let reasoning = StreamReply { parts: vec![data(json!({"choices":[{"index":0,"delta":{"reasoning_content":"private reasoning"},"finish_reason":null}]}))], ..reply(Provider::OpenAi, false) };
        let invalid_json = StreamReply { parts: vec!["data: private-secret invalid-json\n\n".into()], ..reply(Provider::OpenAi, false) };
        for (frames, limit, expected) in [(malformed, 512*1024, tembra::Code::Invalid), (cut_off, 512*1024, tembra::Code::Invalid), (oversized, 256, tembra::Code::Exhausted), (reasoning, 512*1024, tembra::Code::Invalid), (invalid_json, 512*1024, tembra::Code::Invalid)] {
            let server = Server::streaming(vec![frames]).await;
            let mut config = configuration(Provider::OpenAi, &server.endpoint);
            config.connections[0].response_bytes = std::num::NonZeroUsize::new(limit).unwrap();
            let host = Host::new(LocalExecutor);
            host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()]).unwrap();
            let counter = Rc::new(Counter::default());
            let reference = install_agent(&host, counter.clone());
            let failure = call::<_,ExecutionResult>(&host, reference, "alice", Method::Execute.as_str(), request(true)).await.unwrap().unwrap_err();
            assert_eq!(failure.error.code, expected, "{:?}", failure.error);
            assert!(!format!("{:?}", failure.error).contains("private-secret"));
            let state = failure.state.unwrap();
            assert!(matches!(state.phase(), swarmie_agent_loop::Phase::ModelPending { .. }));
            assert_eq!(state.context().messages.len(), 1);
            assert_eq!(counter.0.get(), 0);
            close(&host).await;
            server.finish().await;
        }
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropped_stream_waiters_keep_physical_ownership_until_transport_settles() {
    LocalSet::new()
        .run_until(async {
            let mut frames = reply(Provider::OpenAi, false);
            frames.interval = Duration::from_millis(20);
            let server = Server::streaming(vec![frames]).await;
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
            reference.ready().await.unwrap();
            let previews = Rc::new(RefCell::new(vec![]));
            let collect = previews.clone();
            let observation = reference
                .state::<Option<ModelProgress>>()
                .unwrap()
                .subscribe(move |value| {
                    collect.borrow_mut().push(value);
                    Ok(())
                })
                .unwrap();
            let caller = host.clone();
            let operation = OperationId::new();
            let running = tokio::task::spawn_local(async move {
                call::<_, ModelResponse>(
                    &caller,
                    reference,
                    "alice",
                    ModelMethod::Stream.as_str(),
                    (operation, input(false)),
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(2), async {
                while !previews
                    .borrow()
                    .iter()
                    .any(|v| v.as_ref().as_ref().is_some_and(|v| v.content == "Do"))
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            running.abort();
            assert!(running.await.unwrap_err().is_cancelled());
            assert!(host.kernel().diagnostics().calls > 0);
            let calls = server.calls.clone();
            server.finish().await;
            drop(observation);
            close(&host).await;
            assert_eq!(calls.borrow().len(), 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_suppresses_previews_without_turning_them_into_receipts() {
    LocalSet::new()
        .run_until(async {
            let server = Server::streaming(vec![reply(Provider::OpenAi, false)]).await;
            let host = Host::new(LocalExecutor);
            host.install(vec![
                unprotected_orb(
                    configuration(Provider::OpenAi, &server.endpoint),
                    Rc::new(Secrets),
                )
                .unwrap(),
            ])
            .unwrap();
            let binding = host.clone();
            let previews = Rc::new(RefCell::new(vec![]));
            let collect = previews.clone();
            host.install(vec![swarmie_agent_loop::orb(move |d| {
                let collect = collect.clone();
                Ok(Agent::new(
                    Rc::new(ModelService::bind(
                        &d,
                        binding.clone(),
                        provider::SERVICE,
                        key().segments(),
                    )?),
                    Rc::new(Counter::default()),
                )
                .with_progress(Progress::new(move |context, value| {
                    if value.content == "Do" {
                        context.cancel().unwrap();
                    }
                    collect.borrow_mut().push(value);
                })))
            })])
            .unwrap();
            let reference = Rc::new(
                host.get(swarmie_agent_loop::SERVICE, vec!["main".into()], "alice")
                    .unwrap(),
            );
            let result = call::<_, ExecutionResult>(
                &host,
                reference,
                "alice",
                Method::Execute.as_str(),
                request(false),
            )
            .await;
            let error = match result {
                Err(error) => error,
                Ok(Err(failure)) => {
                    assert!(matches!(
                        failure.state.unwrap().phase(),
                        swarmie_agent_loop::Phase::ModelPending { .. }
                    ));
                    failure.error
                }
                Ok(Ok(_)) => panic!("cancelled preview cannot become a confirmed result"),
            };
            assert_eq!(error.code, tembra::Code::Cancelled);
            assert_eq!(
                previews
                    .borrow()
                    .iter()
                    .map(|v| v.content.clone())
                    .collect::<Vec<_>>(),
                ["", "", "Do"]
            );
            server.finish().await;
            close(&host).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn coalescing_retains_only_latest_text_and_flushes_the_settled_preview() {
    LocalSet::new().run_until(async {
        let mut frames = reply(Provider::OpenAi, false);
        frames.interval = Duration::ZERO;
        let chunk = frames.parts[0].replace("Do", "x");
        frames.parts.splice(0..2, std::iter::repeat_n(chunk, 128));
        let server = Server::streaming(vec![frames]).await;
        let mut config = configuration(Provider::OpenAi, &server.endpoint);
        config.connections[0].progress_interval_ms = std::num::NonZeroU64::new(60_000).unwrap();
        let host = Host::new(LocalExecutor);
        host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()]).unwrap();
        let reference = install_agent(&host, Rc::new(Counter::default()));
        reference.ready().await.unwrap();
        let previews = Rc::new(RefCell::new(vec![]));
        let collect = previews.clone();
        let observation = reference.state::<Option<ModelProgress>>().unwrap().subscribe(move |value| { if let Some(value) = value.as_ref() { collect.borrow_mut().push(value.content.clone()); } Ok(()) }).unwrap();
        let output = call::<_,ExecutionResult>(&host, reference.clone(), "alice", Method::Execute.as_str(), request(false)).await.unwrap().unwrap();
        assert_eq!(previews.borrow().as_slice(), [String::new(), String::new(), "x".into(), "x".repeat(128)]);
        assert_eq!(reference.state::<Option<ModelProgress>>().unwrap().value().unwrap().as_ref().as_ref().unwrap().content, "x".repeat(128));
        assert!(matches!(&output.messages[0], swarmie_providers::Message::Assistant(m) if m.content.len() == 128));
        drop(observation);
        close(&host).await;
        server.finish().await;
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn coalescing_deadline_flushes_pending_text_while_the_transport_is_idle() {
    LocalSet::new()
        .run_until(async {
            let mut frames = reply(Provider::OpenAi, false);
            frames.interval = Duration::from_millis(5);
            frames
                .parts
                .splice(2..2, std::iter::repeat_n(": keepalive\n\n".into(), 30));
            let total = frames.parts.len();
            let server = Server::streaming(vec![frames]).await;
            let mut config = configuration(Provider::OpenAi, &server.endpoint);
            config.connections[0].progress_interval_ms = std::num::NonZeroU64::new(25).unwrap();
            let host = Host::new(LocalExecutor);
            host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()])
                .unwrap();
            let reference = install_agent(&host, Rc::new(Counter::default()));
            reference.ready().await.unwrap();
            let (send, receive) = futures::channel::oneshot::channel();
            let send = RefCell::new(Some(send));
            let observation = reference
                .state::<Option<ModelProgress>>()
                .unwrap()
                .subscribe(move |value| {
                    if value.as_ref().as_ref().is_some_and(|v| v.content == "Done")
                        && let Some(send) = send.borrow_mut().take()
                    {
                        let _ = send.send(());
                    }
                    Ok(())
                })
                .unwrap();
            let running_host = host.clone();
            let running = tokio::task::spawn_local(async move {
                call::<_, ExecutionResult>(
                    &running_host,
                    reference,
                    "alice",
                    Method::Execute.as_str(),
                    request(false),
                )
                .await
                .unwrap()
                .unwrap()
            });
            tokio::time::timeout(Duration::from_secs(2), receive)
                .await
                .unwrap()
                .unwrap();
            assert!(server.sent.get() < total - 2);
            assert!(!running.is_finished());
            running.await.unwrap();
            drop(observation);
            close(&host).await;
            server.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn streaming_timeout_settles_sse_and_ndjson_without_resending() {
    LocalSet::new()
        .run_until(async {
            for kind in [Provider::OpenAi, Provider::Ollama] {
                let mut frames = reply(kind, false);
                frames.interval = Duration::from_millis(100);
                let server = Server::streaming(vec![frames]).await;
                let mut config = configuration(kind, &server.endpoint);
                config.connections[0].timeout_ms = std::num::NonZeroU64::new(15).unwrap();
                let host = Host::new(LocalExecutor);
                host.install(vec![unprotected_orb(config, Rc::new(Secrets)).unwrap()])
                    .unwrap();
                let reference = install_agent(&host, Rc::new(Counter::default()));
                let failed = call::<_, ExecutionResult>(
                    &host,
                    reference.clone(),
                    "alice",
                    Method::Execute.as_str(),
                    request(false),
                )
                .await
                .unwrap()
                .unwrap_err();
                assert_eq!(failed.error.code, tembra::Code::Unavailable);
                let state = *failed.state.unwrap();
                assert!(matches!(
                    state.phase(),
                    swarmie_agent_loop::Phase::ModelPending { .. }
                ));
                let resumed = call::<_, ExecutionResult>(
                    &host,
                    reference,
                    "alice",
                    Method::Resume.as_str(),
                    state,
                )
                .await
                .unwrap()
                .unwrap_err();
                assert_eq!(
                    resumed.error.reason,
                    swarmie_agent_loop::FailureReason::RecoveryRequired.as_str()
                );
                assert_eq!(server.calls.borrow().len(), 1);
                server.finish().await;
                close(&host).await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_the_initial_preview_prevents_http_dispatch() {
    LocalSet::new()
        .run_until(async {
            let server = Server::streaming(vec![reply(Provider::OpenAi, false)]).await;
            let host = Host::new(LocalExecutor);
            host.install(vec![
                unprotected_orb(
                    configuration(Provider::OpenAi, &server.endpoint),
                    Rc::new(Secrets),
                )
                .unwrap(),
            ])
            .unwrap();
            let binding = host.clone();
            host.install(vec![swarmie_agent_loop::orb(move |d| {
                Ok(Agent::new(
                    Rc::new(ModelService::bind(
                        &d,
                        binding.clone(),
                        provider::SERVICE,
                        key().segments(),
                    )?),
                    Rc::new(Counter::default()),
                )
                .with_progress(Progress::new(|context, value| {
                    assert!(value.content.is_empty());
                    context.cancel().unwrap();
                })))
            })])
            .unwrap();
            let reference = Rc::new(
                host.get(swarmie_agent_loop::SERVICE, vec!["main".into()], "alice")
                    .unwrap(),
            );
            let result = call::<_, ExecutionResult>(
                &host,
                reference,
                "alice",
                Method::Execute.as_str(),
                request(false),
            )
            .await;
            let error = match result {
                Err(error) => error,
                Ok(Err(failure)) => failure.error,
                Ok(Ok(_)) => panic!("initial cancellation must prevent inference"),
            };
            assert_eq!(error.code, tembra::Code::Cancelled);
            assert!(server.calls.borrow().is_empty());
            close(&host).await;
            // No HTTP request was admitted, so dropping the fixture cancels its accept.
            drop(server);
        })
        .await;
}
