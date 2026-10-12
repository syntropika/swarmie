use adapters::builtin_builder;
use futures_util::StreamExt;
use mpai::*;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("mpai-claude-lifecycle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn options(&self) -> SpawnOptions {
        SpawnOptions {
            project_id: None,
            project_revision: None,
            harness: "claude".into(),
            cwd: self.0.clone(),
            model: Some("fixture-model".into()),
            execution: Default::default(),
            config: json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_lifecycle.py")]}),
        }
    }
    fn marker(&self, name: &str) {
        std::fs::write(self.0.join(name), "enabled").unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn next(events: &mut EventStream) -> RuntimeEvent {
    tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}
async fn turn(agent: &Agent, events: &mut EventStream, text: &str) -> (Value, String) {
    agent.prompt(text).await.unwrap();
    let mut body = None;
    let mut anchor = None;
    loop {
        match next(events).await.kind {
            EventKind::AssistantMessage(message) => {
                body = Some(
                    serde_json::from_str(message["content"][0]["text"].as_str().unwrap()).unwrap(),
                );
                anchor = message["uuid"].as_str().map(String::from);
            }
            EventKind::TurnEnded { .. } => break,
            _ => (),
        }
    }
    (body.unwrap(), anchor.unwrap_or_default())
}

#[tokio::test]
async fn conversation_rollback_uses_main_assistant_uuid_replaces_native_connection_and_retains_settings()
 {
    let workspace = Workspace::new();
    let runtime = builtin_builder().build();
    let agent = runtime.spawn(workspace.options()).await.unwrap();
    let mut events = agent.events();
    agent
        .configure(json!({"effort":"high","fast":true}))
        .await
        .unwrap();
    turn(&agent, &mut events, "first").await;
    let (_, cut) = turn(&agent, &mut events, "second").await;
    turn(&agent, &mut events, "third").await;
    let before = agent.snapshot().await;
    assert!(
        before.capabilities.rollback.is_available() && before.capabilities.compact.is_available()
    );
    agent.rollback(1).await.unwrap();
    let after = agent.snapshot().await;
    assert_ne!(after.native.id, before.native.id);
    assert_eq!(after.turns_completed, 2);
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("agent-file.txt")).unwrap(),
        "changed by third"
    );
    assert!(!workspace.0.join("rewind.jsonl").exists());
    let (body, _) = turn(&agent, &mut events, "after rollback").await;
    assert_eq!(body["history"], json!(["first", "second"]));
    assert_eq!(body["session"], after.native.id);
    assert_eq!(body["settings"]["effortLevel"], "high");
    assert_eq!(body["settings"]["fastMode"], true);
    assert_eq!(body["checkpointing"], "true");
    assert!(
        body["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == &json!(format!("--resume-session-at={cut}")))
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn file_rewind_requires_explicit_scope_and_uses_first_dropped_user_anchor() {
    let workspace = Workspace::new();
    let runtime = builtin_builder().build();
    let agent = runtime.spawn(workspace.options()).await.unwrap();
    let mut events = agent.events();
    turn(&agent, &mut events, "first").await;
    turn(&agent, &mut events, "second").await;
    let before = agent.snapshot().await;
    let transcript: Value = serde_json::from_slice(
        &std::fs::read(
            workspace
                .0
                .join("native-sessions")
                .join(format!("{}.json", before.native.id)),
        )
        .unwrap(),
    )
    .unwrap();
    agent
        .rollback_with(1, RollbackScope::ConversationAndFiles)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("agent-file.txt")).unwrap(),
        "restored by native rewind"
    );
    let requests: Vec<Value> = std::fs::read_to_string(workspace.0.join("rewind.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["dry_run"], true);
    assert_eq!(requests[1]["dry_run"], false);
    assert!(
        requests
            .iter()
            .all(|request| request["user_message_id"] == transcript[1]["user"])
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn failed_fork_or_rewind_keeps_live_conversation_and_resume_history_limit_is_explicit() {
    let workspace = Workspace::new();
    let runtime = builtin_builder().build();
    let agent = runtime.spawn(workspace.options()).await.unwrap();
    let mut events = agent.events();
    turn(&agent, &mut events, "first").await;
    turn(&agent, &mut events, "second").await;
    let before = agent.snapshot().await;
    workspace.marker("fail-fork");
    assert!(matches!(agent.rollback(1).await, Err(Error::Provider(_))));
    assert_eq!(agent.snapshot().await.native.id, before.native.id);
    assert_eq!(agent.snapshot().await.turns_completed, 2);
    std::fs::remove_file(workspace.0.join("fail-fork")).unwrap();
    workspace.marker("deny-actual-rewind");
    assert!(matches!(
        agent
            .rollback_with(1, RollbackScope::ConversationAndFiles)
            .await,
        Err(Error::Provider(_))
    ));
    assert_eq!(agent.snapshot().await.native.id, before.native.id);
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("agent-file.txt")).unwrap(),
        "changed by second"
    );
    let (body, _) = turn(&agent, &mut events, "still usable").await;
    assert_eq!(body["history"], json!(["first", "second"]));
    let id = agent.snapshot().await.id;
    agent.close().await.unwrap();
    let resumed = runtime.resume(&id).await.unwrap();
    let error = resumed.rollback(1).await.unwrap_err().to_string();
    assert!(error.contains("earlier resumed history is unavailable"));
    assert!(
        resumed.snapshot().await.capabilities.metadata["rollbackHistory"]
            .as_str()
            .unwrap()
            .contains("live connection")
    );
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn compaction_waits_for_parent_receipt_propagates_refusal_and_does_not_create_turn_boundaries()
 {
    let workspace = Workspace::new();
    let runtime = builtin_builder().build();
    let agent = runtime.spawn(workspace.options()).await.unwrap();
    let mut events = agent.events();
    turn(&agent, &mut events, "first").await;
    turn(&agent, &mut events, "second").await;
    let mut compact_events = agent.events();
    let sequence = agent.snapshot().await.sequence;
    let started = Instant::now();
    agent.compact().await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(180));
    assert_eq!(agent.snapshot().await.turns_completed, 2);
    let mut finished = false;
    while !finished {
        let event = next(&mut compact_events).await;
        if event.sequence <= sequence {
            continue;
        }
        assert!(!matches!(
            event.kind,
            EventKind::TurnStarted | EventKind::TurnEnded { .. } | EventKind::AssistantMessage(_)
        ));
        finished = matches!(event.kind,EventKind::SessionUpdated(ref value) if value["compaction"]=="finished");
    }
    assert!(
        agent
            .rollback(1)
            .await
            .unwrap_err()
            .to_string()
            .contains("native compaction")
    );
    workspace.marker("refuse-compact");
    assert!(
        agent
            .compact()
            .await
            .unwrap_err()
            .to_string()
            .contains("Not enough messages")
    );
    let (body, _) = turn(&agent, &mut events, "after compact").await;
    assert_eq!(body["history"], json!([]));
    turn(&agent, &mut events, "second after compact").await;
    agent.rollback(1).await.unwrap();
    agent.close().await.unwrap();
}

#[tokio::test]
async fn missing_main_uuid_cannot_be_replaced_by_a_subagent_anchor() {
    let workspace = Workspace::new();
    workspace.marker("omit-main-uuid");
    let agent = builtin_builder()
        .build()
        .spawn(workspace.options())
        .await
        .unwrap();
    let mut events = agent.events();
    turn(&agent, &mut events, "first").await;
    turn(&agent, &mut events, "second").await;
    assert!(
        matches!(agent.rollback(1).await,Err(Error::Unsupported(message)) if message.contains("native assistant UUID"))
    );
    assert_eq!(agent.snapshot().await.turns_completed, 2);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn ephemeral_sessions_and_unadvertised_advanced_model_controls_are_truthful() {
    let workspace = Workspace::new();
    let runtime = builtin_builder().build();
    let mut options = workspace.options();
    options.execution.ephemeral = true;
    let agent = runtime.spawn(options).await.unwrap();
    let capabilities = agent.snapshot().await.capabilities;
    assert!(
        !capabilities.rollback.is_available()
            && !capabilities.resume.is_available()
            && !capabilities.fork.is_available()
    );
    assert_eq!(capabilities.metadata["rollbackScopes"], json!([]));
    assert!(matches!(
        agent.rollback(1).await,
        Err(Error::Unsupported(_))
    ));
    agent.close().await.unwrap();
    let mut options = workspace.options();
    options.model = Some("unadvertised-native-alias".into());
    options.config["effort"] = json!("high");
    assert!(matches!(
        runtime.spawn(options).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn native_queued_cancel_receipt_ends_turn_without_waiting_for_absent_result() {
    let workspace = Workspace::new();
    let agent = builtin_builder()
        .build()
        .spawn(workspace.options())
        .await
        .unwrap();
    let mut events = agent.events();
    agent.prompt("queued-native-cancel").await.unwrap();
    let start = Instant::now();
    agent.cancel().await.unwrap();
    loop {
        if let EventKind::TurnEnded { outcome, .. } = next(&mut events).await.kind {
            assert_eq!(outcome, TurnOutcome::Cancelled);
            break;
        }
    }
    assert!(start.elapsed() < Duration::from_secs(2));
    let (body, _) = turn(&agent, &mut events, "after queued cancel").await;
    assert_eq!(body["history"], json!([]));
    agent.close().await.unwrap();
}

#[tokio::test]
async fn quotas_use_native_receipts_rich_slim_and_explicit_no_subscription() {
    let workspace = Workspace::new();
    let agent = builtin_builder()
        .build()
        .spawn(workspace.options())
        .await
        .unwrap();
    let mut events = agent.events();
    let slim = agent.quotas().await.unwrap();
    assert_eq!(slim["provider"], "claude");
    assert_eq!(slim["plan"], "pro");
    assert_eq!(slim["windows"][0]["remainingPercent"], 80.0);
    assert_eq!(slim["windows"][0]["resetsAt"], 1791075723i64);
    assert!(slim["windows"][1]["resetsAt"].is_null());
    loop {
        if let EventKind::QuotasUpdated(value) = next(&mut events).await.kind {
            assert_eq!(value, slim);
            break;
        }
    }
    workspace.marker("quota-rich");
    let rich = agent.quotas().await.unwrap();
    assert_eq!(rich["plan"], "max");
    assert_eq!(rich["windows"][0]["usedPercent"], 42.5);
    assert_eq!(rich["windows"][1]["usedPercent"], 100.0);
    assert!(rich["windows"][1]["resetsAt"].is_null());
    assert_eq!(rich["resetCredits"]["available"], 2);
    workspace.marker("quota-none");
    assert!(matches!(agent.quotas().await, Err(Error::Unsupported(_))));
    assert_eq!(agent.snapshot().await.state, SessionState::Idle);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn command_updates_replace_catalog_and_emit_native_change() {
    let workspace = Workspace::new();
    let agent = builtin_builder()
        .build()
        .spawn(workspace.options())
        .await
        .unwrap();
    let mut events = agent.events();
    assert_eq!(agent.slash_commands().await.unwrap()[0]["name"], "compact");
    turn(&agent, &mut events, "commands-update").await;
    let commands = agent.slash_commands().await.unwrap();
    assert_eq!(commands.as_array().unwrap().len(), 1);
    assert_eq!(commands[0]["name"], "fixture-command");
    assert_eq!(commands[0]["description"], "Updated native command");
    let journal = agent.replay(0).await.unwrap();
    assert!(
        journal.iter().any(
            |event| matches!(&event.kind,EventKind::CommandsUpdated(value) if value==&commands)
        )
    );
    agent.close().await.unwrap();
}

async fn request(agent: &Agent, events: &mut EventStream, text: &str) -> PermissionRequest {
    agent.prompt(text).await.unwrap();
    loop {
        if let EventKind::PermissionRequested(request) = next(events).await.kind {
            return request;
        }
    }
}
#[tokio::test]
async fn question_cancel_waits_for_native_terminal_and_deny_reason_survives() {
    let workspace = Workspace::new();
    let agent = builtin_builder()
        .build()
        .spawn(workspace.options())
        .await
        .unwrap();
    let mut events = agent.events();
    let permission = request(&agent, &mut events, "ask-permission").await;
    let start = Instant::now();
    agent
        .respond_permission(PermissionResponse {
            answers: Default::default(),
            id: permission.id,
            option: "cancel".into(),
            data: Value::Null,
        })
        .await
        .unwrap();
    assert!(
        agent.snapshot().await.turn.is_some(),
        "wire delivery is not a native terminal receipt"
    );
    loop {
        if let EventKind::TurnEnded { outcome, .. } = next(&mut events).await.kind {
            assert_eq!(outcome, TurnOutcome::Cancelled);
            break;
        }
    }
    assert!(start.elapsed() >= Duration::from_millis(100));
    let body: Value =
        serde_json::from_slice(&std::fs::read(workspace.0.join("permission-answer.json")).unwrap())
            .unwrap();
    assert_eq!(body["interrupt"], true);
    for field in ["message", "reason"] {
        let permission = request(&agent, &mut events, "ask-tool").await;
        agent
            .respond_permission(PermissionResponse {
                answers: Default::default(),
                id: permission.id,
                option: "deny".into(),
                data: json!({field:"Please use a read-only command."}),
            })
            .await
            .unwrap();
        loop {
            if matches!(next(&mut events).await.kind, EventKind::TurnEnded { .. }) {
                break;
            }
        }
        let body: Value = serde_json::from_slice(
            &std::fs::read(workspace.0.join("permission-answer.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(body["message"], "Please use a read-only command.");
        assert!(body.get("interrupt").is_none());
    }
    agent.close().await.unwrap();
}

#[tokio::test]
async fn plan_approval_is_explicit_and_background_tool_keeps_identity_after_parent_end() {
    let workspace = Workspace::new();
    let agent = builtin_builder()
        .build()
        .spawn(workspace.options())
        .await
        .unwrap();
    let mut events = agent.events();
    let permission = request(&agent, &mut events, "ask-plan").await;
    assert_eq!(permission.kind, "plan");
    assert_eq!(
        permission.metadata["plan"]["markdown"],
        "Implement the fixture plan."
    );
    assert!(agent.replay(0).await.unwrap().iter().any(
        |event| matches!(&event.kind,EventKind::PlanUpdated(value) if value.status.as_deref()==Some("proposed"))
    ));
    agent
        .respond_permission(PermissionResponse {
            answers: Default::default(),
            id: permission.id,
            option: "approve".into(),
            data: Value::Null,
        })
        .await
        .unwrap();
    loop {
        if matches!(next(&mut events).await.kind, EventKind::TurnEnded { .. }) {
            break;
        }
    }
    agent.prompt("background-tools").await.unwrap();
    let mut parent_ended = false;
    let mut progress = false;
    loop {
        match next(&mut events).await.kind {
            EventKind::ToolUpdated(call) if call.id == "background-bash" => {
                assert_eq!(call.name.as_deref(), Some("Bash"));
                assert_eq!(call.input["command"], "sleep 1");
                if call.metadata["progress"]["elapsedMs"] == 1250.0 {
                    progress = true;
                }
            }
            EventKind::TurnEnded { .. } => parent_ended = true,
            EventKind::ToolCompleted(call) if call.id == "background-bash" => {
                assert!(parent_ended && progress);
                assert_eq!(call.status, "failed");
                assert_eq!(call.name.as_deref(), Some("Bash"));
                assert_eq!(call.input["command"], "sleep 1");
                assert_eq!(call.output, "Native background process failed");
                break;
            }
            _ => (),
        }
    }
    let before = agent.snapshot().await.turns_completed;
    turn(&agent, &mut events, "denied-tool").await;
    let calls: Vec<_> = agent
        .replay(0)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::ToolCompleted(call) if call.id == "denied-bash" => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].status, "denied");
    assert_eq!(agent.snapshot().await.turns_completed, before + 1);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn imported_native_history_keeps_its_rollback_guard_when_reconnected() {
    let workspace = Workspace::new();
    let runtime = builtin_builder().build();
    let mut options = workspace.options();
    options.config["nativeHistory"] =
        json!({"native":{"provider":"claude","id":"imported-session"}});
    let agent = runtime.spawn(options).await.unwrap();
    let mut events = agent.events();
    turn(&agent, &mut events, "create native saved reference").await;
    let snapshot = agent.snapshot().await;
    assert_eq!(
        snapshot.capabilities.metadata["rollbackHistorySupported"],
        false
    );
    agent.close().await.unwrap();
    let resumed = runtime.resume(&snapshot.id).await.unwrap();
    assert_eq!(
        resumed.snapshot().await.capabilities.metadata["rollbackHistorySupported"],
        false
    );
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn active_interrupt_correlates_an_early_error_terminal_without_overriding_success() {
    for (prompt, expected) in [
        ("active-native-cancel", TurnOutcome::Cancelled),
        ("successful-cancel-race", TurnOutcome::Completed),
    ] {
        let workspace = Workspace::new();
        let agent = builtin_builder()
            .build()
            .spawn(workspace.options())
            .await
            .unwrap();
        let mut events = agent.events();
        agent.prompt(prompt).await.unwrap();
        agent.cancel().await.unwrap();
        loop {
            let event = next(&mut events).await;
            assert!(!matches!(event.kind, EventKind::Error { .. }));
            if let EventKind::TurnEnded { outcome, .. } = event.kind {
                assert_eq!(outcome, expected);
                break;
            }
        }
        agent.close().await.unwrap();
    }
}
