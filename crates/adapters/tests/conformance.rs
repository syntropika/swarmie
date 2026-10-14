use adapters::*;
use futures_util::StreamExt;
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use swarmie::*;
fn options(provider: &str) -> SpawnOptions {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    SpawnOptions {
        project_id: None,
        project_revision: None,
        execution: Default::default(),
        harness: provider.into(),
        cwd: root.clone(),
        model: None,
        config: json!({"executable":"python3","args":[root.join("tests/fixtures/agent.py"),provider]}),
    }
}
fn runtime() -> Runtime {
    builtin_builder()
        .harness(AcpHarness::new("acp", "python3", vec![]))
        .build()
}
#[tokio::test]
async fn claude_native_tool_error_is_not_a_successful_completion() {
    let runtime = runtime();
    let agent = runtime.spawn(options("claude")).await.unwrap();
    let mut events = agent.events();
    agent.prompt("claude-tool-failed").await.unwrap();
    let events = finish(&mut events).await;
    let completed = events
        .iter()
        .find_map(|event| {
            if let EventKind::ToolCompleted(tool) = &event.kind {
                Some(tool)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(completed.status, "failed");
    assert_eq!(completed.name.as_deref(), Some("Read"));
    assert_eq!(completed.input["file_path"], "hello.txt");
    agent.close().await.unwrap();
}
#[tokio::test]
async fn questions_translate_portable_answers_to_native_responses() {
    for provider in ["codex", "claude", "pi"] {
        let runtime = runtime();
        let agent = runtime.spawn(options(provider)).await.unwrap();
        let mut events = agent.events();
        agent.prompt("question").await.unwrap();
        let request = loop {
            if let EventKind::PermissionRequested(request) = next(&mut events).await.kind {
                break request;
            }
        };
        assert_eq!(request.kind, "question");
        assert!(request.metadata["questions"].is_array());
        assert_eq!(request.questions.len(), 1);
        let id = request.questions[0].id.clone();
        agent
            .respond_permission(PermissionResponse {
                id: request.id,
                option: "answer".into(),
                data: json!(null),
                answers: std::collections::BTreeMap::from([(
                    id,
                    QuestionAnswer::Text("Linux".into()),
                )]),
            })
            .await
            .unwrap();
        let events = finish(&mut events).await;
        let text: String = events
            .iter()
            .filter_map(|e| {
                if let EventKind::TextDelta { text, .. } = &e.kind {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        let native: serde_json::Value = serde_json::from_str(&text).unwrap();
        match provider {
            "codex" => assert_eq!(
                native["result"]["answers"]["target"]["answers"],
                json!(["Linux"])
            ),
            "claude" => assert_eq!(
                native["response"]["response"]["updatedInput"]["answers"]["Which target?"],
                "Linux"
            ),
            "pi" => assert_eq!(native["value"], "Linux"),
            _ => unreachable!(),
        }
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn configuration_is_applied_to_codex_turn_and_invalid_choice_is_rejected() {
    let agent = runtime().spawn(options("codex")).await.unwrap();
    assert!(agent.snapshot().await.capabilities.configure.is_available());
    let configuration = json!(agent.configuration().await.unwrap());
    assert!(
        configuration
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "effort")
    );
    assert!(matches!(
        agent.configure(json!({"effort":"invented"})).await,
        Err(Error::Invalid(_))
    ));
    agent
        .configure(json!({"model":"fixture-model","effort":"high","approvalPolicy":"on-request"}))
        .await
        .unwrap();
    let mut events = agent.events();
    agent.prompt("configuration").await.unwrap();
    let events = finish(&mut events).await;
    let text: String = events
        .iter()
        .filter_map(|e| {
            if let EventKind::TextDelta { text, .. } = &e.kind {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    let request: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(request["model"], "fixture-model");
    assert_eq!(request["effort"], "high");
    assert_eq!(request["approvalPolicy"], "on-request");
    agent.close().await.unwrap();
}
#[tokio::test]
async fn claude_effort_and_fast_controls_use_advertised_native_settings() {
    let mut options = options("claude");
    options.model = Some("fixture-model".into());
    let runtime = runtime();
    let agent = runtime.spawn(options).await.unwrap();
    let descriptors = json!(agent.configuration().await.unwrap());
    assert!(
        descriptors
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "effort")
    );
    assert!(
        descriptors
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "fast" && d["type"] == "boolean")
    );
    assert!(matches!(
        agent.configure(json!({"fast":"yes"})).await,
        Err(Error::Invalid(_))
    ));
    agent.configure(json!({"effort":"high"})).await.unwrap();
    agent.configure(json!({"fast":true})).await.unwrap();
    let id = agent.snapshot().await.id;
    agent.close().await.unwrap();
    let resumed = runtime.resume(&id).await.unwrap();
    let descriptors = json!(resumed.configuration().await.unwrap());
    assert!(
        descriptors
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "effort" && d["value"] == "high")
    );
    assert!(
        descriptors
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "fast" && d["value"] == true)
    );
    resumed.close().await.unwrap();
}
async fn next(events: &mut EventStream) -> RuntimeEvent {
    tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("event deadline")
        .expect("stream ended")
        .expect("event failed")
}
async fn finish(events: &mut EventStream) -> Vec<RuntimeEvent> {
    let mut result = vec![];
    loop {
        let e = next(events).await;
        let end = matches!(e.kind, EventKind::TurnEnded { .. });
        result.push(e);
        if end {
            return result;
        }
    }
}

#[tokio::test]
async fn claude_stream_preserves_message_identity_and_correlates_tool_input_and_result() {
    let runtime = runtime();
    let agent = runtime.spawn(options("claude")).await.unwrap();
    let mut events = agent.events();
    agent.prompt("claude-stream").await.unwrap();
    let events = finish(&mut events).await;
    let text: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::TextDelta { text, item_id } => Some((text.as_str(), item_id.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(text.len(), 2);
    assert!(text.iter().all(|(_, id)| *id == Some("message-stable")));
    assert_eq!(
        text.iter().map(|(text, _)| *text).collect::<String>(),
        "A table:\n| File | Contents |\n|---|---|\n| hello.txt | fixture |"
    );
    let tools: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ToolStarted(tool)
            | EventKind::ToolUpdated(tool)
            | EventKind::ToolCompleted(tool) => Some(tool),
            _ => None,
        })
        .collect();
    assert_eq!(tools.len(), 4);
    assert!(
        tools
            .iter()
            .all(|tool| tool.id == "read-stable" && tool.name.as_deref() == Some("Read"))
    );
    assert_eq!(
        tools.last().unwrap().input,
        json!({"file_path": "hello.txt"})
    );
    assert_eq!(tools.last().unwrap().output, "fixture");
    assert_eq!(tools.last().unwrap().status, "completed");
    agent.close().await.unwrap();
}
#[tokio::test]
async fn native_protocols_support_multiple_ordered_turns_and_raw_events() {
    for provider in ["codex", "pi", "claude", "acp"] {
        let runtime = runtime();
        let agent = runtime.spawn(options(provider)).await.unwrap();
        let mut events = agent.events();
        let mut sequence = 0;
        for _ in 0..2 {
            agent.prompt("hello").await.unwrap();
            let events = finish(&mut events).await;
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e.kind, EventKind::TurnStarted))
                    .count(),
                1,
                "{provider}"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e.kind, EventKind::TurnEnded { .. }))
                    .count(),
                1
            );
            assert!(
                events.iter().any(
                    |e| matches!(&e.kind,EventKind::TextDelta { text,.. } if text.contains("Hello"))
                        && e.raw.is_some()
                ),
                "{provider}: {:?}",
                events
                    .iter()
                    .filter_map(|event| match &event.kind {
                        EventKind::TextDelta { text, .. } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            );
            for e in events {
                assert!(e.sequence > sequence);
                sequence = e.sequence;
            }
        }
        agent.close().await.unwrap();
        assert_eq!(agent.snapshot().await.state, SessionState::Closed);
        let id = agent.snapshot().await.id;
        let resumed = runtime.resume(&id).await.unwrap();
        let mut events = resumed.events();
        resumed.prompt("hello").await.unwrap();
        finish(&mut events).await;
        assert!(resumed.snapshot().await.sequence > sequence);
        resumed.close().await.unwrap();
    }
}
#[tokio::test]
async fn permissions_validate_options_and_are_answered_once() {
    for provider in ["codex", "claude", "acp"] {
        let r = runtime();
        let a = r.spawn(options(provider)).await.unwrap();
        let mut events = a.events();
        a.prompt("permission").await.unwrap();
        let p = loop {
            if let EventKind::PermissionRequested(p) = next(&mut events).await.kind {
                break p;
            }
        };
        assert_eq!(a.snapshot().await.state, SessionState::NeedsInput);
        if provider == "acp" {
            assert_eq!(
                p.options
                    .iter()
                    .find(|o| o.id == "allow_once")
                    .unwrap()
                    .kind,
                "approve_once"
            );
            assert_eq!(
                p.options
                    .iter()
                    .find(|o| o.id == "allow_always")
                    .unwrap()
                    .kind,
                "approve_session"
            );
            assert_eq!(
                p.options.iter().find(|o| o.id == "opaque").unwrap().kind,
                "unknown"
            );
        }
        assert!(
            a.respond_permission(PermissionResponse {
                answers: Default::default(),
                id: p.id.clone(),
                option: "invalid".into(),
                data: json!(null)
            })
            .await
            .is_err()
        );
        let response = PermissionResponse {
            answers: Default::default(),
            id: p.id,
            option: p
                .options
                .iter()
                .find(|o| matches!(o.kind.as_str(), "approve" | "approve_once"))
                .unwrap()
                .id
                .clone(),
            data: json!(null),
        };
        a.respond_permission(response.clone()).await.unwrap();
        assert!(a.respond_permission(response).await.is_err());
        finish(&mut events).await;
        a.close().await.unwrap();
    }
}
#[tokio::test]
async fn pi_message_end_closes_each_assistant_message_without_closing_a_retried_turn() {
    let runtime = runtime();
    let agent = runtime.spawn(options("pi")).await.unwrap();
    let mut events = agent.events();
    agent.prompt("retry-error").await.unwrap();
    let events = finish(&mut events).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::MessageEnded { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::TurnEnded { .. }))
            .count(),
        1
    );
    assert!(
        matches!(&events.last().unwrap().kind,EventKind::TurnEnded { outcome, .. } if outcome.as_str() == "completed")
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn active_turns_reject_new_prompts_and_support_cancel() {
    for provider in ["codex", "pi", "claude", "acp"] {
        let r = runtime();
        let a = r.spawn(options(provider)).await.unwrap();
        let mut events = a.events();
        a.prompt("hold").await.unwrap();
        assert!(matches!(a.prompt("second").await, Err(Error::Busy)));
        if a.capabilities().await.steering.is_available() {
            a.steer("constraint").await.unwrap();
        } else {
            assert!(matches!(
                a.steer("constraint").await,
                Err(Error::Unsupported(_))
            ));
        }
        a.cancel().await.unwrap();
        finish(&mut events).await;
        a.close().await.unwrap();
    }
}
#[tokio::test]
async fn pi_waits_for_settled_and_handled_commands_do_not_hang() {
    let r = runtime();
    let a = r.spawn(options("pi")).await.unwrap();
    let mut events = a.events();
    a.prompt("retry").await.unwrap();
    let out = finish(&mut events).await;
    assert!(
        out.iter()
            .any(|e| matches!(&e.kind,EventKind::TextDelta { text,.. } if text == "retry output"))
    );
    a.prompt("handled").await.unwrap();
    let out = finish(&mut events).await;
    assert!(out.iter().any(
        |e| matches!(&e.kind,EventKind::TurnEnded { outcome, .. } if outcome.as_str() == "handled")
    ));
    a.close().await.unwrap();
}
#[tokio::test]
async fn crashes_end_active_turn_and_fail_the_session() {
    for provider in ["pi", "claude", "acp"] {
        let r = runtime();
        let a = r.spawn(options(provider)).await.unwrap();
        let mut events = a.events();
        let result = a.prompt("crash").await;
        if provider == "pi" {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if a.snapshot().await.state == SessionState::Failed {
                    break;
                }
                let _ = next(&mut events).await;
            }
        })
        .await
        .unwrap();
    }
}
#[tokio::test]
async fn unqualified_native_errors_block_followups_without_a_confirmed_turn() {
    for provider in ["codex", "pi"] {
        let r = runtime();
        let a = r.spawn(options(provider)).await.unwrap();
        assert!(a.prompt("reject").await.is_err());
        assert!(a.snapshot().await.turn.is_none());
        assert_eq!(a.snapshot().await.state, SessionState::NeedsInput);
        assert!(matches!(a.prompt("hello").await, Err(Error::Busy)));
        a.close().await.unwrap();
    }
}

struct SlowObserver;
#[async_trait::async_trait]
impl RuntimeExtension for SlowObserver {
    fn id(&self) -> &str {
        "slow-observer"
    }
    async fn on_event(&self, _: &RuntimeEvent) {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}
#[tokio::test]
async fn acp_terminal_never_overtakes_content_under_backpressure() {
    let r = builtin_builder()
        .harness(AcpHarness::new("acp", "python3", vec![]))
        .extension(SlowObserver)
        .build();
    let a = r.spawn(options("acp")).await.unwrap();
    let mut events = a.events();
    a.prompt("burst").await.unwrap();
    let out = finish(&mut events).await;
    assert_eq!(
        out.iter()
            .filter(|e| matches!(e.kind, EventKind::TextDelta { .. }))
            .count(),
        800
    );
    assert!(
        out.iter()
            .filter(|e| matches!(e.kind, EventKind::TextDelta { .. }))
            .all(|e| e.turn_id.is_some())
    );
    a.close().await.unwrap();
}
#[tokio::test]
async fn cancellation_reason_is_preserved_and_closed_stream_finishes() {
    let r = runtime();
    let a = r.spawn(options("pi")).await.unwrap();
    let mut events = a.events();
    a.prompt("hold").await.unwrap();
    a.cancel().await.unwrap();
    let out = finish(&mut events).await;
    assert!(
        out.iter()
            .any(|e| matches!(&e.kind,EventKind::TurnEnded { outcome, .. } if outcome.as_str() == "cancelled"))
    );
    a.close().await.unwrap();
    tokio::time::timeout(Duration::from_millis(300), async {
        while events.next().await.is_some() {}
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn opencode_ignores_old_idle_and_user_text_and_does_not_duplicate_deltas() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut server = tokio::process::Command::new("python3")
        .args([
            root.join("tests/fixtures/opencode.py").to_str().unwrap(),
            "--port",
            &port.to_string(),
        ])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let r = runtime();
    let mut o = options("opencode");
    o.config = json!({"url":format!("http://127.0.0.1:{port}")});
    let a = r.spawn(o).await.unwrap();
    let mut events = a.events();
    a.prompt("hold").await.unwrap();
    a.cancel().await.unwrap();
    finish(&mut events).await;
    a.prompt("hello").await.unwrap();
    let out = finish(&mut events).await;
    let text = out
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "Fixture output");
    assert!(
        !out.iter()
            .any(|e| matches!(&e.kind,EventKind::AssistantMessage(v) if v["role"] == "user"))
    );
    a.close().await.unwrap();
    server.kill().await.unwrap();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn pi_retry_success_is_terminal_success_and_user_messages_are_not_assistant() {
    let r = runtime();
    let a = r.spawn(options("pi")).await.unwrap();
    let mut events = a.events();
    a.prompt("retry-error").await.unwrap();
    let out = finish(&mut events).await;
    assert!(
        out.iter()
            .any(|e| matches!(e.kind, EventKind::Error { .. }))
    );
    assert!(
        out.iter()
            .any(|e| matches!(&e.kind,EventKind::TurnEnded { outcome, .. } if outcome.as_str() == "completed"))
    );
    assert!(
        !out.iter()
            .any(|e| matches!(&e.kind,EventKind::AssistantMessage(v) if v["role"] == "user"))
    );
    a.close().await.unwrap();
}

#[tokio::test]
async fn terminal_outcomes_are_portable_and_retain_native_reasons() {
    let cases = [
        (
            "codex",
            json!({"status":"completed"}),
            TurnOutcome::Completed,
            Some("completed"),
        ),
        (
            "codex",
            json!({"status":"interrupted"}),
            TurnOutcome::Interrupted,
            Some("interrupted"),
        ),
        (
            "codex",
            json!({"status":"failed"}),
            TurnOutcome::Failed,
            Some("failed"),
        ),
        (
            "codex",
            json!({"status":"failed","error":{"codexErrorInfo":"contextWindowExceeded"}}),
            TurnOutcome::Limited,
            Some("failed"),
        ),
        (
            "codex",
            json!({"status":"future_status"}),
            TurnOutcome::Unknown,
            Some("future_status"),
        ),
        ("codex", json!({}), TurnOutcome::Unknown, None),
        (
            "claude",
            json!({"subtype":"success","is_error":false}),
            TurnOutcome::Completed,
            Some("success"),
        ),
        (
            "claude",
            json!({"subtype":"error_during_execution","is_error":true}),
            TurnOutcome::Failed,
            Some("error_during_execution"),
        ),
        (
            "claude",
            json!({"subtype":"error_max_turns","is_error":true}),
            TurnOutcome::Limited,
            Some("error_max_turns"),
        ),
        (
            "claude",
            json!({"subtype":"error_max_budget_usd","is_error":true}),
            TurnOutcome::Limited,
            Some("error_max_budget_usd"),
        ),
        (
            "claude",
            json!({"subtype":"future_reason"}),
            TurnOutcome::Unknown,
            Some("future_reason"),
        ),
        (
            "pi",
            json!({"stopReason":"stop"}),
            TurnOutcome::Completed,
            Some("stop"),
        ),
        (
            "pi",
            json!({"stopReason":"length"}),
            TurnOutcome::Limited,
            Some("length"),
        ),
        (
            "pi",
            json!({"stopReason":"error"}),
            TurnOutcome::Failed,
            Some("error"),
        ),
        (
            "pi",
            json!({"stopReason":"aborted"}),
            TurnOutcome::Cancelled,
            Some("aborted"),
        ),
        (
            "pi",
            json!({"stopReason":"future_reason"}),
            TurnOutcome::Unknown,
            Some("future_reason"),
        ),
        (
            "acp",
            json!({"stopReason":"end_turn"}),
            TurnOutcome::Completed,
            Some("end_turn"),
        ),
        (
            "acp",
            json!({"stopReason":"cancelled"}),
            TurnOutcome::Cancelled,
            Some("cancelled"),
        ),
        (
            "acp",
            json!({"stopReason":"max_tokens"}),
            TurnOutcome::Limited,
            Some("max_tokens"),
        ),
        (
            "acp",
            json!({"stopReason":"max_turn_requests"}),
            TurnOutcome::Limited,
            Some("max_turn_requests"),
        ),
        (
            "acp",
            json!({"stopReason":"refusal"}),
            TurnOutcome::Refused,
            Some("refusal"),
        ),
        (
            "acp",
            json!({"stopReason":"future_reason"}),
            TurnOutcome::Unknown,
            Some("future_reason"),
        ),
        ("acp", json!({}), TurnOutcome::Unknown, None),
    ];
    for (provider, payload, expected, native) in cases {
        let runtime = runtime();
        let agent = runtime.spawn(options(provider)).await.unwrap();
        let mut events = agent.events();
        agent
            .prompt(format!("terminal:{payload}").as_str())
            .await
            .unwrap();
        let records = finish(&mut events).await;
        let terminal = records.last().unwrap();
        let EventKind::TurnEnded {
            outcome,
            native_reason,
        } = &terminal.kind
        else {
            unreachable!()
        };
        assert_eq!(*outcome, expected, "{provider}: {payload}");
        assert_eq!(native_reason.as_deref(), native, "{provider}: {payload}");
        assert!(terminal.turn_id.is_some());
        assert!(terminal.raw.is_some());
        let serialized = serde_json::to_value(terminal).unwrap();
        assert_eq!(serialized["data"]["outcome"], expected.as_str());
        assert!(
            !serialized["data"]
                .as_object()
                .unwrap()
                .contains_key("reason")
        );
        if let Some(native) = native {
            assert_eq!(serialized["data"]["nativeReason"], native);
        }
        agent.close().await.unwrap();
    }
}

#[tokio::test]
async fn acp_discovery_does_not_claim_session_catalogs_are_unsupported() {
    let runtime = runtime();
    let detection = runtime
        .discover()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.harness == "acp")
        .unwrap();
    assert_eq!(
        detection.capabilities.models.availability,
        CapabilityAvailability::Unknown
    );
    assert_eq!(
        detection.capabilities.configure.availability,
        CapabilityAvailability::Unknown
    );
    let agent = runtime.spawn(options("acp")).await.unwrap();
    let capabilities = agent.capabilities().await;
    assert_eq!(capabilities.models.evidence, CapabilityEvidence::Negotiated);
    assert_eq!(
        capabilities.configure.evidence,
        CapabilityEvidence::Negotiated
    );
    assert!(capabilities.queue.is_available());
    assert_eq!(capabilities.queue.source, CapabilitySource::Runtime);
    agent.close().await.unwrap();
}
