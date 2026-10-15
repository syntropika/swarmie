#![cfg(unix)]
use adapters::CursorHarness;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use swarmie::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("swarmie-cursor-sdk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::create_dir_all(path.join("bin")).unwrap();
        let launcher = path.join("bin/cursor-sdk-bridge");
        // Parallel forks can inherit a newly written launcher's writable descriptor
        // until exec, even after its creating thread closes it. Execute the immutable
        // interpreter instead, retaining the distribution layout for manifest checks.
        let interpreter = process::find_executable("python3")
            .expect("python3 fixture interpreter")
            .canonicalize()
            .unwrap();
        std::os::unix::fs::symlink(interpreter, &launcher).unwrap();
        std::fs::write(
            path.join("manifest.json"),
            r#"{"protocol":"sdk.v1","sdkVersion":"1.0.37"}"#,
        )
        .unwrap();
        Self(path)
    }
    fn options(&self) -> SpawnOptions {
        let mut options = SpawnOptions::new("cursor", &self.0);
        options.config = json!({
            "executable":self.0.join("bin/cursor-sdk-bridge"),
            "args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cursor_sdk_bridge.py")]
        });
        options
            .execution
            .env
            .insert("CURSOR_API_KEY".into(), "fixture-cursor-account-key".into());
        options
    }
    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.0.join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn finish(connection: &mut BackendConnection) -> (TurnOutcome, Vec<EventKind>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        loop {
            match connection.events.recv().await.unwrap() {
                BackendEvent::Content { kind, .. } => events.push(*kind),
                BackendEvent::TurnFinished { outcome, .. } => return (outcome, events),
                event => panic!("unexpected Cursor event: {event:?}"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn native_sdk_connect_streams_authenticated_frames_without_duplicate_text() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options.model = Some("model-b".into());
    options.execution.mcp_servers.push(McpServer {
        name: "fixture".into(),
        transport: McpTransport::Stdio {
            command: "fixture-mcp".into(),
            args: vec!["--stdio".into()],
            env: Default::default(),
        },
    });
    let mut connection = CursorHarness.connect(&options, None, false).await.unwrap();
    assert_eq!(connection.native.id, "native-agent");
    assert!(connection.capabilities.resume.is_available());
    assert!(!connection.capabilities.permissions.is_available());
    assert!(!connection.capabilities.fork.is_available());
    connection
        .backend
        .prompt(&Prompt {
            content: vec![
                PromptContent::Text("hello".into()),
                PromptContent::Image {
                    data: "AA==".into(),
                    mime_type: "image/png".into(),
                },
            ],
        })
        .await
        .unwrap();
    let (outcome, events) = finish(&mut connection).await;
    assert_eq!(outcome, TurnOutcome::Completed);
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            EventKind::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EventKind::ReasoningDelta { text } if text == "Inspect"))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, EventKind::ToolCompleted(_)))
            .count(),
        1
    );
    assert!(events.iter().any(|event| matches!(event, EventKind::ToolStarted(tool) if tool.id == "tool-1" && tool.name.as_deref() == Some("read"))));
    assert!(events.iter().any(
        |event| matches!(event, EventKind::ToolCompleted(tool) if tool.status == "completed")
    ));
    assert!(events.iter().any(
        |event| matches!(event, EventKind::UsageUpdated(usage) if usage.total_tokens == Some(15))
    ));
    let requests = fixture.requests();
    let create = requests
        .iter()
        .find(|value| value["method"] == "CreateAgent")
        .unwrap();
    assert_eq!(create["request"]["options"]["model"]["id"], "model-b");
    assert_eq!(
        create["request"]["options"]["mcpServers"]["fixture"]["stdio"]["command"],
        "fixture-mcp"
    );
    let send = requests
        .iter()
        .find(|value| value["method"] == "Send")
        .unwrap();
    assert_eq!(
        send["request"]["message"]["images"][0]["data"]["mimeType"],
        "image/png"
    );
    assert!(
        !serde_json::to_string(&options)
            .unwrap()
            .contains("fixture-cursor-account-key")
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_uses_native_run_id_and_waits_for_cancelled_result() {
    let fixture = Fixture::new();
    let mut connection = CursorHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("cancel"))
        .await
        .unwrap();
    // The native init frame may not have arrived yet. Cancellation is retained
    // until its run ID is known rather than cancelling an unrelated agent.
    connection.backend.cancel().await.unwrap();
    let (outcome, _) = finish(&mut connection).await;
    assert_eq!(outcome, TurnOutcome::Cancelled);
    assert!(
        fixture.requests().iter().any(
            |value| value["method"] == "CancelRun" && value["request"]["runId"] == "native-run"
        )
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn disconnected_live_stream_is_resolved_without_resubmission() {
    let fixture = Fixture::new();
    let mut connection = CursorHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("disconnect"))
        .await
        .unwrap();
    let (outcome, _) = finish(&mut connection).await;
    assert_eq!(outcome, TurnOutcome::Completed);
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|value| value["method"] == "Send")
            .count(),
        1
    );
    assert!(
        requests
            .iter()
            .any(|value| value["method"] == "WaitLiveRun")
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn resume_preserves_native_agent_identity_and_configures_next_send() {
    let fixture = Fixture::new();
    let mut connection = CursorHarness
        .connect(
            &fixture.options(),
            Some(&NativeSessionRef {
                provider: "cursor".into(),
                id: "native-agent".into(),
            }),
            false,
        )
        .await
        .unwrap();
    connection
        .backend
        .configure(json!({"model":"model-b","mode":"plan"}))
        .await
        .unwrap();
    assert!(matches!(
        connection.backend.configure(json!({"mode":"ask"})).await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        connection
            .backend
            .configure(json!({"executionProfile":"ask"}))
            .await,
        Err(Error::Invalid(_))
    ));
    connection
        .backend
        .prompt(&Prompt::from("hello"))
        .await
        .unwrap();
    finish(&mut connection).await;
    let requests = fixture.requests();
    assert!(
        requests
            .iter()
            .any(|value| value["method"] == "ResumeAgent")
    );
    assert!(
        !requests
            .iter()
            .any(|value| value["method"] == "CreateAgent")
    );
    let send = requests
        .iter()
        .find(|value| value["method"] == "Send")
        .unwrap();
    assert_eq!(send["request"]["options"]["model"]["id"], "model-b");
    assert_eq!(send["request"]["options"]["mode"], "AGENT_MODE_OPTION_PLAN");
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn unsupported_policies_and_lifecycle_options_fail_before_native_effects() {
    let fixture = Fixture::new();
    for config in [
        json!({"executionProfile":"ask"}),
        json!({"approvalPolicy":"never"}),
        json!({"sandbox":"none"}),
        json!({"apiKey":"must-not-be-persisted"}),
    ] {
        let mut options = fixture.options();
        options
            .config
            .as_object_mut()
            .unwrap()
            .extend(config.as_object().unwrap().clone());
        assert!(matches!(
            CursorHarness.connect(&options, None, false).await,
            Err(Error::Unsupported(_))
        ));
    }
    assert!(matches!(
        CursorHarness.connect(&fixture.options(), None, true).await,
        Err(Error::Unsupported(_))
    ));
    let mut options = fixture.options();
    options.execution.ephemeral = true;
    assert!(matches!(
        CursorHarness.connect(&options, None, false).await,
        Err(Error::Unsupported(_))
    ));
    assert!(!fixture.0.join("requests.jsonl").exists());
}

#[tokio::test]
async fn malformed_or_remote_discovery_is_rejected_without_leaking_stderr() {
    for variable in [
        "FIXTURE_BAD_DISCOVERY",
        "FIXTURE_REMOTE_DISCOVERY",
        "FIXTURE_BAD_TOKEN",
        "FIXTURE_EXIT_EARLY",
        "FIXTURE_LONG_LINE",
    ] {
        let fixture = Fixture::new();
        let mut options = fixture.options();
        options.execution.env.insert(variable.into(), "1".into());
        let error = match CursorHarness.connect(&options, None, false).await {
            Ok(_) => panic!("invalid bridge accepted"),
            Err(error) => error,
        };
        assert!(!error.to_string().contains("fixture-cursor-account-key"));
        assert!(!error.to_string().contains("fixture-local-bridge-token"));
        assert!(!fixture.0.join("requests.jsonl").exists());
    }
}

#[tokio::test]
async fn strict_tool_disable_removes_builtin_and_implicit_mcp_sources() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options.execution.tools = ToolPolicy::Disabled;
    let connection = CursorHarness.connect(&options, None, false).await.unwrap();
    let requests = fixture.requests();
    let create = requests
        .iter()
        .find(|value| value["method"] == "CreateAgent")
        .unwrap();
    assert_eq!(create["request"]["options"]["tools"], json!({"names":[]}));
    assert_eq!(
        create["request"]["options"]["local"]["settingSources"],
        json!([])
    );
    assert_eq!(create["request"]["options"]["mcpServers"], json!({}));
    connection.backend.close().await.unwrap();
    std::fs::remove_file(fixture.0.join("manifest.json")).unwrap();
    assert!(matches!(
        CursorHarness.connect(&options, None, false).await,
        Err(Error::Unsupported(_))
    ));
    options.execution.mcp_servers.push(McpServer {
        name: "incompatible".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1".into(),
            headers: Default::default(),
        },
    });
    assert!(matches!(
        CursorHarness.connect(&options, None, false).await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn repeated_turns_release_native_busy_guard_before_terminal_events() {
    let fixture = Fixture::new();
    let mut connection = CursorHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    for _ in 0..4 {
        connection
            .backend
            .prompt(&Prompt::from("hello"))
            .await
            .unwrap();
        assert_eq!(finish(&mut connection).await.0, TurnOutcome::Completed);
    }
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|value| value["method"] == "Send")
            .count(),
        4
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn ambiguous_submission_failure_closes_route_before_another_prompt() {
    let fixture = Fixture::new();
    let connection = CursorHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    assert!(matches!(
        connection.backend.prompt(&Prompt::from("ambiguous")).await,
        Err(Error::Provider(_))
    ));
    assert!(matches!(
        connection.backend.prompt(&Prompt::from("again")).await,
        Err(Error::Rejected(_))
    ));
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|value| value["method"] == "Send")
            .count(),
        1
    );
}

#[tokio::test]
async fn native_failure_preserves_redacted_status_reason() {
    let fixture = Fixture::new();
    let mut connection = CursorHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("failed"))
        .await
        .unwrap();
    let (outcome, events) = finish(&mut connection).await;
    assert_eq!(outcome, TurnOutcome::Failed);
    assert!(events.iter().any(|event| matches!(event, EventKind::Error { message } if message.starts_with("Fixture failure:") && message.contains("[redacted]"))));
    let encoded = serde_json::to_string(&events).unwrap();
    assert!(!encoded.contains("fixture-cursor-account-key"));
    assert!(!encoded.contains("fixture-local-bridge-token"));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn an_unresolved_accepted_stream_does_not_admit_another_native_turn() {
    let fixture = Fixture::new();
    let mut connection = CursorHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("unresolved"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let BackendEvent::Exited { message } = connection.events.recv().await.unwrap() {
                assert!(message.contains("unresolved"));
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        connection.backend.prompt(&Prompt::from("again")).await,
        Err(Error::Rejected(_))
    ));
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|value| value["method"] == "Send")
            .count(),
        1
    );
}
