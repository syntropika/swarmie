use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use swarmie_ai::{
    AuthProfile, ChatProfile, Effect, Error, Failure, Model, Protocol, Timeouts, WireMode,
};
use swarmie_model_catalog::{
    Capability, Catalog, Evidence, Layer, LimitClaim, Limits, ModelIdentity, Precision,
    Preparation, Requirements, Snapshot, SourceRecord,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use zeroize::Zeroizing;

fn protocols() -> [Protocol; 6] {
    [
        Protocol::ChatCompletions(ChatProfile::OpenAi),
        Protocol::ChatCompletions(ChatProfile::DeepSeek),
        Protocol::ChatCompletions(ChatProfile::Kimi),
        Protocol::Anthropic,
        Protocol::Gemini,
        Protocol::Ollama,
    ]
}

fn snapshot(protocol: Protocol, endpoint: &str) -> Arc<Snapshot> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let identity = ModelIdentity {
        provider: "fixture".into(),
        adapter: protocol.adapter().into(),
        endpoint: endpoint.into(),
        deployment: None,
        model: "test-model".into(),
        revision: None,
    };
    let evidence = Evidence {
        source: "synthetic-protocol-case".into(),
        revision: "1".into(),
        layer: Layer::Provider,
        observed_at_ms: now,
        expires_at_ms: None,
    };
    let claim = |tokens: u64| LimitClaim {
        tokens: tokens.try_into().unwrap(),
        precision: Precision::Reported,
        evidence: evidence.clone(),
    };
    let qualification = Evidence {
        layer: Layer::Adapter,
        ..evidence.clone()
    };
    let mut catalog = Catalog::new(1);
    catalog
        .replace_sources(vec![SourceRecord {
            identity: identity.clone(),
            limits: Limits {
                context: vec![claim(10000)],
                input: vec![claim(9000)],
                output: vec![claim(1000)],
            },
            tools: Capability::Supported {
                evidence: qualification.clone(),
                value: (),
            },
            streaming: Capability::Supported {
                evidence: qualification,
                value: (),
            },
            native_compaction: Capability::default(),
            native_output_maximum: Capability::default(),
        }])
        .unwrap();
    catalog
        .prepare(
            Preparation {
                identity,
                configuration_revision: "fixture-configuration".into(),
                requirements: Requirements::default(),
                allow_stale_for_ms: 0,
            },
            now,
        )
        .unwrap()
}

fn model(protocol: Protocol, endpoint: &str) -> Model {
    let anonymous = protocol == Protocol::Ollama;
    Model::new(
        snapshot(protocol, endpoint),
        protocol,
        if anonymous {
            AuthProfile::Anonymous
        } else {
            AuthProfile::ApiKey
        },
        if anonymous {
            None
        } else {
            Some(Zeroizing::new("explicit-secret".into()))
        },
        None,
        Timeouts::default(),
    )
    .unwrap()
}

fn endpoint(protocol: Protocol, origin: &str) -> String {
    format!(
        "{origin}{}",
        match protocol {
            Protocol::ChatCompletions(_) => "/v1/chat/completions",
            Protocol::Anthropic => "/v1/messages",
            Protocol::Gemini => "/v1beta/models/test-model",
            Protocol::Ollama => "/api/chat",
            Protocol::Responses => "/v1/responses",
        }
    )
}

fn request(protocol: Protocol) -> Value {
    match protocol {
        Protocol::ChatCompletions(profile) => {
            let mut value = json!({"model":"test-model", "messages":[
                {"role":"system","content":"Keep the original history."},
                {"role":"user","content":"Continue."},
                {"role":"assistant","content":null,"reasoning_content":"retained reasoning", "vendor_signature":"opaque-signature",
                 "tool_calls":[{"type":"function","id":"call-1","function":{"name":"lookup","arguments":"{ \"key\": 1 }", "namespace":"fixture"}}]},
                {"role":"tool","tool_call_id":"call-1","content":"result"}],
                "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]});
            value[if profile == ChatProfile::OpenAi {
                "max_completion_tokens"
            } else {
                "max_tokens"
            }] = 100.into();
            if profile != ChatProfile::OpenAi {
                value["thinking"] = json!({"type":"enabled"});
                value["reasoning_effort"] = "max".into();
            }
            value
        }
        Protocol::Anthropic => {
            json!({"model":"test-model","max_tokens":100,"system":"Instructions.","messages":[
            {"role":"user","content":"Look up the key."},
            {"role":"assistant","content":[{"type":"thinking","thinking":"retained reasoning","signature":"opaque-signature"},
                {"type":"redacted_thinking","data":"opaque-redacted"},
                {"type":"tool_use","id":"call-1","name":"lookup","input":{"key":1}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","content":"result"}]}],
            "tools":[{"name":"lookup","input_schema":{"type":"object"}}]})
        }
        Protocol::Gemini => {
            json!({"contents":[{"role":"user","parts":[{"text":"Look up the key."}]},
            {"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"key":1}},"thoughtSignature":"opaque-signature"}]},
            {"role":"user","parts":[{"functionResponse":{"name":"lookup","response":{"value":"result"}}}]}],
            "tools":[{"functionDeclarations":[{"name":"lookup","parameters":{"type":"object"}}]}],
            "generationConfig":{"maxOutputTokens":100,"thinkingConfig":{"includeThoughts":true}}})
        }
        Protocol::Ollama => {
            json!({"model":"test-model","messages":[{"role":"user","content":"Look up the key."},
            {"role":"assistant","content":"","thinking":"retained reasoning","tool_calls":[{"function":{"name":"lookup","arguments":{"key":1}}}]},
            {"role":"tool","tool_name":"lookup","content":"result"}], "think":true,
            "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}],
            "options":{"num_predict":100,"num_ctx":10000}})
        }
        Protocol::Responses => unreachable!(),
    }
}

fn response(protocol: Protocol) -> Value {
    let mut value = match protocol {
        Protocol::ChatCompletions(_) => {
            json!({"id":"completion-1","model":"test-model","object":"chat.completion",
            "choices":[{"index":0,"message":{"role":"assistant","content":"answer","reasoning_content":"reasoning",
                "tool_calls":[{"type":"function","id":"call-2","function":{"name":"lookup","arguments":"{ \"key\": 2 }"}}]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":10,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":3}}})
        }
        Protocol::Anthropic => {
            json!({"id":"message-1","model":"test-model","type":"message","role":"assistant","content":[
            {"type":"thinking","thinking":"reasoning","signature":"opaque-signature"},
            {"type":"tool_use","id":"call-2","name":"lookup","input":{"key":2}}], "stop_reason":"tool_use",
            "usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":3}})
        }
        Protocol::Gemini => {
            json!({"responseId":"response-1","modelVersion":"test-model","candidates":[{"index":0,"content":{"role":"model","parts":[
            {"functionCall":{"name":"lookup","args":{"key":2}},"thoughtSignature":"opaque-signature"}]}, "finishReason":"STOP"}],
            "usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":20,"thoughtsTokenCount":3}})
        }
        Protocol::Ollama => {
            json!({"model":"test-model","message":{"role":"assistant","content":"answer","thinking":"reasoning",
            "tool_calls":[{"function":{"name":"lookup","arguments":{"key":2}}}]},"done":true,"done_reason":"stop",
            "prompt_eval_count":10,"eval_count":20})
        }
        Protocol::Responses => unreachable!(),
    };
    value["future_extension"] = json!({"opaque":"preserve"});
    value
}

fn events(protocol: Protocol) -> Vec<Value> {
    match protocol {
        Protocol::ChatCompletions(_) => vec![
            json!({"id":"completion-1","model":"test-model","object":"chat.completion.chunk","choices":[{"index":0,
                "delta":{"role":"assistant","reasoning_content":"opaque reasoning","tool_calls":[{"index":0,"id":"call-2","type":"function","function":{"name":"lookup","arguments":"{ \"key\": "}}]},"finish_reason":null}]}),
            json!({"id":"completion-1","model":"test-model","object":"chat.completion.chunk","choices":[{"index":0,
                "delta":{"tool_calls":[{"index":0,"function":{"arguments":"2 }"}}],"vendor_signature":"opaque"},"finish_reason":"tool_calls"}]}),
            json!({"id":"completion-1","model":"test-model","object":"chat.completion.chunk","choices":[],
                "usage":{"prompt_tokens":10,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":3}}}),
        ],
        Protocol::Anthropic => vec![
            json!({"type":"message_start","message":{"id":"message-1","type":"message","role":"assistant","model":"test-model","content":[],"usage":{"input_tokens":10,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"reasoning"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque-signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call-2","name":"lookup","input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{ \"key\": "}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"2 }"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"future_event","opaque":{"data":"keep"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":20}}),
            json!({"type":"message_stop"}),
        ],
        Protocol::Gemini => vec![
            json!({"responseId":"response-1","modelVersion":"test-model","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"reasoning","thought":true}]}}]}),
            json!({"responseId":"response-1","modelVersion":"test-model","candidates":[{"index":0,"content":{"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"key":2}},"thoughtSignature":"opaque-signature"}]}}]}),
            json!({"responseId":"response-1","modelVersion":"test-model","candidates":[{"index":0,"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":20,"thoughtsTokenCount":3}}),
        ],
        Protocol::Ollama => vec![
            json!({"model":"test-model","message":{"role":"assistant","content":"","thinking":"reasoning"},"done":false}),
            json!({"model":"test-model","message":{"role":"assistant","content":"answer","tool_calls":[{"function":{"name":"lookup","arguments":{"key":2}}}]},"done":false}),
            json!({"model":"test-model","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":10,"eval_count":20}),
        ],
        Protocol::Responses => unreachable!(),
    }
}

fn stream_body(protocol: Protocol, events: &[Value], terminal: bool) -> String {
    let mut body = String::new();
    for event in events {
        match protocol {
            Protocol::Ollama => body.push_str(&format!("{event}\n")),
            Protocol::Anthropic => body.push_str(&format!(
                "event: {}\r\ndata: {event}\r\n\r\n",
                event["type"].as_str().unwrap()
            )),
            _ => body.push_str(&format!("data: {event}\n\n")),
        }
    }
    if terminal && matches!(protocol, Protocol::ChatCompletions(_)) {
        body.push_str("data: [DONE]\n\n");
    }
    body
}

async fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    let mut data = Vec::new();
    let mut buffer = [0; 2048];
    loop {
        let n = socket.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        data.extend_from_slice(&buffer[..n]);
        if let Some(end) = data.windows(4).position(|v| v == b"\r\n\r\n") {
            let headers = String::from_utf8(data[..end].to_vec()).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            if data.len() >= end + 4 + length {
                return (headers, data[end + 4..].to_vec());
            }
        }
    }
}

async fn fixture(
    protocol: Protocol,
    body: String,
    streaming: bool,
) -> (String, tokio::task::JoinHandle<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = endpoint(
        protocol,
        &format!("http://{}", listener.local_addr().unwrap()),
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        let content_type = if streaming {
            if protocol == Protocol::Ollama {
                "application/x-ndjson"
            } else {
                "text/event-stream"
            }
        } else {
            "application/json"
        };
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
        // Deliberately fragment the provider body, including JSON and SSE delimiters.
        for part in body.as_bytes().chunks(7) {
            // Refusal can close transport before the fixture finishes writing.
            if let Err(error) = socket.write_all(part).await {
                assert!(matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ));
                break;
            }
            tokio::task::yield_now().await;
        }
        request
    });
    (url, task)
}

#[tokio::test]
async fn all_complete_profiles_preserve_admitted_bytes_results_and_auth_destinations() {
    for protocol in protocols() {
        let expected = response(protocol);
        let (url, server) = fixture(protocol, expected.to_string(), false).await;
        let model = model(protocol, &url);
        let input = request(protocol);
        let prepared = model
            .prepare(uuid::Uuid::new_v4(), input.clone(), WireMode::Complete)
            .unwrap();
        assert_eq!(prepared.description().protocol, protocol);
        let destination = prepared.description().destination.clone();
        let mut admitted_bytes = Vec::new();
        let admitted = prepared
            .admit(|_, bytes, _, _| {
                admitted_bytes = bytes.to_vec();
                Ok(())
            })
            .unwrap();
        let output = admitted
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert_eq!(output.response(), Some(&expected));
        assert_eq!(output.protocol(), protocol);
        assert!(output.events().is_empty());
        let (headers, bytes) = server.await.unwrap();
        assert_eq!(bytes, admitted_bytes);
        let actual: Value = serde_json::from_slice(&bytes).unwrap();
        let mut expected_input = input;
        if protocol != Protocol::Gemini {
            expected_input["stream"] = false.into();
        }
        assert_eq!(actual, expected_input);
        if protocol == Protocol::Gemini {
            assert!(headers.starts_with("POST /v1beta/models/test-model:generateContent HTTP/1.1"));
            assert!(destination.ends_with(":generateContent"));
        } else {
            assert_eq!(destination, url);
        }
        let lower = headers.to_ascii_lowercase();
        match protocol {
            Protocol::Anthropic => {
                assert!(lower.contains("x-api-key: explicit-secret"));
                assert!(lower.contains("anthropic-version: 2023-06-01"));
                assert!(!lower.contains("authorization:"));
            }
            Protocol::Gemini => {
                assert!(lower.contains("x-goog-api-key: explicit-secret"));
                assert!(!lower.contains("authorization:"));
                assert!(!destination.contains("explicit-secret"));
            }
            Protocol::Ollama => {
                assert!(!lower.contains("authorization:"));
                assert!(!lower.contains("explicit-secret"));
            }
            _ => assert!(lower.contains("authorization: bearer explicit-secret")),
        }
    }
}

#[tokio::test]
async fn all_stream_profiles_retain_every_opaque_signature_argument_and_usage_frame() {
    for protocol in protocols() {
        let expected = events(protocol);
        let body = stream_body(protocol, &expected, true);
        let (url, server) = fixture(protocol, body, true).await;
        let prepared = model(protocol, &url)
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap();
        if protocol == Protocol::Gemini {
            assert!(
                prepared
                    .description()
                    .destination
                    .ends_with(":streamGenerateContent?alt=sse")
            );
        }
        let output = prepared
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert_eq!(output.events(), expected, "{protocol:?}");
        let document = output.response().expect("confirmed stream has a document");
        match protocol {
            Protocol::ChatCompletions(_) => {
                let message = &document["choices"][0]["message"];
                assert_eq!(document["object"], "chat.completion");
                assert_eq!(message["reasoning_content"], "opaque reasoning");
                assert_eq!(message["vendor_signature"], "opaque");
                assert_eq!(
                    message["tool_calls"][0]["function"]["arguments"],
                    "{ \"key\": 2 }"
                );
                assert!(message["tool_calls"][0].get("index").is_none());
                assert_eq!(
                    document["usage"]["prompt_tokens_details"]["cached_tokens"],
                    3
                );
            }
            Protocol::Anthropic => {
                assert_eq!(
                    document["content"][0],
                    json!({"type":"thinking","thinking":"reasoning","signature":"opaque-signature"})
                );
                assert_eq!(document["content"][1]["input"], json!({"key":2}));
                assert_eq!(
                    document["usage"],
                    json!({"input_tokens":10,"output_tokens":20})
                );
            }
            Protocol::Gemini => {
                let parts = &document["candidates"][0]["content"]["parts"];
                assert_eq!(parts[0], json!({"text":"reasoning","thought":true}));
                assert_eq!(parts[1]["thoughtSignature"], "opaque-signature");
                assert_eq!(parts[1]["functionCall"]["args"], json!({"key":2}));
                assert_eq!(document["usageMetadata"]["thoughtsTokenCount"], 3);
            }
            Protocol::Ollama => {
                assert_eq!(document["message"]["content"], "answer");
                assert_eq!(document["message"]["thinking"], "reasoning");
                assert_eq!(
                    document["message"]["tool_calls"][0]["function"]["arguments"],
                    json!({"key":2})
                );
                assert_eq!(document["eval_count"], 20);
            }
            Protocol::Responses => unreachable!(),
        }
        let (headers, bytes) = server.await.unwrap();
        if protocol == Protocol::Gemini {
            assert!(headers.starts_with(
                "POST /v1beta/models/test-model:streamGenerateContent?alt=sse HTTP/1.1"
            ));
        }
        let input: Value = serde_json::from_slice(&bytes).unwrap();
        if protocol != Protocol::Gemini {
            assert_eq!(input["stream"], true);
        }
        if matches!(protocol, Protocol::ChatCompletions(_)) {
            assert_eq!(input["stream_options"]["include_usage"], true);
        }
    }
}

#[tokio::test]
async fn all_profiles_refuse_premature_stream_termination() {
    for protocol in protocols() {
        let mut frames = events(protocol);
        if !matches!(protocol, Protocol::ChatCompletions(_)) {
            frames.pop();
        }
        let (url, server) = fixture(protocol, stream_body(protocol, &frames, false), true).await;
        let error = model(protocol, &url)
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .err()
            .unwrap();
        assert_eq!(
            error,
            Error {
                failure: Failure::Protocol,
                effect: Effect::Unknown
            },
            "{protocol:?}"
        );
        server.await.unwrap();
    }
}

#[test]
fn every_profile_requires_an_explicit_enforceable_cap_and_exact_metadata_binding() {
    for protocol in protocols() {
        let url = endpoint(protocol, "https://provider.example");
        let model = model(protocol, &url);
        let mut no_cap = request(protocol);
        match protocol {
            Protocol::ChatCompletions(_) => {
                no_cap.as_object_mut().unwrap().remove("max_tokens");
                no_cap
                    .as_object_mut()
                    .unwrap()
                    .remove("max_completion_tokens");
            }
            Protocol::Anthropic => {
                no_cap.as_object_mut().unwrap().remove("max_tokens");
            }
            Protocol::Gemini => {
                no_cap["generationConfig"]
                    .as_object_mut()
                    .unwrap()
                    .remove("maxOutputTokens");
            }
            Protocol::Ollama => {
                no_cap["options"]
                    .as_object_mut()
                    .unwrap()
                    .remove("num_predict");
            }
            _ => unreachable!(),
        }
        assert_eq!(
            model
                .prepare(uuid::Uuid::new_v4(), no_cap, WireMode::Complete)
                .err()
                .unwrap()
                .effect,
            Effect::NotDispatched
        );
        let other = if protocol == Protocol::Responses {
            Protocol::Anthropic
        } else {
            Protocol::Responses
        };
        assert_eq!(
            Model::new(
                snapshot(protocol, &url),
                other,
                AuthProfile::ApiKey,
                Some(Zeroizing::new("key".into())),
                None,
                Timeouts::default()
            )
            .err()
            .unwrap()
            .failure,
            Failure::Metadata
        );
    }
}

#[test]
fn anonymous_access_and_gemini_model_resources_are_explicit() {
    for protocol in protocols() {
        if protocol == Protocol::Ollama {
            continue;
        }
        let url = endpoint(protocol, "https://provider.example");
        assert_eq!(
            Model::new(
                snapshot(protocol, &url),
                protocol,
                AuthProfile::Anonymous,
                None,
                None,
                Timeouts::default()
            )
            .err()
            .unwrap()
            .failure,
            Failure::UnsupportedProfile
        );
    }
    let url = "https://provider.example/v1beta/models/foreign-model";
    assert_eq!(
        Model::new(
            snapshot(Protocol::Gemini, url),
            Protocol::Gemini,
            AuthProfile::ApiKey,
            Some(Zeroizing::new("key".into())),
            None,
            Timeouts::default()
        )
        .err()
        .unwrap()
        .failure,
        Failure::Configuration
    );
}

#[tokio::test]
async fn inconsistent_terminals_and_missing_usage_do_not_grant_replay() {
    for protocol in protocols() {
        let mut frames = events(protocol);
        match protocol {
            Protocol::ChatCompletions(_) => {
                frames
                    .last_mut()
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove("usage");
            }
            Protocol::Anthropic => {
                frames.retain(|v| v["type"] != "content_block_stop");
            }
            Protocol::Gemini => {
                frames.last_mut().unwrap()["candidates"][0]["finishReason"] =
                    "FINISH_REASON_UNSPECIFIED".into();
            }
            Protocol::Ollama => {
                frames
                    .last_mut()
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove("eval_count");
            }
            _ => unreachable!(),
        }
        let (url, server) = fixture(protocol, stream_body(protocol, &frames, true), true).await;
        let error = model(protocol, &url)
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .err()
            .unwrap();
        assert_eq!(error.effect, Effect::Unknown);
        server.await.unwrap();
    }
}

#[test]
fn unsupported_remote_state_and_multiple_candidates_refuse_without_mutation() {
    let cases = [
        (Protocol::Gemini, json!({"cachedContent":"hidden"})),
        (
            Protocol::Gemini,
            json!({"generationConfig":{"maxOutputTokens":100,"candidateCount":2}}),
        ),
        (Protocol::Anthropic, json!({"context_management":{}})),
        (
            Protocol::ChatCompletions(ChatProfile::Kimi),
            json!({"max_completion_tokens":100}),
        ),
        (Protocol::Ollama, json!({"options":{"num_predict":-1}})),
    ];
    for (protocol, changes) in cases {
        let model = model(protocol, &endpoint(protocol, "https://provider.example"));
        let mut input = request(protocol);
        input
            .as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        assert!(
            model
                .prepare(uuid::Uuid::new_v4(), input, WireMode::Complete)
                .is_err()
        );
    }
}

#[tokio::test]
async fn a_confirmed_terminal_is_not_overwritten_by_trailing_framing_errors() {
    for protocol in protocols().into_iter().filter(|p| *p != Protocol::Gemini) {
        let mut body = stream_body(protocol, &events(protocol), true);
        body.push_str(if protocol == Protocol::Ollama {
            "not-json\n"
        } else {
            "data: not-json\n\n"
        });
        let (url, server) = fixture(protocol, body, true).await;
        let result = model(protocol, &url)
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await;
        assert!(result.is_ok(), "{protocol:?}");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn gemini_accepts_a_separate_usage_only_final_chunk() {
    let protocol = Protocol::Gemini;
    let mut frames = events(protocol);
    let usage = frames
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("usageMetadata")
        .unwrap();
    frames
        .push(json!({"responseId":"response-1","modelVersion":"test-model","usageMetadata":usage}));
    let (url, server) = fixture(protocol, stream_body(protocol, &frames, true), true).await;
    let output = model(protocol, &url)
        .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
        .unwrap()
        .admit(|_, _, _, _| Ok(()))
        .unwrap()
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(output.events(), frames);
    server.await.unwrap();
}

#[test]
fn malformed_native_content_and_context_overrides_refuse_before_dispatch() {
    let cases = [
        (
            Protocol::Anthropic,
            json!({"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"reasoning"}]}]}),
        ),
        (
            Protocol::Anthropic,
            json!({"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"c","name":"lookup","input":"not-an-object"}]}]}),
        ),
        (
            Protocol::Gemini,
            json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":"lookup","args":"partial"},"thoughtSignature":"opaque"}]}]}),
        ),
        (
            Protocol::Gemini,
            json!({"generationConfig":{"maxOutputTokens":100,"responseModalities":["IMAGE"]}}),
        ),
        (
            Protocol::Ollama,
            json!({"options":{"num_predict":100,"num_ctx":9000}}),
        ),
        (Protocol::Ollama, json!({"think":42})),
    ];
    for (protocol, changes) in cases {
        let model = model(protocol, &endpoint(protocol, "https://provider.example"));
        let mut input = request(protocol);
        input
            .as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        assert_eq!(
            model
                .prepare(uuid::Uuid::new_v4(), input, WireMode::Complete)
                .err()
                .unwrap()
                .effect,
            Effect::NotDispatched
        );
    }
}

#[tokio::test]
async fn provider_stream_errors_have_a_distinct_safe_failure_category() {
    for protocol in protocols() {
        let error = json!({"type":"error", "error":{"type":"overloaded_error", "message":"private provider detail"}});
        let (url, server) = fixture(protocol, stream_body(protocol, &[error], false), true).await;
        let error = model(protocol, &url)
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .err()
            .unwrap();
        assert_eq!(
            error,
            Error {
                failure: Failure::Provider,
                effect: Effect::Unknown
            }
        );
        assert!(!format!("{error:?}").contains("private provider detail"));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn sse_profiles_accept_bom_and_bare_cr_without_changing_transcripts() {
    for protocol in protocols().into_iter().filter(|p| *p != Protocol::Ollama) {
        let expected = events(protocol);
        let body = stream_body(protocol, &expected, true)
            .replace("\r\n", "\n")
            .replace('\n', "\r");
        let (url, server) = fixture(protocol, format!("\u{feff}{body}"), true).await;
        let output = model(protocol, &url)
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert_eq!(output.events(), expected);
        assert!(output.response().is_some());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn reconstructed_assistant_messages_can_be_prepared_without_losing_opaque_content() {
    for protocol in protocols() {
        let (url, server) = fixture(
            protocol,
            stream_body(protocol, &events(protocol), true),
            true,
        )
        .await;
        let model = model(protocol, &url);
        let output = model
            .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .unwrap()
            .wait()
            .await
            .unwrap();
        server.await.unwrap();
        let document = output.response().unwrap();
        let assistant = match protocol {
            Protocol::ChatCompletions(_) => document["choices"][0]["message"].clone(),
            Protocol::Anthropic => json!({"role":"assistant","content":document["content"]}),
            Protocol::Gemini => document["candidates"][0]["content"].clone(),
            Protocol::Ollama => document["message"].clone(),
            Protocol::Responses => unreachable!(),
        };
        let field = if protocol == Protocol::Gemini {
            "contents"
        } else {
            "messages"
        };
        let mut next = request(protocol);
        next[field].as_array_mut().unwrap().push(assistant.clone());
        model
            .prepare(uuid::Uuid::new_v4(), next, WireMode::Complete)
            .unwrap()
            .admit(|_, bytes, _, _| {
                let sent: Value = serde_json::from_slice(bytes).unwrap();
                assert_eq!(sent[field].as_array().unwrap().last(), Some(&assistant));
                Ok(())
            })
            .unwrap();
    }
}

#[tokio::test]
async fn malformed_anthropic_arguments_refuse_even_after_transport_terminal() {
    let protocol = Protocol::Anthropic;
    let mut frames = events(protocol);
    for frame in &mut frames {
        if frame["delta"]["type"] == "input_json_delta" {
            frame["delta"]["partial_json"] = "not-json".into();
        }
    }
    let (url, server) = fixture(protocol, stream_body(protocol, &frames, true), true).await;
    let error = model(protocol, &url)
        .prepare(uuid::Uuid::new_v4(), request(protocol), WireMode::Stream)
        .unwrap()
        .admit(|_, _, _, _| Ok(()))
        .unwrap()
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .err()
        .unwrap();
    assert_eq!(
        error,
        Error {
            failure: Failure::Protocol,
            effect: Effect::Unknown
        }
    );
    server.await.unwrap();
}

#[tokio::test]
async fn typed_codec_profiles_freeze_once_and_decode_confirmed_documents_with_reported_usage() {
    for protocol in protocols() {
        for mode in [WireMode::Complete, WireMode::Stream] {
            let streaming = mode == WireMode::Stream;
            let expected = if streaming {
                None
            } else {
                Some(if protocol == Protocol::Gemini {
                    serde_json::from_str(
                        &response(protocol)
                            .to_string()
                            .replace("opaque-signature", "b3BhcXVlLXNpZ25hdHVyZQ=="),
                    )
                    .unwrap()
                } else {
                    response(protocol)
                })
            };
            let body = if streaming {
                let body = stream_body(protocol, &events(protocol), true);
                if protocol == Protocol::Gemini {
                    body.replace("opaque-signature", "b3BhcXVlLXNpZ25hdHVyZQ==")
                } else {
                    body
                }
            } else {
                expected.as_ref().unwrap().to_string()
            };
            let (url, server) = fixture(protocol, body, streaming).await;
            let prepared = model(protocol, &url)
                .prepare(
                    uuid::Uuid::new_v4(),
                    swarmie_ai::CompletionRequest::new("Continue.")
                        .max_tokens(100)
                        .tool(swarmie_ai::codecs::completion::ToolDefinition::new(
                            swarmie_ai::codecs::message::ToolName::new("lookup").unwrap(),
                            "Look up a key.",
                            json!({"type":"object","properties":{"key":{"type":"integer"}}}),
                        )),
                    mode,
                )
                .unwrap_or_else(|error| panic!("{protocol:?} {mode:?}: {error}"));
            let mut inspected = Vec::new();
            let admitted = prepared
                .admit(|_, bytes, headers, _| {
                    inspected = bytes.to_vec();
                    assert_eq!(headers["content-type"], "application/json");
                    if protocol != Protocol::Ollama {
                        assert!(
                            headers
                                .values()
                                .any(|value| value == "Bearer explicit-secret"
                                    || value == "explicit-secret")
                        );
                    }
                    Ok(())
                })
                .unwrap();
            let execution = admitted
                .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
                .unwrap();
            // The host owns the terminal result while progress subscribers detach.
            drop(execution.previews());
            let outcome = execution.wait().await.unwrap();
            let completion = outcome.completion().unwrap().unwrap_or_else(|error| {
                panic!(
                    "{protocol:?} {mode:?}: {error:?}; raw={:?}",
                    outcome.response()
                )
            });
            assert!(!completion.choice.is_empty());
            use swarmie_ai::codecs::message::{Message, ToolResultContent};
            let mut next = swarmie_ai::CompletionRequest::new("Continue.")
                .max_tokens(100)
                .tool(swarmie_ai::codecs::completion::ToolDefinition::new(
                    swarmie_ai::codecs::message::ToolName::new("lookup").unwrap(),
                    "Look up a key.",
                    json!({"type":"object","properties":{"key":{"type":"integer"}}}),
                ));
            let results = completion
                .tool_calls()
                .map(|call| call.result(vec![ToolResultContent::text("result")]))
                .collect::<Vec<_>>();
            assert!(!results.is_empty());
            next.chat_history = vec![
                Arc::new(Message::user("Continue.")),
                Arc::new(completion.message().unwrap()),
                Arc::new(Message::tool_results(results)),
            ];
            let continuation = model(protocol, &url)
                .prepare(uuid::Uuid::new_v4(), next, WireMode::Complete)
                .unwrap_or_else(|error| panic!("continuation {protocol:?} {mode:?}: {error}"));
            continuation
                .admit(|_, body, _, _| {
                    let body = String::from_utf8_lossy(body);
                    assert!(body.contains("lookup"));
                    if protocol == Protocol::Anthropic {
                        assert!(body.contains("opaque-signature"));
                    }
                    if protocol == Protocol::Gemini {
                        assert!(body.contains("b3BhcXVlLXNpZ25hdHVyZQ=="));
                    }
                    Ok(())
                })
                .unwrap();
            assert_eq!(
                outcome.usage().unwrap().output_tokens,
                Some(if protocol == Protocol::Gemini { 23 } else { 20 }),
                "{protocol:?} {mode:?}"
            );
            assert!(outcome.usage().unwrap().input_tokens.is_some());
            if let Some(expected) = expected {
                assert_eq!(outcome.response(), Some(&expected));
            }
            let (headers, sent) = server.await.unwrap();
            assert_eq!(sent, inspected);
            if protocol == Protocol::Gemini {
                assert!(!headers.lines().next().unwrap().contains("key="));
            }
        }
    }
}

#[tokio::test]
async fn unknown_typed_finish_retains_raw_usage_and_an_explicit_error_stop_without_replay() {
    let protocol = Protocol::ChatCompletions(ChatProfile::OpenAi);
    let mut expected = response(protocol);
    expected["choices"][0]["finish_reason"] = json!("new_provider_finish");
    let (url, server) = fixture(protocol, expected.to_string(), false).await;
    let outcome = model(protocol, &url)
        .prepare(
            uuid::Uuid::new_v4(),
            swarmie_ai::CompletionRequest::new("Continue.").max_tokens(100),
            WireMode::Complete,
        )
        .unwrap()
        .admit(|_, _, _, _| Ok(()))
        .unwrap()
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(outcome.response(), Some(&expected));
    assert!(matches!(
        outcome.completion().unwrap().unwrap().stop(),
        swarmie_ai::codecs::message::StopReason::Error(_)
    ));
    assert_eq!(outcome.usage().unwrap().output_tokens, Some(20));
    server.await.unwrap();
}

#[tokio::test]
async fn invalid_gemini_signatures_survive_receipt_and_refuse_continuation_before_dispatch() {
    use swarmie_ai::codecs::{
        completion::ToolDefinition,
        message::{Message, ToolName, ToolResultContent},
    };
    let protocol = Protocol::Gemini;
    let (url, server) = fixture(protocol, response(protocol).to_string(), false).await;
    let tool = ToolDefinition::new(
        ToolName::new("lookup").unwrap(),
        "Look up a key.",
        json!({"type":"object"}),
    );
    let model = model(protocol, &url);
    let outcome = model
        .prepare(
            uuid::Uuid::new_v4(),
            swarmie_ai::CompletionRequest::new("Continue.")
                .max_tokens(100)
                .tool(tool.clone()),
            WireMode::Complete,
        )
        .unwrap()
        .admit(|_, _, _, _| Ok(()))
        .unwrap()
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let completion = outcome.completion().unwrap().unwrap();
    assert!(
        serde_json::to_string(&completion.raw)
            .unwrap()
            .contains("opaque-signature")
    );
    let results = completion
        .tool_calls()
        .map(|call| call.result(vec![ToolResultContent::text("result")]))
        .collect::<Vec<_>>();
    let mut next = swarmie_ai::CompletionRequest::new("Continue.")
        .max_tokens(100)
        .tool(tool);
    next.chat_history = vec![
        Arc::new(Message::user("Continue.")),
        Arc::new(completion.message().unwrap()),
        Arc::new(Message::tool_results(results)),
    ];
    let error = model
        .prepare(uuid::Uuid::new_v4(), next, WireMode::Complete)
        .err()
        .unwrap();
    assert_eq!(error.failure, Failure::Request);
    assert_eq!(error.effect, Effect::NotDispatched);
    server.await.unwrap();
}

#[test]
fn typed_ollama_refuses_restrictive_tool_choices_before_admission() {
    use swarmie_ai::codecs::{
        completion::ToolDefinition,
        message::{ToolChoice, ToolName},
    };

    let model = model(Protocol::Ollama, "http://127.0.0.1:9/api/chat");
    let request = || {
        swarmie_ai::CompletionRequest::new("Use the available tools.")
            .max_tokens(100)
            .tool(ToolDefinition::new(
                ToolName::new("lookup").unwrap(),
                "Look up a key.",
                json!({"type":"object"}),
            ))
    };
    for mode in [WireMode::Complete, WireMode::Stream] {
        for choice in [
            ToolChoice::None,
            ToolChoice::Required,
            ToolChoice::Specific {
                function_names: vec![ToolName::new("lookup").unwrap()],
            },
        ] {
            let error = model
                .prepare(uuid::Uuid::new_v4(), request().tool_choice(choice), mode)
                .err()
                .unwrap();
            assert_eq!(error.failure, Failure::Request);
            assert_eq!(error.effect, Effect::NotDispatched);
        }
        let mut automatic = Vec::new();
        model
            .prepare(
                uuid::Uuid::new_v4(),
                request().tool_choice(ToolChoice::Auto),
                mode,
            )
            .unwrap()
            .admit(|_, body, _, _| {
                automatic = body.to_vec();
                assert_eq!(
                    serde_json::from_slice::<Value>(body).unwrap()["tools"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                Ok(())
            })
            .unwrap();
        model
            .prepare(uuid::Uuid::new_v4(), request(), mode)
            .unwrap()
            .admit(|_, body, _, _| {
                assert_eq!(body, automatic);
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn typed_options_refuse_unrepresentable_schema_and_nonfinite_numbers() {
    for profile in [ChatProfile::DeepSeek, ChatProfile::Kimi] {
        let model = model(
            Protocol::ChatCompletions(profile),
            "https://example.test/v1/chat/completions",
        );
        let mut request = swarmie_ai::CompletionRequest::new("Continue.").max_tokens(100);
        request.output_schema = Some(serde_json::from_value(json!({"type":"object"})).unwrap());
        let error = model
            .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
            .err()
            .unwrap();
        assert_eq!(error.failure, Failure::Request);
        let request = swarmie_ai::CompletionRequest::new("Continue.")
            .max_tokens(100)
            .temperature(f64::NAN);
        assert!(
            model
                .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
                .is_err()
        );
    }
}
