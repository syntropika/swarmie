//! Adversarial provider framing must never become a successful partial generation.
#[expect(
    dead_code,
    reason = "shared HTTP fixture also serves other integration suites"
)]
mod support;

use serde_json::{Value, json};
use support::*;
use swarmie_model_contracts::*;

fn terminal(output: Value) -> Value {
    json!({"type":"response.completed","response":{
        "id":"resp-protocol", "model":"fixture-model", "status":"completed", "output":output
    }})
}

fn assert_refused(provider: &str, frames: Vec<String>, kind: ProviderErrorKind) {
    let server = Server::new(vec![Reply::Frames(
        frames.into_iter().map(|frame| (0, frame)).collect(),
    )]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = provider.into();
        let host = host(&config, driver).await;
        let generation = generation(&host, request("adversarial-protocol")).await;
        generation.start().await.unwrap();
        let final_state = settled(&generation).await;
        assert!(final_state.physically_settled);
        assert!(final_state.response.is_none());
        let error = final_state.error.unwrap();
        assert_eq!(error.kind, kind);
        assert_eq!(error.submission, Submission::ResponseReceived);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        host.close().await.unwrap();
    });
}

#[test]
fn malformed_known_responses_events_cannot_hide_behind_valid_completion() {
    for malformed in [
        json!({"type":"response.output_text.delta","delta":null}),
        json!({"type":"response.reasoning_summary_text.delta","delta":42}),
        json!({"type":"response.function_call_arguments.delta","delta":{}}),
        json!({"type":"response.output_text.delta","delta":"","output_index":"wrong"}),
    ] {
        assert_refused(
            "openai_resp",
            vec![frame(malformed), frame(terminal(json!([])))],
            ProviderErrorKind::Parsing,
        );
    }
}

#[test]
fn unsupported_content_events_fail_explicitly_but_protocol_pings_remain_valid() {
    assert_refused(
        "openai_resp",
        vec![
            frame(json!({"type":"response.audio.delta","delta":"opaque-audio"})),
            frame(terminal(json!([]))),
        ],
        ProviderErrorKind::DataLoss,
    );
    let server = Server::new(vec![Reply::Frames(vec![
        (
            0,
            frame(json!({"type":"response.in_progress","response":{"id":"resp-protocol"}})),
        ),
        (0, frame(json!({"type":"ping"}))),
        (0, frame(json!({"type":"rate_limits.updated","limits":[]}))),
        (
            0,
            frame(json!({"type":"response.metrics.updated","metrics":{"queue_ms":0}})),
        ),
        (
            0,
            frame(json!({"type":"response.output_text.delta","delta":"done"})),
        ),
        (
            0,
            frame(terminal(
                json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}]),
            )),
        ),
    ])]);
    run(|driver| async {
        let mut config = configuration(&server.url);
        config.connections[0].provider = "openai_resp".into();
        let host = host(&config, driver).await;
        let generation = generation(&host, request("valid-protocol-metadata")).await;
        generation.start().await.unwrap();
        let state = settled(&generation).await;
        assert!(state.error.is_none(), "{:?}", state.error);
        assert_eq!(
            state.response.unwrap().message,
            Message::text(Role::Assistant, "done")
        );
        host.close().await.unwrap();
    });
}

#[test]
fn responses_plain_eof_is_not_a_successful_terminal_event() {
    assert_refused(
        "openai_resp",
        vec![frame(
            json!({"type":"response.output_text.delta","delta":"partial"}),
        )],
        ProviderErrorKind::Parsing,
    );
}

#[test]
fn responses_terminal_cannot_replace_an_already_completed_opaque_reasoning_item() {
    let reasoning = json!({"type":"reasoning","id":"rs-original","summary":[],"encrypted_content":"original-signature"});
    let mut changed = reasoning.clone();
    changed["encrypted_content"] = "changed-signature".into();
    assert_refused(
        "openai_resp",
        vec![
            frame(json!({"type":"response.output_item.done","output_index":0,"item":reasoning})),
            frame(terminal(json!([changed]))),
        ],
        ProviderErrorKind::DataLoss,
    );
}

#[test]
fn responses_reasoning_without_its_native_item_cannot_be_returned_as_replayable() {
    assert_refused(
        "openai_resp",
        vec![
            frame(
                json!({"type":"response.reasoning_summary_text.delta","delta":"summary without item"}),
            ),
            frame(terminal(json!([]))),
        ],
        ProviderErrorKind::DataLoss,
    );
}

fn anthropic(value: Value) -> String {
    format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap()
    )
}

fn start(index: u64) -> Value {
    json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":format!("call-{index}"),"name":"lookup","input":{}}})
}

#[test]
fn anthropic_rejects_mismatched_or_reopened_blocks_before_tool_snapshots_diverge() {
    for frames in [
        vec![start(0), start(1)],
        vec![
            start(0),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
        ],
        vec![start(0), json!({"type":"content_block_stop","index":1})],
        vec![
            start(0),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
        ],
        vec![
            start(0),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"wrong block kind"}}),
        ],
    ] {
        let mut frames: Vec<_> = frames.into_iter().map(anthropic).collect();
        frames.push(anthropic(json!({"type":"message_stop"})));
        assert_refused("anthropic", frames, ProviderErrorKind::DataLoss);
    }
}

#[test]
fn anthropic_terminal_requires_the_active_signed_block_to_be_closed() {
    assert_refused(
        "anthropic",
        vec![
            anthropic(
                json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"partial","signature":"opaque"}}),
            ),
            anthropic(json!({"type":"message_stop"})),
        ],
        ProviderErrorKind::Parsing,
    );
}
