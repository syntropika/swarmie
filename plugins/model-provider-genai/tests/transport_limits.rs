//! Documents the boundary between parsed-event budgets and upstream framing buffers.
#[allow(dead_code)] // This integration binary exercises only the streaming fixture helpers.
mod support;

use serde_json::json;
use support::*;
use swarmie_model_contracts::{ProviderErrorKind, Submission};

#[test]
fn incomplete_frame_is_stopped_by_deadline_before_parsed_event_budget_can_inspect_it() {
    let server = Server::new(vec![Reply::Frames(vec![
        // More than event_bytes, but deliberately no SSE boundary or complete JSON.
        (
            0,
            format!("data: {{\"padding\":\"{}", "x".repeat(16 * 1024)),
        ),
        (1500, "\"}\n\n".into()),
    ])]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].timeout_ms = 300;
        config.limits.event_bytes = 512;
        let host = host(&config, driver).await;
        let g = generation(&host, request("unfinished-frame")).await;
        g.start().await.unwrap();
        let state = settled(&g).await;
        assert!(state.physically_settled);
        assert!(state.response.is_none());
        assert!(state.events.is_empty());
        let error = state.error.unwrap();
        assert_eq!(error.kind, ProviderErrorKind::Timeout);
        assert_eq!(error.submission, Submission::MayHaveBeenSent);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        host.close().await.unwrap();
    });
}

#[test]
fn complete_oversized_frame_is_refused_after_upstream_framing() {
    let server = Server::new(vec![Reply::Frames(vec![(
        0,
        frame(json!({"choices":[{"delta":{"content":"x".repeat(16 * 1024)}}]})),
    )])]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.limits.event_bytes = 512;
        let host = host(&config, driver).await;
        let g = generation(&host, request("complete-frame")).await;
        g.start().await.unwrap();
        let state = settled(&g).await;
        assert!(state.physically_settled);
        assert!(state.response.is_none());
        assert!(state.events.is_empty());
        let error = state.error.unwrap();
        assert_eq!(error.kind, ProviderErrorKind::PayloadLimit);
        assert_eq!(error.submission, Submission::ResponseReceived);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        host.close().await.unwrap();
    });
}

#[test]
fn malformed_responses_frame_cannot_be_skipped_before_a_valid_completion() {
    for malformed in ["{malformed", "null", "[]"] {
        let server = Server::new(vec![Reply::Frames(vec![
            (0, format!("data: {malformed}\n\n")),
            (
                0,
                frame(json!({"type":"response.output_text.delta","delta":"answer"})),
            ),
            (
                0,
                frame(json!({"type":"response.completed","response":{
                    "id":"resp_1","model":"fixture-model","status":"completed",
                    "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]}],
                    "usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}
                }})),
            ),
        ])]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = "openai_resp".into();
            let host = host(&config, driver).await;
            let g = generation(&host, request("malformed-responses")).await;
            g.start().await.unwrap();
            let state = settled(&g).await;
            assert_eq!(state.error.unwrap().kind, ProviderErrorKind::Parsing);
            assert!(state.events.is_empty());
            assert!(state.response.is_none());
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            host.close().await.unwrap();
        });
    }
}

#[test]
fn final_native_tool_arguments_must_match_the_last_streamed_snapshot() {
    for (partial, final_key, contradicted) in [("{\"key\":7}", 8, true), ("{\"key\":", 7, false)] {
        let call = |key| json!({"type":"function_call","id":"item_1","call_id":"call_original","name":"lookup","arguments":format!("{{\"key\":{key}}}")});
        let server = Server::new(vec![Reply::Frames(vec![
            (
                0,
                frame(
                    json!({"type":"response.output_item.added","output_index":0,"item":{
                        "type":"function_call","id":"item_1","call_id":"call_original","name":"lookup","arguments":""
                    }}),
                ),
            ),
            (
                0,
                frame(
                    json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":partial}),
                ),
            ),
            (
                0,
                frame(json!({"type":"response.output_item.done","output_index":0,"item":call(7)})),
            ),
            (
                0,
                frame(json!({"type":"response.completed","response":{
                    "id":"resp_1","model":"fixture-model","status":"completed","output":[call(final_key)],
                    "usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}
                }})),
            ),
        ])]);
        run(|driver| async {
            let mut config = configuration(&server.url);
            config.connections[0].provider = "openai_resp".into();
            let host = host(&config, driver).await;
            let g = generation(&host, request("incoherent-tool-snapshot")).await;
            g.start().await.unwrap();
            let state = settled(&g).await;
            if contradicted {
                assert_eq!(state.error.unwrap().kind, ProviderErrorKind::DataLoss);
                assert!(state.response.is_none());
            } else {
                assert!(state.error.is_none());
                let response = state.response.unwrap();
                assert!(response.message.content.iter().any(|part| matches!(part,
                swarmie_model_contracts::ContentPart::ToolCall { call }
                    if call.id == "call_original" && call.arguments == json!({"key":7}))));
            }
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            host.close().await.unwrap();
        });
    }
}

#[test]
fn anthropic_sse_event_name_must_match_the_native_content_type() {
    let server = Server::new(vec![Reply::Frames(vec![
        (0, "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"lost\"}}\nevent: ping\n\n".into()),
        (0, "data: {\"type\":\"message_stop\"}\nevent: message_stop\n\n".into()),
    ])]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "anthropic".into();
        let host = host(&config, driver).await;
        let g = generation(&host, request("mismatched-sse-event")).await;
        g.start().await.unwrap();
        let state = settled(&g).await;
        assert_eq!(state.error.unwrap().kind, ProviderErrorKind::DataLoss);
        assert!(state.response.is_none());
        assert!(state.events.is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        host.close().await.unwrap();
    });
}
