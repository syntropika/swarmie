//! Native developer instructions through the public Contract, a real Host and local HTTP.
#[expect(
    dead_code,
    reason = "shared HTTP fixture also serves other integration suites"
)]
mod support;

use serde_json::{Value, json};
use support::*;
use swarmie_model_contracts::*;

fn instructions(id: &str) -> ModelRequest {
    let mut request = request(id);
    request.messages = vec![
        Message::text(Role::System, "system first"),
        Message::text(Role::Developer, "developer first"),
        Message::text(Role::User, "question"),
        Message::text(Role::System, "system later"),
        Message::text(Role::Developer, "developer later"),
        Message::text(Role::User, "answer now"),
    ];
    request
}

fn responses_output() -> Value {
    json!({
        "id":"resp_native_developer", "model":"fixture-model", "status":"completed",
        "output":[{"type":"message", "id":"msg_native_developer", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":"native answer", "annotations":[]}]}],
        "usage":{"input_tokens":6,"output_tokens":2,"total_tokens":8}
    })
}

fn assert_native_order(sent: &Value, field: &str) {
    let messages = sent[field].as_array().unwrap();
    let roles = messages
        .iter()
        .map(|message| message["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        ["system", "developer", "user", "system", "developer", "user"]
    );
    let text = messages
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "system first",
            "developer first",
            "question",
            "system later",
            "developer later",
            "answer now"
        ]
    );
    // Inline instruction messages retain their ordered positions. Responses' top-level
    // instructions field has separate replacement semantics and is deliberately unused.
    assert!(sent.get("instructions").is_none());
}

#[test]
fn generate_preserves_native_system_and_developer_for_both_openai_apis() {
    for kind in ["openai", "openai_resp"] {
        let reply = if kind == "openai" {
            completion("native answer", "stop")
        } else {
            responses_output()
        };
        let server = Server::new(vec![Reply::Json(reply)]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = kind.into();
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let request = instructions("native-generate");
            assert_eq!(
                p.capabilities(request.model.clone())
                    .await
                    .unwrap()
                    .developer_role,
                Support::ModelDependent
            );
            let output = response(p.generate(request).await.unwrap());
            assert_eq!(
                output.message,
                Message::text(Role::Assistant, "native answer")
            );
            assert_native_order(
                &server.requests.lock().unwrap()[0],
                if kind == "openai" {
                    "messages"
                } else {
                    "input"
                },
            );
            host.close().await.unwrap();
        });
    }
}

#[test]
fn streaming_preserves_native_system_and_developer_for_both_openai_apis() {
    for kind in ["openai", "openai_resp"] {
        let frames = if kind == "openai" {
            vec![
                (
                    0,
                    frame(json!({"choices":[{"delta":{"content":"native "}}]})),
                ),
                (
                    15,
                    frame(json!({"choices":[{"delta":{"content":"answer"}}]})),
                ),
                (
                    0,
                    frame(json!({"choices":[{"delta":{},"finish_reason":"stop"}]})),
                ),
                (0, done()),
            ]
        } else {
            vec![
                (
                    0,
                    frame(json!({"type":"response.output_text.delta","delta":"native "})),
                ),
                (
                    15,
                    frame(json!({"type":"response.output_text.delta","delta":"answer"})),
                ),
                (
                    0,
                    frame(json!({"type":"response.completed","response":responses_output()})),
                ),
            ]
        };
        let server = Server::new(vec![Reply::Frames(frames)]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = kind.into();
            let host = host(&config, driver).await;
            let g = generation(&host, instructions("native-stream")).await;
            g.start().await.unwrap();
            let state = settled(&g).await;
            assert!(state.error.is_none(), "{kind}: {:?}", state.error);
            assert_eq!(
                state.response.unwrap().message,
                Message::text(Role::Assistant, "native answer")
            );
            assert_native_order(
                &server.requests.lock().unwrap()[0],
                if kind == "openai" {
                    "messages"
                } else {
                    "input"
                },
            );
            host.close().await.unwrap();
        });
    }
}

#[test]
fn providers_without_native_developer_reject_before_http_in_both_paths() {
    for kind in ["anthropic", "gemini", "ollama", "deepseek", "groq"] {
        let server = Server::new(vec![]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = kind.into();
            let host = host(&config, driver).await;
            let p = provider(&host).await;
            let request = instructions("reject-developer");
            assert_eq!(
                p.capabilities(request.model.clone())
                    .await
                    .unwrap()
                    .developer_role,
                Support::Unsupported
            );
            let GenerateResult::Error { error } = p.generate(request.clone()).await.unwrap() else {
                panic!("{kind} accepted unsupported developer instructions");
            };
            assert_eq!(error.kind, ProviderErrorKind::Unsupported);
            assert_eq!(error.submission, Submission::NotSent);
            let refused = host
                .get(
                    GenerationEndpoint::new(swarmie_model_provider_genai::GENERATION_CONTRACT)
                        .unwrap(),
                    GenerationInput { request },
                )
                .await;
            let error = match refused {
                Ok(_) => panic!("{kind} prepared unsupported developer instructions"),
                Err(error) => error,
            };
            assert_eq!(error.reason, "model_request");
            assert_eq!(
                error.message,
                "generation request refused before submission"
            );
            assert!(server.requests.lock().unwrap().is_empty());
            host.close().await.unwrap();
        });
    }
}
