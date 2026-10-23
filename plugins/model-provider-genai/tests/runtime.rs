mod support;
use serde_json::json;
use std::{rc::Rc, sync::atomic::Ordering, time::Duration};
use support::*;
use swarmie_model_contracts::*;
use swarmie_model_provider_genai::{self as adapter, Credentials};
use tembra::{ContractToken, core};

#[test]
fn chat_and_independently_named_registry_contract_through_real_tembra() {
    let server = Server::new(vec![Reply::Json(completion("hello", "stop"))]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let registry = host.core().registry(&model_providers().unwrap()).unwrap();
        let table = registry.table(None).unwrap();
        let entry = table.get(adapter::PLUGIN_ID).unwrap();
        let registration: ProviderRegistration =
            serde_json::from_value(entry.data().clone()).unwrap();
        assert_eq!(registration.connections, vec!["primary"]);
        let p = host
            .get(
                ModelEndpoint::new(&registration.provider_contract).unwrap(),
                ModelProviderSchemaInput {},
            )
            .await
            .unwrap();
        assert_eq!(
            p.core().contract().members(""),
            ModelProviderSchema.descriptor().unwrap().members("")
        );
        let r = response(p.generate(request("simple")).await.unwrap());
        assert_eq!(r.message, Message::text(Role::Assistant, "hello"));
        assert_eq!(r.usage.total_tokens, Some(8));
        assert_eq!(r.finish_reason, FinishReason::Stop);
        host.close().await.unwrap();
        assert!(registry.table(None).is_err());
    });
}

#[test]
fn real_sse_is_incremental_ordered_and_final_response_is_coherent() {
    let server = Server::new(vec![Reply::Frames(vec![
        (0, frame(json!({"choices":[{"delta":{"content":"hello"}}]}))),
        (
            70,
            frame(json!({"choices":[{"delta":{"content":" world"}}]})),
        ),
        (
            0,
            frame(
                json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}),
            ),
        ),
        (0, done()),
    ])]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let g = generation(&host, request("stream")).await;
        assert_eq!(
            g.progress().value().unwrap().phase,
            GenerationPhase::Prepared
        );
        assert!(server.requests.lock().unwrap().is_empty());
        let observations = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = observations.clone();
        let subscription = g
            .progress()
            .subscribe(move |s| {
                seen.borrow_mut().push(s.value()?);
                Ok(())
            })
            .unwrap();
        g.start().await.unwrap();
        g.start().await.unwrap();
        wait_for(|| g.progress().value().is_ok_and(|s| !s.events.is_empty())).await;
        let first = g.progress().value().unwrap();
        assert_eq!(first.phase, GenerationPhase::Running);
        assert!(matches!(&first.events[0].event,ModelEvent::TextDelta{text} if text=="hello"));
        let state = settled(&g).await;
        assert!(state.error.is_none(), "{:?}", state.error);
        assert_eq!(
            state.response.unwrap().message,
            Message::text(Role::Assistant, "hello world")
        );
        assert!(
            observations
                .borrow()
                .iter()
                .any(|s| s.phase == GenerationPhase::Running && !s.events.is_empty())
        );
        subscription.unsubscribe();
        host.close().await.unwrap();
    });
}

#[test]
fn explicit_cancel_drops_http_before_publishing_physical_settlement() {
    let server = Server::new(vec![Reply::Hold]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let g = generation(&host, request("cancel")).await;
        g.start().await.unwrap();
        wait_for(|| !server.requests.lock().unwrap().is_empty()).await;
        g.cancel().await.unwrap();
        let s = settled(&g).await;
        assert!(s.physically_settled);
        assert_eq!(s.error.unwrap().kind, ProviderErrorKind::Cancelled);
        wait_for(|| server.disconnected.load(Ordering::SeqCst)).await;
        host.close().await.unwrap();
    });
}

#[test]
fn last_reference_release_and_host_close_stop_actual_http_work() {
    for close_host in [false, true] {
        let server = Server::new(vec![Reply::Hold]);
        run(|driver| async {
            let host = host(&configuration(&server.url), driver).await;
            let g = generation(&host, request("release")).await;
            g.start().await.unwrap();
            wait_for(|| !server.requests.lock().unwrap().is_empty()).await;
            let weak = g.progress();
            if close_host {
                host.close().await.unwrap();
            } else {
                drop(g);
            }
            wait_for(|| server.disconnected.load(Ordering::SeqCst)).await;
            assert!(weak.value().is_err());
            if !close_host {
                host.close().await.unwrap();
            }
        });
    }
}

#[test]
fn timeout_and_concurrency_limit_preserve_submission_uncertainty() {
    let server = Server::new(vec![Reply::Hold]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].timeout_ms = 150;
        config.connections[0].max_concurrent = 1;
        let host = host(&config, driver).await;
        let g = generation(&host, request("timeout")).await;
        g.start().await.unwrap();
        wait_for(|| !server.requests.lock().unwrap().is_empty()).await;
        let p = provider(&host).await;
        let GenerateResult::Error { error } = p.generate(request("busy")).await.unwrap() else {
            panic!("expected capacity refusal");
        };
        assert_eq!(error.kind, ProviderErrorKind::Busy);
        assert_eq!(error.submission, Submission::NotSent);
        let s = settled(&g).await;
        let error = s.error.unwrap();
        assert_eq!(error.kind, ProviderErrorKind::Timeout);
        assert_ne!(error.submission, Submission::NotSent);
        host.close().await.unwrap();
    });
}

#[test]
fn structured_tool_calls_round_trip_ids_arguments_and_reasoning_between_turns() {
    let mut tool = completion("", "tool_calls");
    tool["choices"][0]["message"] = json!({"role":"assistant","content":null,"reasoning_content":"plan","tool_calls":[{"id":"call_exact_42","type":"function","function":{"name":"lookup","arguments":"{\"key\":7}"}}]});
    let server = Server::new(vec![
        Reply::Json(tool),
        Reply::Json(completion("done", "stop")),
    ]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let mut request = request("tools");
        request.tools = vec![ToolDefinition {
            name: "lookup".into(),
            description: "Read a value".into(),
            parameters: json!({"type":"object","properties":{"key":{"type":"integer"}},"required":["key"]}),
        }];
        let response = response(p.generate(request.clone()).await.unwrap());
        assert!(
            response
                .message
                .content
                .iter()
                .any(|p| matches!(p,ContentPart::Reasoning{text} if text=="plan"))
        );
        let call = response
            .message
            .content
            .iter()
            .find_map(|p| match p {
                ContentPart::ToolCall { call } => Some(call),
                _ => None,
            })
            .unwrap();
        assert_eq!(call.id, "call_exact_42");
        assert_eq!(call.arguments, json!({"key":7}));
        request.request_id = "tool-result".into();
        request.messages.push(response.message);
        request.messages.push(Message {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult {
                call_id: "call_exact_42".into(),
                value: json!({"found":true}),
                is_error: false,
            }],
        });
        assert_eq!(
            support::response(p.generate(request).await.unwrap()).message,
            Message::text(Role::Assistant, "done")
        );
        {
            let received = server.requests.lock().unwrap();
            assert_eq!(
                received[1]["messages"][1]["tool_calls"][0]["id"],
                "call_exact_42"
            );
            assert_eq!(received[1]["messages"][2]["tool_call_id"], "call_exact_42");
            assert_eq!(received[1]["messages"][1]["reasoning_content"], "plan");
        }
        host.close().await.unwrap();
    });
}

#[test]
fn streaming_tool_argument_snapshots_are_replacements_with_structured_final_values() {
    let frames = vec![
        (
            0,
            frame(
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_stream","function":{"name":"lookup","arguments":"{\"key\":"}}]}}]}),
            ),
        ),
        (
            40,
            frame(
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"7}"}}]}}]}),
            ),
        ),
        (
            0,
            frame(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})),
        ),
        (0, done()),
    ];
    let server = Server::new(vec![Reply::Frames(frames)]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let mut req = request("stream-tools");
        req.tools = vec![ToolDefinition {
            name: "lookup".into(),
            description: String::new(),
            parameters: json!({"type":"object"}),
        }];
        let g = generation(&host, req).await;
        g.start().await.unwrap();
        wait_for(|| g.progress().value().is_ok_and(|s| !s.events.is_empty())).await;
        let first = g.progress().value().unwrap();
        assert!(
            matches!(&first.events[0].event,ModelEvent::ToolCallSnapshot{arguments_json,..} if arguments_json=="{\"key\":")
        );
        let final_state = settled(&g).await;
        assert!(final_state.error.is_none(), "{:?}", final_state.error);
        let r = final_state.response.unwrap();
        assert_eq!(r.finish_reason, FinishReason::ToolCalls);
        assert!(r.message.content.iter().any(|p|matches!(p,ContentPart::ToolCall{call} if call.id=="call_stream" && call.arguments==json!({"key":7}))));
        assert_eq!(
            server.requests.lock().unwrap()[0]["parallel_tool_calls"],
            false
        );
        host.close().await.unwrap();
    });
}

#[test]
fn structured_output_and_all_finish_categories_are_preserved() {
    let reasons = [
        ("stop", FinishReason::Stop),
        ("length", FinishReason::Length),
        ("tool_calls", FinishReason::ToolCalls),
        ("content_filter", FinishReason::ContentFilter),
        ("stop_sequence", FinishReason::StopSequence),
        ("new_reason", FinishReason::Other),
    ];
    let server = Server::new(
        reasons
            .iter()
            .map(|(r, _)| Reply::Json(completion("{\"answer\":7}", r)))
            .collect(),
    );
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        for (index, (raw, finish)) in reasons.into_iter().enumerate() {
            let mut req = request(&format!("schema-{index}"));
            req.options.response_format = Some(ResponseFormat::JsonSchema {
                name: "answer".into(),
                schema: json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"]}),
            });
            let r = response(p.generate(req).await.unwrap());
            assert_eq!(r.finish_reason, finish);
            assert_eq!(r.raw_finish_reason.as_deref(), Some(raw));
        }
        assert_eq!(
            server.requests.lock().unwrap()[0]["response_format"]["type"],
            "json_schema"
        );
        host.close().await.unwrap();
    });
}

#[test]
fn http_errors_and_parsing_failures_never_expose_echoed_credentials() {
    struct Secret;
    impl Credentials for Secret {
        fn resolve(&self, _: &str) -> Option<String> {
            Some("private-key-never-portable".into())
        }
    }
    let server = Server::new(vec![
        Reply::Http(429, "private-key-never-portable".into()),
        Reply::Json(json!({"secret":"private-key-never-portable"})),
    ]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].credential_env = Some("TEST_KEY_REFERENCE".into());
        let host = tembra::Host::with_driver("secrets", "test", driver.clone()).unwrap();
        host.install(
            adapter::facet_with_credentials(&config, driver, Rc::new(Secret)).unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
        let p = provider(&host).await;
        for (id, kind) in [
            ("http", ProviderErrorKind::RateLimited),
            ("parsing", ProviderErrorKind::Parsing),
        ] {
            let result = p.generate(request(id)).await.unwrap();
            assert!(!format!("{result:?}").contains("private-key"));
            let GenerateResult::Error { error } = result else {
                panic!("expected failure");
            };
            assert_eq!(error.kind, kind);
            if id == "http" {
                assert_eq!(error.http_status, Some(429));
                assert_eq!(error.retry_after_ms, Some(2000));
            }
        }
        let registry = host.core().registry(&model_providers().unwrap()).unwrap();
        assert!(
            !format!(
                "{:?}",
                registry
                    .table(None)
                    .unwrap()
                    .get(adapter::PLUGIN_ID)
                    .unwrap()
                    .data()
            )
            .contains("private-key")
        );
        host.close().await.unwrap();
    });
}

#[test]
fn malformed_midstream_and_parallel_tool_loss_fail_without_false_completion() {
    for tail in [
        "data: {broken-json}\n\n".to_owned(),
        frame(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"one","arguments":"{}"}},{"index":1,"id":"b","function":{"name":"two","arguments":"{}"}}]}}]}),
        ),
    ] {
        let server = Server::new(vec![Reply::Frames(vec![
            (
                0,
                frame(json!({"choices":[{"delta":{"content":"partial"}}]})),
            ),
            (40, tail),
            (0, done()),
        ])]);
        run(|driver| async {
            let host = host(&configuration(&server.url), driver).await;
            let g = generation(&host, request("failed-stream")).await;
            g.start().await.unwrap();
            wait_for(|| g.progress().value().is_ok_and(|s| !s.events.is_empty())).await;
            let s = settled(&g).await;
            assert!(s.response.is_none());
            assert!(matches!(
                s.error.unwrap().kind,
                ProviderErrorKind::Parsing | ProviderErrorKind::DataLoss
            ));
            host.close().await.unwrap();
        });
    }
}

#[test]
fn multiple_configured_models_and_connections_and_dynamic_discovery() {
    let one = Server::new(vec![
        Reply::Json(completion("one", "stop")),
        Reply::Json(json!({"data":[{"id":"fixture-model"},{"id":"second-model"}]})),
    ]);
    let two = Server::new(vec![Reply::Json(completion("two", "stop"))]);
    run(|driver| async {
        let mut config = configuration(&one.url);
        let mut extra = config.connections[0].clone();
        extra.id = "secondary".into();
        extra.base_url = Some(two.url.clone());
        config.connections.push(extra);
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        assert_eq!(
            response(p.generate(request("one")).await.unwrap()).message,
            Message::text(Role::Assistant, "one")
        );
        let mut req = request("two");
        req.model.connection = "secondary".into();
        req.model.model = "second-model".into();
        assert_eq!(
            response(p.generate(req).await.unwrap()).message,
            Message::text(Role::Assistant, "two")
        );
        assert_eq!(two.requests.lock().unwrap()[0]["model"], "second-model");
        let ModelList::Models { models } = p.models("primary".into()).await.unwrap() else {
            panic!("expected discovered models");
        };
        assert_eq!(models.len(), 2);
        assert_eq!(
            p.capabilities(request("capabilities").model)
                .await
                .unwrap()
                .developer_role,
            Support::ModelDependent
        );
        host.close().await.unwrap();
    });
}

#[test]
fn payload_limits_and_unsupported_features_refuse_before_http() {
    let server = Server::new(vec![]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.limits.request_bytes = 1024;
        config.connections[0].provider = "ollama".into();
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        let mut cases = vec![];
        let mut req = request("large");
        req.messages[0] = Message::text(Role::User, "x".repeat(2048));
        cases.push((req, ProviderErrorKind::PayloadLimit));
        let mut req = request("developer");
        req.messages[0].role = Role::Developer;
        cases.push((req, ProviderErrorKind::Unsupported));
        let mut req = request("ollama-schema");
        req.options.response_format = Some(ResponseFormat::JsonSchema {
            name: "test".into(),
            schema: json!({"type":"object"}),
        });
        cases.push((req, ProviderErrorKind::Unsupported));
        let mut req = request("unsupported-continuation-format");
        req.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentPart::Extension {
                extension: Extension {
                    namespace: "swarmie.continuation.ollama".into(),
                    version: 1,
                    value: json!({"kind":"custom_content","value":{"type":"text","text":"opaque"}}),
                },
            }],
        });
        cases.push((req, ProviderErrorKind::Unsupported));
        for (req, kind) in cases {
            let GenerateResult::Error { error } = p.generate(req).await.unwrap() else {
                panic!("expected refusal");
            };
            assert_eq!(error.kind, kind);
            assert_eq!(error.submission, Submission::NotSent);
        }
        assert!(server.requests.lock().unwrap().is_empty());
        host.close().await.unwrap();
    });
}

#[test]
fn backpressure_keeps_window_bounded_and_acknowledgement_resumes_stream() {
    let mut frames = (0..40)
        .map(|_| (0, frame(json!({"choices":[{"delta":{"content":"x"}}]}))))
        .collect::<Vec<_>>();
    frames.push((
        0,
        frame(json!({"choices":[{"delta":{},"finish_reason":"stop"}]})),
    ));
    frames.push((0, done()));
    let server = Server::new(vec![Reply::Frames(frames)]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.limits.window_events = 8;
        config.limits.batch_events = 4;
        let host = host(&config, driver).await;
        let g = generation(&host, request("backpressure")).await;
        g.start().await.unwrap();
        wait_for(|| g.progress().value().is_ok_and(|s| s.events.len() == 8)).await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        let paused = g.progress().value().unwrap();
        assert_eq!(paused.events.len(), 8);
        assert_eq!(paused.phase, GenerationPhase::Running);
        assert!(g.acknowledge(9).await.is_err());
        g.acknowledge(4).await.unwrap();
        g.acknowledge(2).await.unwrap();
        let s = settled(&g).await;
        assert_eq!(
            s.response.unwrap().message,
            Message::text(Role::Assistant, "x".repeat(40))
        );
        assert!(s.events.len() <= 8);
        host.close().await.unwrap();
    });
}

#[test]
fn signed_thinking_is_preserved_as_opaque_continuation_in_tool_roundtrip() {
    let server = Server::new(vec![
        Reply::Json(
            json!({"id":"msg_signed","type":"message","role":"assistant","model":"fixture-model","content":[{"type":"thinking","thinking":"signed plan","signature":"opaque-signature-123"},{"type":"tool_use","id":"signed_call","name":"lookup","input":{"key":7}}],"stop_reason":"tool_use","usage":{"input_tokens":9,"output_tokens":4}}),
        ),
        Reply::Json(
            json!({"id":"msg_next","type":"message","role":"assistant","model":"fixture-model","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":2}}),
        ),
    ]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "anthropic".into();
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        let mut req = request("signed");
        req.tools = vec![ToolDefinition {
            name: "lookup".into(),
            description: "Lookup".into(),
            parameters: json!({"type":"object","properties":{"key":{"type":"integer"}}}),
        }];
        let r = response(p.generate(req.clone()).await.unwrap());
        assert!(r.message.content.iter().any(|p|matches!(p,ContentPart::Extension{extension} if extension.namespace=="swarmie.continuation.anthropic")));
        req.request_id = "signed-next".into();
        let mut continuation = r.message;
        continuation
            .content
            .retain(|p| !matches!(p, ContentPart::Reasoning { .. }));
        req.messages.push(continuation);
        req.messages.push(Message {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult {
                call_id: "signed_call".into(),
                value: json!({"value":7}),
                is_error: false,
            }],
        });
        let r = response(p.generate(req).await.unwrap());
        assert_eq!(r.message, Message::text(Role::Assistant, "done"));
        {
            let requests = server.requests.lock().unwrap();
            let content = requests[1]["messages"][1]["content"].as_array().unwrap();
            assert!(content.iter().any(|p| p["type"] == "thinking"
                && p["signature"] == "opaque-signature-123"
                && p["thinking"] == "signed plan"));
        }
        host.close().await.unwrap();
    });
}

#[test]
fn oversized_responses_and_events_are_bounded_failures() {
    let server = Server::new(vec![
        Reply::Json(completion(&"x".repeat(2048), "stop")),
        Reply::Frames(vec![
            (
                0,
                frame(json!({"choices":[{"delta":{"content":"x".repeat(2048)}}]})),
            ),
            (0, done()),
        ]),
    ]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.limits.response_bytes = 1024;
        config.limits.event_bytes = 512;
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        let GenerateResult::Error { error } = p.generate(request("large-response")).await.unwrap()
        else {
            panic!("expected response limit");
        };
        assert_eq!(error.kind, ProviderErrorKind::PayloadLimit);
        let g = generation(&host, request("large-event")).await;
        g.start().await.unwrap();
        let s = settled(&g).await;
        assert_eq!(s.error.unwrap().kind, ProviderErrorKind::PayloadLimit);
        assert!(s.events.is_empty());
        host.close().await.unwrap();
    });
}

#[test]
fn generation_object_admission_is_bounded_and_slots_are_released_on_drop() {
    let server = Server::new(vec![]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.limits.max_generations = 1;
        let host = host(&config, driver).await;
        let g = generation(&host, request("first")).await;
        let refused = host
            .get(
                GenerationEndpoint::new(adapter::GENERATION_CONTRACT).unwrap(),
                GenerationInput {
                    request: request("second"),
                },
            )
            .await;
        assert!(refused.is_err());
        drop(g);
        tokio::time::sleep(Duration::from_millis(15)).await;
        let _second = generation(&host, request("third")).await;
        assert!(server.requests.lock().unwrap().is_empty());
        host.close().await.unwrap();
    });
}

#[test]
fn fresh_host_can_reconnect_without_replaying_retired_generation_automatically() {
    let server = Server::new(vec![
        Reply::Json(completion("first", "stop")),
        Reply::Json(completion("second", "stop")),
    ]);
    run(|driver| async move {
        for (id, expected) in [("attempt-one", "first"), ("attempt-two", "second")] {
            let host = host(&configuration(&server.url), driver.clone()).await;
            let p = provider(&host).await;
            assert_eq!(
                response(p.generate(request(id)).await.unwrap()).message,
                Message::text(Role::Assistant, expected)
            );
            host.close().await.unwrap();
        }
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    });
}

#[test]
#[ignore = "measurement; run explicitly through the root models-benchmark recipe"]
fn benchmark_batched_state_publications() {
    for batch in [1, 32] {
        let mut frames = (0..256)
            .map(|_| (0, frame(json!({"choices":[{"delta":{"content":"x"}}]}))))
            .collect::<Vec<_>>();
        frames.push((
            0,
            frame(json!({"choices":[{"delta":{},"finish_reason":"stop"}]})),
        ));
        frames.push((0, done()));
        let server = Server::new(vec![Reply::Frames(frames)]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.limits.batch_events = batch;
            config.connections[0].timeout_ms = 30_000;
            let host = host(&config, driver).await;
            let g = generation(&host, request("benchmark")).await;
            let count = Rc::new(std::cell::Cell::new(0));
            let updates = count.clone();
            let subscription = g
                .progress()
                .subscribe(move |_| {
                    updates.set(updates.get() + 1);
                    Ok(())
                })
                .unwrap();
            let started = std::time::Instant::now();
            g.start().await.unwrap();
            let s = settled(&g).await;
            assert!(s.error.is_none());
            println!(
                "{}",
                json!({"profile":"debug","chunks":256,"batch_events":batch,"state_publications":count.get(),"elapsed_us":started.elapsed().as_micros()})
            );
            subscription.unsubscribe();
            host.close().await.unwrap();
        });
    }
}

#[test]
fn streaming_provider_logs_cannot_echo_credentials() {
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Capture(Arc<Mutex<String>>);
    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Visitor<'a>(&'a mut String);
            impl tracing::field::Visit for Visitor<'_> {
                fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    self.0.push_str(&format!("{value:?}"));
                }
            }
            event.record(&mut Visitor(&mut self.0.lock().unwrap()));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    let captured = Arc::new(Mutex::new(String::new()));
    let server = Server::new(vec![Reply::Http(429, "fixture-private-key".into())]);
    tracing::subscriber::with_default(Capture(captured.clone()), || {
        run(|driver| async {
            let host = host(&configuration(&server.url), driver).await;
            let g = generation(&host, request("log-redaction")).await;
            g.start().await.unwrap();
            let s = settled(&g).await;
            assert_eq!(s.error.unwrap().kind, ProviderErrorKind::RateLimited);
            tracing::info!("sanitized-capture-marker");
            host.close().await.unwrap();
        })
    });
    let logs = captured.lock().unwrap();
    assert!(logs.contains("sanitized-capture-marker"));
    assert!(!logs.contains("fixture-private-key"));
}

#[test]
fn named_endpoints_reject_nonportable_numbers_before_normalization_or_http() {
    let server = Server::new(vec![]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let mut req = request("nonportable");
        req.options.temperature = Some(f64::NAN);
        let error = p.generate(req).await.unwrap_err();
        assert_eq!(error.reason, "model_argument");
        let g = generation(&host, request("ack-range")).await;
        assert!(g.acknowledge(u64::MAX).await.is_err());
        assert!(server.requests.lock().unwrap().is_empty());
        host.close().await.unwrap();
    });
}

#[test]
fn call_ids_can_repeat_after_their_prior_turn_has_been_resolved() {
    let server = Server::new(vec![Reply::Json(completion("done", "stop"))]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let mut req = request("repeated-call-id");
        for _ in 0..2 {
            req.messages.push(Message {
                role: Role::Assistant,
                content: vec![ContentPart::ToolCall {
                    call: ToolCall {
                        id: "provider-reused-id".into(),
                        name: "lookup".into(),
                        arguments: json!({}),
                        extensions: vec![],
                    },
                }],
            });
            req.messages.push(Message {
                role: Role::Tool,
                content: vec![ContentPart::ToolResult {
                    call_id: "provider-reused-id".into(),
                    value: json!({"error":"unavailable"}),
                    is_error: true,
                }],
            });
        }
        assert!(matches!(
            p.generate(req).await.unwrap(),
            GenerateResult::Response { .. }
        ));
        let received = server.requests.lock().unwrap()[0].clone();
        assert!(
            received["messages"][2]["content"]
                .as_str()
                .unwrap()
                .contains("is_error")
        );
        host.close().await.unwrap();
    });
}

#[test]
fn model_discovery_refuses_incomplete_results_instead_of_truncating() {
    let server = Server::new(vec![Reply::Json(
        json!({"data":(0..1025).map(|n|json!({"id":format!("model-{n}")})).collect::<Vec<_>>()}),
    )]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let ModelList::Error { error } = p.models("primary".into()).await.unwrap() else {
            panic!("oversized discovery must fail");
        };
        assert_eq!(error.kind, ProviderErrorKind::PayloadLimit);
        host.close().await.unwrap();
    });
}

#[test]
fn native_ollama_chat_and_ndjson_stream_work_without_credentials() {
    let complete = json!({"model":"fixture-model","message":{"role":"assistant","content":"local"},"done":true,"done_reason":"stop","prompt_eval_count":5,"eval_count":3});
    let server = Server::new(vec![
        Reply::Json(complete),
        Reply::Frames(vec![
            (
                0,
                format!(
                    "{}\n",
                    json!({"model":"fixture-model","message":{"thinking":"plan"},"done":false})
                ),
            ),
            (
                20,
                format!(
                    "{}\n",
                    json!({"model":"fixture-model","message":{"content":"local stream"},"done":false})
                ),
            ),
            (
                0,
                format!(
                    "{}\n",
                    json!({"model":"fixture-model","message":{"content":""},"done":true,"done_reason":"stop","prompt_eval_count":5,"eval_count":3})
                ),
            ),
        ]),
    ]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "ollama".into();
        config.connections[0].credential_env = None;
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        assert_eq!(
            response(p.generate(request("local-chat")).await.unwrap()).message,
            Message::text(Role::Assistant, "local")
        );
        let g = generation(&host, request("local-stream")).await;
        g.start().await.unwrap();
        let s = settled(&g).await;
        assert!(s.error.is_none(), "{:?}", s.error);
        let r = s.response.unwrap();
        assert!(
            r.message
                .content
                .iter()
                .any(|p| matches!(p,ContentPart::Text{text} if text=="local stream"))
        );
        assert_eq!(r.usage.total_tokens, Some(8));
        host.close().await.unwrap();
    });
}
