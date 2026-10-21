#[expect(
    dead_code,
    reason = "shared HTTP fixture also serves cancellation and transport suites"
)]
mod support;
use serde_json::{Value, json};
use support::*;
use swarmie_model_contracts::*;

fn tool() -> ToolDefinition {
    ToolDefinition {
        name: "lookup".into(),
        description: "Lookup".into(),
        parameters: json!({"type":"object","properties":{"key":{"type":"integer"}}}),
    }
}
fn schema() -> ResponseFormat {
    ResponseFormat::JsonSchema {
        name: "answer".into(),
        schema: json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"],"additionalProperties":false}),
    }
}
fn tool_response() -> Value {
    let mut value = completion("", "tool_calls");
    value["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"call-original","type":"function","function":{"name":"lookup","arguments":"{\"key\":7}"}}]});
    value
}
fn tool_result(id: &str) -> Message {
    Message {
        role: Role::Tool,
        content: vec![ContentPart::ToolResult {
            call_id: id.into(),
            value: json!({"found":true}),
            is_error: false,
        }],
    }
}
fn serialized_message(message: Message, redact: bool) -> Message {
    let mut message: Message =
        serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap();
    if redact {
        message
            .content
            .retain(|part| !matches!(part, ContentPart::Reasoning { .. }));
    }
    message
}
fn tool_calls(message: &Message) -> Vec<&ToolCall> {
    message
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::ToolCall { call } => Some(call),
            _ => None,
        })
        .collect()
}

#[test]
fn tools_with_json_schema_accept_tool_only_generate_and_streaming() {
    let server = Server::new(vec![
        Reply::Json(tool_response()),
        Reply::Frames(vec![
            (
                0,
                frame(
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-original","function":{"name":"lookup","arguments":"{\"key\":7}"}}]}}]}),
                ),
            ),
            (
                0,
                frame(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})),
            ),
            (0, done()),
        ]),
        Reply::Json(completion("plain text", "stop")),
    ]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let mut req = request("schema-and-tools");
        req.tools = vec![tool()];
        req.options.response_format = Some(schema());
        let r = response(p.generate(req.clone()).await.unwrap());
        assert_eq!(r.finish_reason, FinishReason::ToolCalls);
        assert_eq!(tool_calls(&r.message)[0].id, "call-original");
        assert_eq!(tool_calls(&r.message)[0].arguments, json!({"key":7}));
        req.request_id = "schema-stream-tools".into();
        let g = generation(&host, req.clone()).await;
        g.start().await.unwrap();
        let state = settled(&g).await;
        assert!(state.error.is_none(), "{:?}", state.error);
        let r = state.response.unwrap();
        assert_eq!(r.finish_reason, FinishReason::ToolCalls);
        assert_eq!(tool_calls(&r.message)[0].arguments, json!({"key":7}));
        req.request_id = "invalid-json-answer".into();
        assert!(
            matches!(p.generate(req).await.unwrap(), GenerateResult::Error { error } if error.kind==ProviderErrorKind::Parsing)
        );
        host.close().await.unwrap();
    });
}

#[test]
fn unqualified_tool_json_combinations_and_ignored_json_modes_are_refused_before_http() {
    let server = Server::new(vec![]);
    for provider_name in ["gemini", "ollama"] {
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = provider_name.into();
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let mut req = request("unsupported-combination");
            req.tools = vec![tool()];
            req.options.response_format = Some(ResponseFormat::Json);
            assert!(
                matches!(p.generate(req.clone()).await.unwrap(), GenerateResult::Error { error } if error.kind==ProviderErrorKind::Unsupported && error.submission==Submission::NotSent)
            );
            assert!(
                host.get(
                    GenerationEndpoint::new(swarmie_model_provider_genai::GENERATION_CONTRACT)
                        .unwrap(),
                    GenerationInput { request: req }
                )
                .await
                .is_err()
            );
            host.close().await.unwrap();
        });
    }
    assert!(server.requests.lock().unwrap().is_empty());
}

fn anthropic(content: Value, stop: &str) -> Value {
    json!({"id":"msg_signed","type":"message","role":"assistant","model":"fixture-model","content":content,"stop_reason":stop,"usage":{"input_tokens":9,"output_tokens":4}})
}
fn ant_content() -> Value {
    json!([
        {"type":"text","text":"before"},
        {"type":"thinking","thinking":"first reasoning","signature":"opaque-first"},
        {"type":"tool_use","id":"call-a","name":"lookup","input":{"key":7}},
        {"type":"text","text":"between"},
        {"type":"thinking","thinking":"second reasoning","signature":"opaque-second"},
        {"type":"tool_use","id":"call-b","name":"lookup","input":{"key":8}},
        {"type":"redacted_thinking","data":"opaque-redacted"}
    ])
}

#[test]
fn anthropic_interleaved_signed_blocks_survive_serialization_and_filtered_replay() {
    for redact in [false, true] {
        let content = ant_content();
        let server = Server::new(vec![
            Reply::Json(anthropic(content.clone(), "tool_use")),
            Reply::Json(anthropic(
                json!([{"type":"text","text":"done"}]),
                "end_turn",
            )),
        ]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = "anthropic".into();
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let mut req = request("signed-order");
            req.tools = vec![tool()];
            req.options.response_format = Some(schema());
            let r = response(p.generate(req.clone()).await.unwrap());
            assert_eq!(
                tool_calls(&r.message)
                    .iter()
                    .map(|c| c.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["call-a", "call-b"]
            );
            assert_eq!(
                r.message
                    .content
                    .iter()
                    .filter(|p| matches!(p, ContentPart::Reasoning { .. }))
                    .count(),
                2
            );
            req.options.response_format = None;
            req.request_id = format!("signed-replay-{redact}");
            req.messages.push(serialized_message(r.message, redact));
            req.messages.push(tool_result("call-a"));
            req.messages.push(tool_result("call-b"));
            response(p.generate(req).await.unwrap());
            assert_eq!(
                server.requests.lock().unwrap()[1]["messages"][1]["content"],
                content
            );
            host.close().await.unwrap();
        });
    }
}

fn responses(output: Value) -> Value {
    json!({"id":"resp_original","object":"response","model":"fixture-model","status":"completed","output":output,"usage":{"input_tokens":8,"output_tokens":4,"total_tokens":12}})
}
fn resp_output() -> Value {
    json!([
        {"id":"message-a","type":"message","role":"assistant","content":[{"type":"output_text","text":"before"}]},
        {"id":"reason-a","type":"reasoning","summary":[{"type":"summary_text","text":"first summary"}],"encrypted_content":"opaque-a"},
        {"id":"fc-a","type":"function_call","call_id":"call-a","name":"lookup","arguments":"{\"key\":7}"},
        {"id":"reason-b","type":"reasoning","summary":[{"type":"summary_text","text":"second summary"}],"encrypted_content":"opaque-b"},
        {"id":"fc-b","type":"function_call","call_id":"call-b","name":"lookup","arguments":"{\"key\":8}"}
    ])
}

#[test]
fn responses_reasoning_item_ids_summaries_and_encrypted_content_keep_original_positions() {
    for streaming in [false, true] {
        let output = resp_output();
        let first = if streaming {
            let mut frames = vec![(
                0,
                frame(
                    json!({"type":"response.output_text.delta","delta":"before","output_index":0,"content_index":0}),
                ),
            )];
            frames.push((0,frame(json!({"type":"response.reasoning_summary_text.delta","delta":"first summary","output_index":1,"summary_index":0}))));
            frames.push((0,frame(json!({"type":"response.reasoning_summary_text.delta","delta":"second summary","output_index":3,"summary_index":0}))));
            for (i, item) in output.as_array().unwrap().iter().enumerate() {
                frames.push((
                    0,
                    frame(json!({"type":"response.output_item.done","output_index":i,"item":item})),
                ));
            }
            frames.push((
                0,
                frame(json!({"type":"response.completed","response":responses(output.clone())})),
            ));
            Reply::Frames(frames)
        } else {
            Reply::Json(responses(output.clone()))
        };
        let server = Server::new(vec![
            first,
            Reply::Json(responses(
                json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}]),
            )),
        ]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = "openai_resp".into();
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let mut req = request("responses-order");
            req.tools = vec![tool()];
            req.options.response_format = Some(schema());
            let r = if streaming {
                let g = generation(&host, req.clone()).await;
                g.start().await.unwrap();
                let s = settled(&g).await;
                assert!(s.error.is_none(), "{:?}", s.error);
                s.response.unwrap()
            } else {
                response(p.generate(req.clone()).await.unwrap())
            };
            assert_eq!(r.finish_reason, FinishReason::ToolCalls);
            assert_eq!(
                r.message
                    .content
                    .iter()
                    .filter(|p| matches!(p, ContentPart::Reasoning { .. }))
                    .count(),
                2
            );
            req.options.response_format = None;
            req.request_id = "responses-replay".into();
            req.messages.push(serialized_message(r.message, true));
            req.messages.push(tool_result("call-a"));
            req.messages.push(tool_result("call-b"));
            response(p.generate(req).await.unwrap());
            {
                let received = server.requests.lock().unwrap();
                let input = received[1]["input"].as_array().unwrap();
                assert_eq!(input[2], output[1]);
                assert_eq!(input[4], output[3]);
                assert_eq!(input.iter().filter(|p| p["type"] == "reasoning").count(), 2);
                assert_eq!(input[3]["call_id"], "call-a");
                assert_eq!(input[5]["call_id"], "call-b");
            }
            host.close().await.unwrap();
        });
    }
}

#[test]
fn conflicting_reasoning_and_replay_are_rejected_without_submitting() {
    let server = Server::new(vec![]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let mut req = request("conflicting-replay");
        req.messages.push(Message {
            role: Role::Assistant,
            content: vec![
                ContentPart::Reasoning {
                    text: "modified".into(),
                },
                ContentPart::Extension {
                    extension: Extension {
                        namespace: "swarmie.continuation.openai".into(),
                        version: 2,
                        value: json!({"kind":"reasoning_replay","value":"original"}),
                    },
                },
            ],
        });
        assert!(
            matches!(p.generate(req).await.unwrap(),GenerateResult::Error{error} if error.kind==ProviderErrorKind::InvalidRequest && error.submission==Submission::NotSent)
        );
        assert!(server.requests.lock().unwrap().is_empty());
        host.close().await.unwrap();
    });
}

fn gemini(parts: Value) -> Value {
    json!({"modelVersion":"fixture-model","candidates":[{"content":{"role":"model","parts":parts},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":4,"totalTokenCount":9}})
}

#[test]
fn gemini_interleaved_signed_thoughts_and_distinct_call_signatures_keep_associations() {
    let parts = json!([
        {"text":"before"},
        {"thought":true,"text":"first summary","thoughtSignature":"opaque-thought-a"},
        {"functionCall":{"id":"call-a","name":"lookup","args":{"key":7}},"thoughtSignature":"opaque-call-a"},
        {"text":"between"},
        {"thought":true,"text":"second summary","thoughtSignature":"opaque-thought-b"},
        {"functionCall":{"id":"call-b","name":"lookup","args":{"key":8}},"thoughtSignature":"opaque-call-b"}
    ]);
    for streaming in [false, true] {
        let first = if streaming {
            let mut frames = Vec::new();
            for (index, part) in parts.as_array().unwrap().iter().enumerate() {
                let mut payload = gemini(json!([part]));
                if index + 1 != parts.as_array().unwrap().len() {
                    payload["candidates"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("finishReason");
                }
                frames.push((0, frame(payload)));
            }
            Reply::Frames(frames)
        } else {
            Reply::Json(gemini(parts.clone()))
        };
        let server = Server::new(vec![first, Reply::Json(gemini(json!([{"text":"done"}])))]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = "gemini".into();
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let mut req = request("gemini-signed");
            req.tools = vec![tool()];
            let r = if streaming {
                let g = generation(&host, req.clone()).await;
                g.start().await.unwrap();
                let state = settled(&g).await;
                assert!(state.error.is_none(), "{:?}", state.error);
                state.response.unwrap()
            } else {
                response(p.generate(req.clone()).await.unwrap())
            };
            assert_eq!(r.finish_reason, FinishReason::ToolCalls);
            assert_eq!(
                r.message
                    .content
                    .iter()
                    .filter(|p| matches!(p, ContentPart::Reasoning { .. }))
                    .count(),
                2
            );
            req.request_id = "gemini-signed-replay".into();
            req.messages.push(serialized_message(r.message, true));
            req.messages.push(tool_result("call-a"));
            req.messages.push(tool_result("call-b"));
            response(p.generate(req).await.unwrap());
            {
                let received = server.requests.lock().unwrap();
                assert_eq!(received[1]["contents"][1]["parts"], parts);
            }
            host.close().await.unwrap();
        });
    }
}

#[test]
fn structured_output_does_not_replace_tool_commentary_or_incomplete_finish_with_json_error() {
    let mut tools = tool_response();
    tools["choices"][0]["message"]["content"] = "I will look that up.".into();
    let server = Server::new(vec![
        Reply::Json(tools),
        Reply::Json(completion("{\"unfinished\":", "length")),
        Reply::Json(completion("refused", "content_filter")),
    ]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        for (i, finish) in [
            FinishReason::ToolCalls,
            FinishReason::Length,
            FinishReason::ContentFilter,
        ]
        .into_iter()
        .enumerate()
        {
            let mut req = request(&format!("structured-branch-{i}"));
            req.tools = vec![tool()];
            req.options.response_format = Some(schema());
            assert_eq!(
                response(p.generate(req).await.unwrap()).finish_reason,
                finish
            );
        }
        host.close().await.unwrap();
    });
}

#[test]
fn anthropic_stream_preserves_interleaved_signed_blocks_and_tool_ids_after_serialized_replay() {
    let mut frames = Vec::new();
    for (index, block) in ant_content().as_array().unwrap().iter().take(6).enumerate() {
        let mut initial = block.clone();
        let delta = match block["type"].as_str().unwrap() {
            "text" => {
                initial["text"] = "".into();
                json!({"type":"text_delta","text":block["text"]})
            }
            "thinking" => {
                initial["thinking"] = "".into();
                initial["signature"] = "".into();
                json!({"type":"thinking_delta","thinking":block["thinking"]})
            }
            "tool_use" => {
                initial["input"] = json!({});
                json!({"type":"input_json_delta","partial_json":block["input"].to_string()})
            }
            _ => unreachable!(),
        };
        frames.push((
            0,
            anthropic_frame(
                json!({"type":"content_block_start","index":index,"content_block":initial}),
            ),
        ));
        frames.push((
            0,
            anthropic_frame(json!({"type":"content_block_delta","index":index,"delta":delta})),
        ));
        if block["type"] == "thinking" {
            frames.push((0,anthropic_frame(json!({"type":"content_block_delta","index":index,"delta":{"type":"signature_delta","signature":block["signature"]}}))));
        }
        frames.push((
            0,
            anthropic_frame(json!({"type":"content_block_stop","index":index})),
        ));
    }
    frames.push((0,anthropic_frame(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8}}))));
    frames.push((0, anthropic_frame(json!({"type":"message_stop"}))));
    let server = Server::new(vec![
        Reply::Frames(frames),
        Reply::Json(anthropic(
            json!([{"type":"text","text":"done"}]),
            "end_turn",
        )),
    ]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "anthropic".into();
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        let mut req = request("anthropic-stream-signed");
        req.tools = vec![tool()];
        req.options.response_format = Some(schema());
        let g = generation(&host, req.clone()).await;
        g.start().await.unwrap();
        let s = settled(&g).await;
        assert!(s.error.is_none(), "{:?}", s.error);
        let r = s.response.unwrap();
        assert_eq!(r.finish_reason, FinishReason::ToolCalls);
        req.options.response_format = None;
        req.request_id = "anthropic-stream-replay".into();
        req.messages.push(serialized_message(r.message, true));
        req.messages.push(tool_result("call-a"));
        req.messages.push(tool_result("call-b"));
        response(p.generate(req).await.unwrap());
        let expected = ant_content().as_array().unwrap()[..6].to_vec();
        assert_eq!(
            server.requests.lock().unwrap()[1]["messages"][1]["content"],
            json!(expected)
        );
        host.close().await.unwrap();
    });
}

#[test]
fn gemini_stream_fails_closed_on_frames_that_genai_reorders_or_overwrites() {
    let server = Server::new(vec![Reply::Frames(vec![(
        0,
        frame(gemini(json!([
            {"text":"before"}, {"thought":true,"text":"later reasoning","thoughtSignature":"opaque"}
        ]))),
    )])]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "gemini".into();
        let host = host(&config, driver).await;
        let g = generation(&host, request("gemini-lossy-frame")).await;
        g.start().await.unwrap();
        let state = settled(&g).await;
        assert_eq!(state.error.unwrap().kind, ProviderErrorKind::DataLoss);
        assert!(state.response.is_none());
        assert!(state.events.is_empty());
        host.close().await.unwrap();
    });
}

fn anthropic_frame(value: Value) -> String {
    format!(
        "data: {value}\nevent: {}\n\n",
        value["type"].as_str().unwrap()
    )
}

#[test]
fn responses_nullable_and_encrypted_only_reasoning_items_replay_without_visible_duplicates() {
    let output = json!([
        {"id":"reason-null","type":"reasoning","summary":[],"encrypted_content":null},
        {"id":"reason-encrypted","type":"reasoning","summary":[],"encrypted_content":"opaque-encrypted-only"},
        {"id":"reason-absent","type":"reasoning","summary":[]},
        {"id":"fc","type":"function_call","call_id":"call-a","name":"lookup","arguments":"{\"key\":7}"}
    ]);
    let server = Server::new(vec![
        Reply::Json(responses(output.clone())),
        Reply::Json(responses(
            json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}]),
        )),
    ]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "openai_resp".into();
        let host = host(&config, driver).await;
        let p = provider(&host).await;
        let mut req = request("nullable-reasoning");
        req.tools = vec![tool()];
        let r = response(p.generate(req.clone()).await.unwrap());
        assert!(
            r.message
                .content
                .iter()
                .all(|p| !matches!(p, ContentPart::Reasoning { .. }))
        );
        req.request_id = "nullable-replay".into();
        req.messages.push(serialized_message(r.message, true));
        req.messages.push(tool_result("call-a"));
        response(p.generate(req).await.unwrap());
        {
            let seen = server.requests.lock().unwrap();
            for i in 0..3 {
                assert_eq!(seen[1]["input"][i + 1], output[i]);
            }
        }
        host.close().await.unwrap();
    });
}

#[test]
fn malformed_or_ambiguous_continuations_fail_closed_without_http() {
    let server = Server::new(vec![]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let p = provider(&host).await;
        let mut req = request("old-ambiguous-replay");
        req.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentPart::Extension {
                extension: Extension {
                    namespace: "swarmie.continuation.openai".into(),
                    version: 1,
                    value: json!({"kind":"reasoning_replay","value":["unknown original position"]}),
                },
            }],
        });
        assert!(
            matches!(p.generate(req.clone()).await.unwrap(),GenerateResult::Error{error} if error.kind==ProviderErrorKind::Unsupported && error.submission==Submission::NotSent)
        );
        req.request_id = "interleaved-plain-reasoning".into();
        req.messages.last_mut().unwrap().content = vec![
            ContentPart::Text {
                text: "before".into(),
            },
            ContentPart::Reasoning {
                text: "later".into(),
            },
        ];
        assert!(
            matches!(p.generate(req).await.unwrap(),GenerateResult::Error{error} if error.kind==ProviderErrorKind::Unsupported && error.submission==Submission::NotSent)
        );
        assert!(server.requests.lock().unwrap().is_empty());
        host.close().await.unwrap();
    });
}

#[test]
fn openai_stream_refuses_simultaneous_channels_that_upstream_would_drop() {
    let server = Server::new(vec![Reply::Frames(vec![
        (
            0,
            frame(
                json!({"choices":[{"delta":{"content":"visible","reasoning_content":"must not disappear"}}]}),
            ),
        ),
        (
            0,
            frame(json!({"choices":[{"delta":{},"finish_reason":"stop"}]})),
        ),
        (0, done()),
    ])]);
    run(|driver| async {
        let host = host(&configuration(&server.url), driver).await;
        let g = generation(&host, request("simultaneous-channels")).await;
        g.start().await.unwrap();
        let state = settled(&g).await;
        assert_eq!(state.error.unwrap().kind, ProviderErrorKind::DataLoss);
        assert!(state.events.is_empty());
        assert!(state.response.is_none());
        host.close().await.unwrap();
    });
}

#[test]
fn request_preflight_refuses_text_hoisting_and_synthetic_gemini_signatures() {
    let server = Server::new(vec![]);
    for provider_name in ["openai", "gemini"] {
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = provider_name.into();
            config.connections[0].models.push("gemini-3-fixture".into());
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let mut req = request("unrepresentable-continuation");
            if provider_name == "gemini" {
                req.model.model = "gemini-3-fixture".into();
            }
            req.tools = vec![tool()];
            let mut content = vec![ContentPart::ToolCall {
                call: ToolCall {
                    id: "call-a".into(),
                    name: "lookup".into(),
                    arguments: json!({"key":7}),
                    extensions: vec![],
                },
            }];
            if provider_name == "openai" {
                content.push(ContentPart::Text {
                    text: "after the call".into(),
                });
            }
            req.messages.push(Message {
                role: Role::Assistant,
                content,
            });
            req.messages.push(tool_result("call-a"));
            assert!(
                matches!(p.generate(req).await.unwrap(), GenerateResult::Error { error } if error.kind == ProviderErrorKind::Unsupported && error.submission == Submission::NotSent)
            );
            assert!(server.requests.lock().unwrap().is_empty());
            host.close().await.unwrap();
        });
    }
}
