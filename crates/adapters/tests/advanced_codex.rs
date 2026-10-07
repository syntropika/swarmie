#![cfg(unix)]

use adapters::CodexHarness;
use mpai::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("mpai-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn options(&self) -> SpawnOptions {
        let mut options = SpawnOptions::new("codex", &self.0);
        let peer = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/codex_features.py")
            .canonicalize()
            .unwrap();
        options.config = json!({"executable":"python3", "args":[peer]});
        options.execution.env.insert(
            "MPAI_FIXTURE_LEDGER".into(),
            self.0.join("ledger.json").to_string_lossy().into(),
        );
        options
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn finish(connection: &mut BackendConnection) -> (String, Vec<EventKind>) {
    tokio::time::timeout(Duration::from_secs(5), async {
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
                BackendEvent::TurnFinished { .. } => return (text, events),
                BackendEvent::Exited { message } => panic!("provider exited: {message}"),
                _ => (),
            }
        }
        panic!("provider stream closed");
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn execution_maps_mcp_credentials_and_native_turn_policy() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options.execution.config_home = Some(fixture.0.join("home"));
    options
        .execution
        .env
        .insert("MPAI_TEST_VALUE".into(), "isolated".into());
    options.execution.mcp_servers.push(McpServer {
        name: "fixture".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1:1234/mcp".into(),
            headers: BTreeMap::from([("Authorization".into(), "fake-secret".into())]),
        },
    });
    options.execution.output_schema = Some(json!({"type":"object"}));
    options.config["effort"] = json!("high");
    options.config["fast"] = json!(true);
    options.config["sandbox"] = json!("workspace-write");
    options.config["approvalsReviewer"] = json!("auto_review");
    let mut connection = CodexHarness.connect(&options, None, false).await.unwrap();
    connection
        .backend
        .prompt(&Prompt {
            content: vec![
                PromptContent::Text("execution".into()),
                PromptContent::Skill {
                    name: "same-name".into(),
                    path: fixture.0.join("second/SKILL.md").to_string_lossy().into(),
                },
            ],
        })
        .await
        .unwrap();
    let (text, events) = finish(&mut connection).await;
    let receipt: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(receipt["value"], "isolated");
    assert_eq!(receipt["home"], json!(fixture.0.join("home")));
    assert!(
        receipt["headers"]
            .as_object()
            .unwrap()
            .values()
            .any(|v| v == "fake-secret")
    );
    assert!(!receipt["arguments"].to_string().contains("fake-secret"));
    assert!(
        receipt["arguments"]
            .to_string()
            .contains("env_http_headers")
    );
    assert_eq!(receipt["turn"]["serviceTier"], "priority");
    assert_eq!(receipt["turn"]["approvalsReviewer"], "auto_review");
    let configurations = connection.backend.configuration().await.unwrap();
    assert!(
        configurations
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["id"] == "approvalsReviewer"
                && field["value"] == json!("auto_review"))
    );
    assert_eq!(
        receipt["turn"]["input"][1],
        json!({"type":"skill","name":"same-name","path":fixture.0.join("second/SKILL.md")})
    );
    assert_eq!(connection.capabilities.metadata["nativeSkillInputs"], true);
    assert!(receipt["turn"].get("fast").is_none());
    assert_eq!(receipt["turn"]["sandboxPolicy"]["type"], "workspaceWrite");
    assert_eq!(receipt["turn"]["outputSchema"], json!({"type":"object"}));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventKind::StructuredOutput(_)))
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn model_changes_validate_against_the_new_catalog_and_reset_incompatible_settings() {
    let fixture = Fixture::new();
    let mut connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .configure(json!({"effort":"high","fast":true}))
        .await
        .unwrap();
    assert!(matches!(
        connection
            .backend
            .configure(json!({"model":"low-only","effort":"high"}))
            .await,
        Err(Error::Invalid(_))
    ));
    let settings = connection
        .backend
        .configure(json!({"model":"low-only"}))
        .await
        .unwrap();
    assert_eq!(settings["effort"], "low");
    assert_eq!(settings["serviceTier"], "default");
    assert_eq!(settings["fast"], false);
    let descriptors = connection.backend.configuration().await.unwrap();
    assert!(
        !descriptors
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["id"] == "fast")
    );
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    let (text, _) = finish(&mut connection).await;
    let receipt: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(receipt["effort"], "low");
    assert_eq!(receipt["model"], "low-only");
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn commands_and_quotas_use_enabled_native_resources_and_the_correct_limit_bucket() {
    let fixture = Fixture::new();
    let connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    let commands = connection.backend.commands().await.unwrap();
    assert_eq!(commands.as_array().unwrap().len(), 1);
    assert_eq!(commands[0]["name"], "review");
    let quotas = connection.backend.quotas().await.unwrap();
    assert_eq!(quotas["windows"][0]["remainingPercent"], 75.0);
    assert_eq!(quotas["windows"][0]["durationMinutes"], 300);
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn native_declined_cancelled_and_nonzero_exit_tools_are_never_reported_as_successful() {
    let fixture = Fixture::new();
    let mut connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("tool-failures"))
        .await
        .unwrap();
    let (_, events) = finish(&mut connection).await;
    let completed: BTreeMap<_, _> = events
        .iter()
        .filter_map(|event| {
            if let EventKind::ToolCompleted(tool) = event {
                Some((tool.id.as_str(), tool.status.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(completed.get("denied"), Some(&"denied"));
    assert_eq!(completed.get("cancelled"), Some(&"cancelled"));
    assert_eq!(completed.get("failed"), Some(&"failed"));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn native_model_reroute_is_normalized_without_changing_the_requested_model() {
    let fixture = Fixture::new();
    let mut connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("reroute"))
        .await
        .unwrap();
    let (_, events) = finish(&mut connection).await;
    let reroute = events
        .iter()
        .find_map(|event| {
            if let EventKind::ModelRerouted(reroute) = event {
                Some(reroute)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(reroute.from_model, "reasoning");
    assert_eq!(reroute.to_model, "safety-model");
    assert_eq!(reroute.reason.as_deref(), Some("highRiskCyberActivity"));
    let configuration = connection.backend.configuration().await.unwrap();
    assert_eq!(
        configuration
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["id"] == "model")
            .unwrap()["value"],
        "reasoning"
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn children_keep_their_own_messages_completion_and_questions() {
    let fixture = Fixture::new();
    let mut connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("children"))
        .await
        .unwrap();
    let (text, events) = finish(&mut connection).await;
    assert_eq!(text, "parent-only");
    let children: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let EventKind::SubagentUpdated(child) = e {
                Some(child)
            } else {
                None
            }
        })
        .filter(|c| c.id.as_deref() == Some("child"))
        .collect();
    assert!(
        children
            .iter()
            .any(|c| c.text_delta.as_deref() == Some("child-only"))
    );
    assert_eq!(
        children.last().unwrap().status.as_deref(),
        Some("completed")
    );
    connection
        .backend
        .prompt(&Prompt::from("child-question"))
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(BackendEvent::Content { kind, .. }) = connection.events.recv().await
                && let EventKind::PermissionRequested(request) = *kind
            {
                break request;
            }
        }
    })
    .await
    .unwrap();
    connection
        .backend
        .respond_permission(&PermissionResponse {
            answers: Default::default(),
            id: request.id,
            option: "answer".into(),
            data: json!({"answers":{"target":"Linux"}}),
        })
        .await
        .unwrap();
    assert_eq!(finish(&mut connection).await.0, "child-answer-received");
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn fork_at_a_native_message_cuts_the_actual_history_and_keeps_the_parent_independent() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let mut parent = CodexHarness.connect(&options, None, false).await.unwrap();
    parent.backend.prompt(&Prompt::from("first")).await.unwrap();
    let (_, events) = finish(&mut parent).await;
    let anchor = events
        .iter()
        .find_map(|event| {
            if let EventKind::AssistantMessage(value) = event {
                value["id"].as_str().map(String::from)
            } else {
                None
            }
        })
        .unwrap();
    parent
        .backend
        .prompt(&Prompt::from("second"))
        .await
        .unwrap();
    finish(&mut parent).await;
    let mut fork = CodexHarness
        .fork_at(&options, &parent.native, &anchor)
        .await
        .unwrap();
    assert_ne!(fork.native.id, parent.native.id);
    let ledger: Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("ledger.json")).unwrap()).unwrap();
    assert_eq!(ledger[&fork.native.id].as_array().unwrap().len(), 1);
    assert_eq!(ledger[&parent.native.id].as_array().unwrap().len(), 2);
    fork.backend
        .prompt(&Prompt::from("fork-only"))
        .await
        .unwrap();
    finish(&mut fork).await;
    let ledger: Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("ledger.json")).unwrap()).unwrap();
    assert_eq!(ledger[&parent.native.id].as_array().unwrap().len(), 2);
    fork.backend.close().await.unwrap();
    parent.backend.close().await.unwrap();
}

#[tokio::test]
async fn compaction_waits_for_completion_and_ephemeral_sessions_do_not_offer_message_forks() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options.execution.ephemeral = true;
    let connection = CodexHarness.connect(&options, None, false).await.unwrap();
    assert!(!connection.capabilities.fork);
    assert_eq!(connection.capabilities.metadata["forkAtMessage"], false);
    let start = Instant::now();
    connection.backend.compact().await.unwrap();
    assert!(start.elapsed() >= Duration::from_millis(120));
    connection.backend.close().await.unwrap();
    options.execution.tools = ToolPolicy::Disabled;
    options.config = json!({"executable":"/missing-provider"});
    assert!(matches!(
        CodexHarness.connect(&options, None, false).await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn recording_reports_its_actual_path_and_resuming_creates_a_fresh_redacted_trace() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options
        .execution
        .env
        .insert("MPAI_TEST_VALUE".into(), "fake-recording-secret".into());
    options.config["diagnostics"] =
        json!({"recording":{"path":fixture.0.join("wire.jsonl"),"maxBytes":65536}});
    let mut connection = CodexHarness.connect(&options, None, false).await.unwrap();
    connection
        .backend
        .prompt(&Prompt::from("execution"))
        .await
        .unwrap();
    let (_, events) = finish(&mut connection).await;
    let first = events
        .iter()
        .find_map(|e| {
            if let EventKind::Diagnostic(d) = e
                && d["code"] == "wire_recording_started"
            {
                d["path"].as_str().map(PathBuf::from)
            } else {
                None
            }
        })
        .unwrap();
    let native = connection.native.clone();
    connection.backend.close().await.unwrap();
    let mut resumed = CodexHarness
        .connect(&options, Some(&native), false)
        .await
        .unwrap();
    resumed
        .backend
        .prompt(&Prompt::from("execution"))
        .await
        .unwrap();
    let (_, events) = finish(&mut resumed).await;
    let second = events
        .iter()
        .find_map(|e| {
            if let EventKind::Diagnostic(d) = e
                && d["code"] == "wire_recording_started"
            {
                d["path"].as_str().map(PathBuf::from)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(first, second);
    resumed.backend.close().await.unwrap();
    for path in [first, second] {
        let trace = std::fs::read_to_string(path).unwrap();
        assert!(!trace.contains("fake-recording-secret"));
        assert!(trace.contains("turn/start"));
        assert!(trace.len() <= 65536);
    }
}
