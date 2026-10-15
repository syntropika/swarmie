#![cfg(unix)]

use adapters::CodexHarness;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use swarmie::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("swarmie-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn options(&self) -> SpawnOptions {
        let mut options = SpawnOptions::new("codex", &self.0);
        let peer = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/codex_features.py")
            .canonicalize()
            .unwrap();
        options.config = json!({"executable":"python3", "args":[peer]});
        options.execution.env.insert(
            "SWARMIE_FIXTURE_LEDGER".into(),
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
        .insert("SWARMIE_TEST_VALUE".into(), "isolated".into());
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
    let selected_skill = fixture.0.join("second/SKILL.md");
    std::fs::create_dir_all(selected_skill.parent().unwrap()).unwrap();
    std::fs::write(
        &selected_skill,
        "---\nname: same-name\ndescription: Test native selected input.\n---\nRun the fixture.\n",
    )
    .unwrap();
    options.execution.env.insert(
        "SWARMIE_SELECTED_SKILL".into(),
        selected_skill.to_string_lossy().into_owned(),
    );
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
    let configurations = json!(connection.backend.configuration().await.unwrap());
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
    assert!(connection.capabilities.native_skill_inputs.is_available());
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
    let descriptors = json!(connection.backend.configuration().await.unwrap());
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
async fn commands_and_quotas_retain_native_enablement_and_the_correct_limit_bucket() {
    let fixture = Fixture::new();
    let connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    let commands = connection.backend.commands().await.unwrap();
    assert_eq!(commands.as_array().unwrap().len(), 2);
    assert_eq!(commands[0]["name"], "review");
    assert_eq!(commands[0]["enabled"], true);
    assert_eq!(commands[1]["enabled"], false);
    assert_eq!(commands[1]["availability"], "blocked-by-native");
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
    let configuration = json!(connection.backend.configuration().await.unwrap());
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
    assert!(!connection.capabilities.fork.is_available());
    assert!(!connection.capabilities.fork_at_message.is_available());
    assert!(!connection.capabilities.rollback.is_available());
    assert_eq!(
        connection.capabilities.metadata["rollbackScopes"],
        json!([])
    );
    assert!(matches!(
        connection.backend.rollback(1).await,
        Err(Error::Unsupported(_))
    ));
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
        .insert("SWARMIE_TEST_VALUE".into(), "fake-recording-secret".into());
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

#[tokio::test]
async fn rollback_resolves_the_excluded_turn_across_native_pages_without_changing_files() {
    let fixture = Fixture::new();
    let mut options = fixture.options();
    options
        .execution
        .env
        .insert("SWARMIE_FIXTURE_PAGE_SIZE".into(), "2".into());
    let mut connection = CodexHarness.connect(&options, None, false).await.unwrap();
    for text in ["first", "second", "third", "fourth", "fifth"] {
        connection
            .backend
            .prompt(&Prompt::from(text))
            .await
            .unwrap();
        finish(&mut connection).await;
    }
    let sentinel = fixture.0.join("user-output.txt");
    std::fs::write(&sentinel, "User-owned output").unwrap();
    connection.backend.rollback(3).await.unwrap();
    let ledger: Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("ledger.json")).unwrap()).unwrap();
    assert_eq!(ledger[&connection.native.id].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "User-owned output"
    );
    assert!(matches!(
        connection
            .backend
            .rollback_with(1, RollbackScope::ConversationAndFiles)
            .await,
        Err(Error::Unsupported(_))
    ));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn rollback_uses_resumed_native_history_and_can_remove_all_turns() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let mut connection = CodexHarness.connect(&options, None, false).await.unwrap();
    connection
        .backend
        .prompt(&Prompt::from("before reconnect"))
        .await
        .unwrap();
    finish(&mut connection).await;
    let native = connection.native.clone();
    connection.backend.close().await.unwrap();
    let resumed = CodexHarness
        .connect(&options, Some(&native), false)
        .await
        .unwrap();
    resumed.backend.rollback(1).await.unwrap();
    let ledger: Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("ledger.json")).unwrap()).unwrap();
    assert_eq!(ledger[&native.id], json!([]));
    resumed.backend.close().await.unwrap();
}

#[tokio::test]
async fn invalid_rollback_counts_do_not_mutate_native_history() {
    let fixture = Fixture::new();
    let mut connection = CodexHarness
        .connect(&fixture.options(), None, false)
        .await
        .unwrap();
    connection
        .backend
        .prompt(&Prompt::from("keep this turn"))
        .await
        .unwrap();
    finish(&mut connection).await;
    let before = std::fs::read(fixture.0.join("ledger.json")).unwrap();
    for count in [0, 2, u64::MAX] {
        assert!(matches!(
            connection.backend.rollback(count).await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            std::fs::read(fixture.0.join("ledger.json")).unwrap(),
            before
        );
    }
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn rollback_rejects_unusable_anchors_and_nonadvancing_pagination() {
    for fault in [
        "SWARMIE_FIXTURE_MISSING_TURN_ID",
        "SWARMIE_FIXTURE_ACTIVE_TURN",
        "SWARMIE_FIXTURE_REPEAT_CURSOR",
    ] {
        let fixture = Fixture::new();
        let mut options = fixture.options();
        options.execution.env.insert(fault.into(), "1".into());
        options
            .execution
            .env
            .insert("SWARMIE_FIXTURE_PAGE_SIZE".into(), "1".into());
        let mut connection = CodexHarness.connect(&options, None, false).await.unwrap();
        for text in ["first", "second", "third"] {
            connection
                .backend
                .prompt(&Prompt::from(text))
                .await
                .unwrap();
            finish(&mut connection).await;
        }
        let before = std::fs::read(fixture.0.join("ledger.json")).unwrap();
        let result = connection.backend.rollback(3).await;
        assert!(
            matches!(result, Err(Error::Provider(_) | Error::Busy)),
            "{fault}: {result:?}"
        );
        assert_eq!(
            std::fs::read(fixture.0.join("ledger.json")).unwrap(),
            before
        );
        connection.backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn common_configuration_receipt_retains_native_resets_even_when_a_control_disappears() {
    let fixture = Fixture::new();
    let runtime = Runtime::builder().harness(CodexHarness).build();
    let agent = runtime.spawn(fixture.options()).await.unwrap();
    agent
        .configure(json!({"effort":"high","fast":true}))
        .await
        .unwrap();
    let receipt = agent.configure(json!({"model":"low-only"})).await.unwrap();
    assert_eq!(receipt.values["model"], "low-only");
    assert_eq!(receipt.values["effort"], "low");
    assert_eq!(receipt.values["fast"], false);
    assert!(
        !receipt
            .configuration
            .iter()
            .any(|descriptor| descriptor.id == "fast")
    );
    let saved = agent.snapshot().await;
    assert_eq!(saved.options.config["effort"], "low");
    assert_eq!(saved.options.config["fast"], false);
    assert_eq!(saved.options.model.as_deref(), Some("low-only"));
    assert!(
        receipt
            .configuration
            .iter()
            .all(|descriptor| descriptor.application == SettingApplication::NextTurn)
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn native_submission_identity_recovers_a_lost_acknowledgement_after_restart() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let state = fixture.0.join("state");
    let runtime = Runtime::builder()
        .harness(CodexHarness)
        .store(storage::SqliteSessionStore::new(&state).await.unwrap())
        .build();
    let agent = runtime.spawn(options.clone()).await.unwrap();
    let id = agent.snapshot().await.id;
    let original = agent
        .submit("original-key", "uncertain-completed")
        .await
        .unwrap();
    assert_eq!(original.delivery, DeliveryKnowledge::Unknown);
    agent.enqueue("next work").await.unwrap();
    agent.close().await.unwrap();
    drop(agent);
    drop(runtime);
    let runtime = Runtime::builder()
        .harness(CodexHarness)
        .store(storage::SqliteSessionStore::new(&state).await.unwrap())
        .build();
    assert_eq!(
        runtime
            .submission(&id, "original-key")
            .await
            .unwrap()
            .unwrap()
            .delivery,
        DeliveryKnowledge::Unknown
    );
    let resumed = runtime
        .resume_with(&id, options.execution.clone())
        .await
        .unwrap();
    let recovered = resumed.submission("original-key").await.unwrap().unwrap();
    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.delivery, DeliveryKnowledge::Accepted);
    assert_eq!(recovered.outcome, Some(TurnOutcome::Completed));
    assert!(!recovered.recovery_required);
    assert_eq!(resumed.snapshot().await.state, SessionState::Idle);
    assert_eq!(resumed.snapshot().await.turns_completed, 1);
    assert!(resumed.snapshot().await.queue.paused);
    assert_eq!(
        resumed
            .submit("original-key", "uncertain-completed")
            .await
            .unwrap()
            .id,
        original.id
    );
    resumed.close().await.unwrap();
    let again = runtime
        .resume_with(&id, options.execution.clone())
        .await
        .unwrap();
    assert_eq!(again.snapshot().await.turns_completed, 1);
    let events = runtime.transcript(&id, 0, 4096).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::UserMessage(_)))
            .count(),
        1
    );
    let messages: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let EventKind::AssistantMessage(message) = &e.kind {
                Some(message)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["text"], "Recovered answer");
    let endings: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::TurnEnded { .. }))
        .collect();
    assert_eq!(endings.len(), 1);
    assert!(matches!(
        &endings[0].kind,
        EventKind::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        }
    ));
    assert!(endings[0].raw.is_some());
    let ledger: Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("ledger.json")).unwrap()).unwrap();
    assert_eq!(
        ledger
            .as_object()
            .unwrap()
            .values()
            .map(|turns| turns.as_array().unwrap().len())
            .sum::<usize>(),
        1
    );
    again.close().await.unwrap();
}

#[tokio::test]
async fn native_submission_reconciliation_requires_exact_identity_and_complete_unambiguous_history()
{
    let fixture = Fixture::new();
    let options = fixture.options();
    let connection = CodexHarness.connect(&options, None, false).await.unwrap();
    let path = fixture.0.join("ledger.json");
    let thread = &connection.native.id;
    let make_turn = |id: &str, identity: &str, status: &str| {
        json!({"id":id,"status":status,"items":[
        {"id":format!("user-{id}"),"type":"userMessage","clientId":identity,"content":[{"type":"text","text":"same text"}]},
        {"id":format!("answer-{id}"),"type":"agentMessage","text":"answer"}]})
    };
    let save = |turns: Vec<Value>| {
        std::fs::write(&path, serde_json::to_vec(&json!({thread:turns})).unwrap()).unwrap()
    };
    save(vec![make_turn("other", "other-attempt", "completed")]);
    assert!(
        connection
            .backend
            .reconcile_submission("attempt")
            .await
            .unwrap()
            .is_none()
    );
    for status in ["inProgress", "interrupted", "future_status"] {
        save(vec![make_turn("target", "attempt", status)]);
        let evidence = connection
            .backend
            .reconcile_submission("attempt")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.submission_id, "attempt");
        assert!(evidence.outcome.is_none());
        assert!(evidence.messages.is_empty());
    }
    let mut interrupted = make_turn("target", "attempt", "interrupted");
    interrupted["completedAt"] = json!(100);
    save(vec![interrupted]);
    assert_eq!(
        connection
            .backend
            .reconcile_submission("attempt")
            .await
            .unwrap()
            .unwrap()
            .outcome,
        Some(TurnOutcome::Interrupted)
    );
    save(vec![
        make_turn("target", "attempt", "completed"),
        make_turn("duplicate", "attempt", "completed"),
    ]);
    assert!(matches!(
        connection.backend.reconcile_submission("attempt").await,
        Err(Error::Provider(_))
    ));
    let mut malformed = make_turn("target", "attempt", "completed");
    malformed["id"] = Value::Null;
    save(vec![malformed]);
    assert!(matches!(
        connection.backend.reconcile_submission("attempt").await,
        Err(Error::Provider(_))
    ));
    connection.backend.close().await.unwrap();
    let mut options = fixture.options();
    options
        .execution
        .env
        .insert("SWARMIE_FIXTURE_PAGE_SIZE".into(), "1".into());
    let connection = CodexHarness.connect(&options, None, false).await.unwrap();
    let thread = &connection.native.id;
    std::fs::write(&path, serde_json::to_vec(&json!({thread:[make_turn("target", "attempt", "completed"),make_turn("later", "unrelated", "completed")]})).unwrap()).unwrap();
    let evidence = connection
        .backend
        .reconcile_submission("attempt")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(evidence.native_turn_id, "target");
    connection.backend.close().await.unwrap();
    options
        .execution
        .env
        .insert("SWARMIE_FIXTURE_REPEAT_CURSOR".into(), "1".into());
    let connection = CodexHarness.connect(&options, None, false).await.unwrap();
    let thread = &connection.native.id;
    std::fs::write(&path, serde_json::to_vec(&json!({thread:[make_turn("target", "attempt", "completed"),make_turn("later", "unrelated", "completed")]})).unwrap()).unwrap();
    assert!(matches!(
        connection.backend.reconcile_submission("attempt").await,
        Err(Error::Provider(_))
    ));
    connection.backend.close().await.unwrap();
}
