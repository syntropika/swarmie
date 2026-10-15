#![cfg(unix)]
use adapters::AntigravityHarness;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use swarmie::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("swarmie-antigravity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn options(&self) -> SpawnOptions {
        let mut options = SpawnOptions::new("antigravity", &self.0);
        options.config = json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/antigravity_agent.py")]});
        options
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn finish(connection: &mut BackendConnection) -> (Value, Vec<EventKind>, String) {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut text = String::new();
        let mut events = vec![];
        while let Some(event) = connection.events.recv().await {
            match event {
                BackendEvent::Content { kind, .. } => {
                    if let EventKind::TextDelta { text: delta, .. } = &*kind {
                        text.push_str(delta);
                    }
                    events.push(*kind);
                }
                BackendEvent::TurnFinished { outcome, .. } => {
                    return (
                        serde_json::from_str(&text).unwrap_or(Value::Null),
                        events,
                        outcome.to_string(),
                    );
                }
                BackendEvent::Exited { message } => panic!("peer exited unexpectedly: {message}"),
                _ => (),
            }
        }
        panic!("peer did not finish")
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn native_stream_applies_flags_schema_and_normalizes_sparse_tools_and_session_usage() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options.model = Some("fixture-model".into());
    options.config["mode"] = json!("plan");
    options.config["effort"] = json!("high");
    options
        .execution
        .env
        .insert("SWARMIE_NATIVE_ENV".into(), "isolated".into());
    options.execution.output_schema =
        Some(json!({"type":"object","properties":{"ok":{"type":"boolean"}}}));
    let mut connection = AntigravityHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    assert_eq!(connection.native.id, "native-conversation");
    assert!(!connection.capabilities.permissions.is_available());
    assert!(!connection.capabilities.fork.is_available());
    assert_eq!(
        connection.capabilities.metadata["wire"],
        "native_stream_json"
    );
    connection
        .backend
        .prompt(&Prompt::from("first"))
        .await
        .unwrap();
    let (receipt, events, reason) = finish(&mut connection).await;
    assert_eq!(reason, "completed");
    assert_eq!(receipt["env"], "isolated");
    assert_eq!(
        receipt["prompt"],
        json!({"event":"user","message":{"content":"first"}})
    );
    for flag in [
        "--input-format=stream-json",
        "--output-format=stream-json",
        "--add-dir",
        "--model",
        "fixture-model",
        "--mode",
        "plan",
        "--effort",
        "high",
        "--json-schema",
    ] {
        assert!(
            receipt["args"].as_array().unwrap().contains(&json!(flag)),
            "missing {flag}"
        );
    }
    assert!(events.iter().any(|e| matches!(e, EventKind::ToolCompleted(t) if t.id == "step-2" && t.name.as_deref() == Some("view_file") && t.input["TargetFile"] == "hello.txt" && t.output == "contents")));
    assert!(events.iter().any(|e| matches!(e, EventKind::SubagentUpdated(child) if child.id.as_deref() == Some("child-conversation") && child.parent_session_id.as_deref() == Some("native-conversation"))));
    assert!(events.iter().any(|e| matches!(e, EventKind::ContextUpdated(c) if c.used_tokens == Some(157) && c.context_window_tokens.is_none())));
    assert!(events.iter().any(|e| matches!(e, EventKind::UsageUpdated(u) if u.scope == "session" && u.total_tokens == Some(77777) && u.cached_input_tokens == Some(50000))));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventKind::StructuredOutput(output) if output["ok"] == true))
    );
    assert_eq!(
        json!(connection.backend.models().await.unwrap())[0]["id"],
        "fixture-model"
    );
    connection
        .backend
        .prompt(&Prompt::from("second"))
        .await
        .unwrap();
    assert_eq!(finish(&mut connection).await.0["pid"], receipt["pid"]);
    connection.backend.close().await.unwrap();
}
#[tokio::test]
async fn native_cancel_settles_tools_and_resumes_the_same_conversation_in_a_new_process() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let mut connection = AntigravityHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("slow"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(BackendEvent::Content { kind, .. }) = connection.events.recv().await
                && matches!(*kind, EventKind::ToolStarted(_))
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    connection.backend.cancel().await.unwrap();
    let (_, events, reason) = finish(&mut connection).await;
    assert_eq!(reason, "cancelled");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventKind::ToolCompleted(tool) if tool.status == "cancelled"))
    );
    connection
        .backend
        .prompt(&Prompt::from("after cancellation"))
        .await
        .unwrap();
    let (receipt, _, reason) = finish(&mut connection).await;
    assert_eq!(reason, "completed");
    assert_eq!(receipt["session"], connection.native.id);
    let launches: Vec<Value> = std::fs::read_to_string(fixture.0.join("launches.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(launches.len(), 2);
    assert_ne!(launches[0]["pid"], launches[1]["pid"]);
    assert!(
        launches[1]["args"]
            .as_array()
            .unwrap()
            .contains(&json!("--conversation"))
    );
    assert_eq!(receipt["pid"], launches[1]["pid"]);
    connection.backend.close().await.unwrap();
}
#[tokio::test]
async fn native_resume_preserves_reference_and_auth_errors_are_not_faked() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let saved = NativeSessionRef {
        provider: "antigravity".into(),
        id: "saved-conversation".into(),
    };
    let connection = AntigravityHarness
        .connect(&options, Some(&saved), false)
        .await
        .unwrap();
    assert_eq!(connection.native.id, saved.id);
    connection.backend.close().await.unwrap();
    let mut failed = options;
    failed
        .execution
        .env
        .insert("SWARMIE_FAIL_INIT".into(), "1".into());
    let error = AntigravityHarness
        .connect(&failed, None, false)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("authentication required"));
}
#[tokio::test]
async fn unsupported_execution_and_non_text_prompts_fail_explicitly() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let mut variants = vec![];
    let mut tools = options.clone();
    tools.execution.tools = ToolPolicy::Disabled;
    variants.push(tools);
    let mut ephemeral = options.clone();
    ephemeral.execution.ephemeral = true;
    variants.push(ephemeral);
    let mut mcp = options.clone();
    mcp.execution.mcp_servers.push(McpServer {
        name: "fixture".into(),
        transport: McpTransport::Stdio {
            command: "server".into(),
            args: vec![],
            env: Default::default(),
        },
    });
    variants.push(mcp);
    for variant in variants {
        assert!(matches!(
            AntigravityHarness.connect(&variant, None, false).await,
            Err(Error::Unsupported(_))
        ));
    }
    assert!(matches!(
        AntigravityHarness.connect(&options, None, true).await,
        Err(Error::Unsupported(_))
    ));
    let connection = AntigravityHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    assert!(matches!(
        connection
            .backend
            .prompt(&Prompt {
                content: vec![PromptContent::Image {
                    data: "AA==".into(),
                    mime_type: "image/png".into()
                }]
            })
            .await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        connection.backend.configure(json!({"model":"other"})).await,
        Err(Error::Unsupported(_))
    ));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn managed_gemini_launch_uses_private_native_home_and_preserves_parent_home() {
    let fixture = Fixture::new();
    let parent_home = std::env::var_os("HOME");
    let overlay = adapters::setup::providers::credential_overlay(
        "antigravity",
        "google",
        "fixture-private-gemini-key",
        &fixture.0,
    )
    .unwrap();
    let private_home = overlay.config_home.clone().unwrap().canonicalize().unwrap();
    let mut options = fixture.options();
    options.execution.config_home = overlay.config_home;
    options.execution.env = overlay.env;
    let mut connection = AntigravityHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("home"))
        .await
        .unwrap();
    let (receipt, _, outcome) = finish(&mut connection).await;
    assert_eq!(outcome, "completed");
    assert_eq!(receipt["home"], private_home.to_string_lossy().as_ref());
    assert!(receipt["geminiHome"].is_null());
    assert_eq!(receipt["settings"]["modelProvider"], "gemini");
    assert_eq!(receipt["geminiKeyPresent"], true);
    assert!(!receipt.to_string().contains("fixture-private-gemini-key"));
    assert_eq!(std::env::var_os("HOME"), parent_home);
    connection.backend.close().await.unwrap();
}
