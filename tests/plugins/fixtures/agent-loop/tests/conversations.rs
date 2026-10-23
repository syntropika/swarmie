//! Multiplexed sessions remain isolated through source pagination and physical process death.

use serde_json::json;
use std::{path::Path, time::Duration};
use swarmie_agent_loop_fixture::{model::Reply, tool::ToolConfiguration};
use swarmie_session_contracts::{AppendEvents, CreateSession, EventQuery, SessionLookup};
use swarmie_task_contracts::{BlockingReason, TaskOutcome, TaskSpec, TaskStatus};
use tembra::Result;

pub mod conversation_support;
pub mod support;
use conversation_support::*;
use support::*;

#[test]
fn interleaved_conversations_keep_repeated_turn_ids_and_paged_context_separate() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("answer alpha"), Reply::text("answer beta")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .sessions
            .create(CreateSession {
                session_id: "shared".into(),
                metadata: json!({}),
            })
            .await?;
        let entries = (0..300)
            .map(|index| {
                let conversation = if index % 2 == 0 { "alpha" } else { "beta" };
                event(
                    conversation,
                    &format!("input-{index}"),
                    &format!("{conversation} message {index}"),
                )
            })
            .collect::<Vec<_>>();
        for (index, batch) in entries.chunks(128).enumerate() {
            fixture
                .sessions
                .append(AppendEvents {
                    session_id: "shared".into(),
                    batch_id: format!("page-{index}"),
                    expected_head: None,
                    events: batch.to_vec(),
                })
                .await?;
        }
        let alpha = fixture
            .agent
            .start(request("shared", "alpha", "same-turn", "input-298"))
            .await?;
        settle(&fixture.tasks, &alpha).await?;
        let beta = fixture
            .agent
            .start(request("shared", "beta", "same-turn", "input-299"))
            .await?;
        settle(&fixture.tasks, &beta).await?;
        assert_ne!(alpha.task.id, beta.task.id);
        let calls = fixture.model.requests.borrow().clone();
        assert_eq!(calls.len(), 2);
        for (call, expected, forbidden) in
            [(&calls[0], "alpha", "beta"), (&calls[1], "beta", "alpha")]
        {
            let messages = texts(&call.messages);
            assert_eq!(messages.len(), 150);
            assert!(messages.iter().all(|text| text.starts_with(expected)));
            assert!(!messages.iter().any(|text| text.contains(forbidden)));
        }
        drop(calls);
        for (conversation, task) in [("alpha", alpha.task.id), ("beta", beta.task.id)] {
            let record = fixture
                .agent
                .inspect(lookup("shared", conversation, "same-turn"))
                .await?
                .unwrap();
            assert_eq!(record.task.id, task);
            assert_eq!(record.conversation_id, conversation);
            assert_eq!(record.task.checkpoint["conversation_id"], conversation);
            assert_eq!(record.task.checkpoint["version"], 3);
            assert!(matches!(
                record.task.outcome,
                Some(TaskOutcome::Completed { .. })
            ));
            let mut query = EventQuery::new("shared");
            query.conversation_id = Some(conversation.into());
            query.turn_id = Some("same-turn".into());
            let page = fixture.sessions.events(query).await?;
            assert!(!page.events.is_empty());
            assert!(
                page.events
                    .iter()
                    .all(|entry| entry.event.correlation.conversation_id.as_deref()
                        == Some(conversation))
            );
        }
        assert!(
            fixture
                .agent
                .start(request("shared", "alpha", "wrong-input", "input-299"))
                .await
                .is_err()
        );
        assert!(
            fixture
                .agent
                .inspect(lookup("shared", "missing", "same-turn"))
                .await?
                .is_none()
        );
        fixture.host.close().await
    });
}

fn child(root: &Path, mode: &str) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "conversation_crash_child", "--nocapture"])
        .env("SWARMIE_CONVERSATION_ROOT", root)
        .env("SWARMIE_CONVERSATION_MODE", mode)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
fn real_crash_recovers_persisted_conversation_without_current_caller_context() {
    let directory = tempfile::tempdir().unwrap();
    let log = directory.path().join("conversation-effects.log");
    let mut crashed = child(directory.path(), "crash");
    let start = std::time::Instant::now();
    while !std::fs::read_to_string(&log).is_ok_and(|text| !text.is_empty()) {
        if let Some(status) = crashed.try_wait().unwrap() {
            panic!("child exited before tool effect: {status}");
        }
        if start.elapsed() > Duration::from_secs(15) {
            crashed.kill().unwrap();
            panic!("child did not reach tool effect");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    crashed.kill().unwrap();
    assert!(!crashed.wait().unwrap().success());
    let resumed = child(directory.path(), "resume")
        .wait_with_output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let effects = std::fs::read_to_string(log).unwrap();
    let ids = effects.lines().collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
}

#[test]
fn conversation_crash_child() -> Result<()> {
    let Ok(mode) = std::env::var("SWARMIE_CONVERSATION_MODE") else {
        return Ok(());
    };
    let root = std::path::PathBuf::from(std::env::var_os("SWARMIE_CONVERSATION_ROOT").unwrap());
    run(|driver| async move {
        let crashing = mode == "crash";
        let fixture = open(
            driver,
            &root,
            if crashing {
                vec![Reply::double(21)]
            } else {
                vec![Reply::text("recovered alpha")]
            },
            ToolConfiguration {
                effect_log: Some(root.join("conversation-effects.log")),
                hang_after_effect: crashing,
                ..Default::default()
            },
            true,
        )
        .await?;
        if crashing {
            seed(
                &fixture.sessions,
                "shared",
                "alpha",
                "alpha-input",
                "alpha retained input",
            )
            .await?;
            seed(
                &fixture.sessions,
                "shared",
                "beta",
                "beta-input",
                "beta forbidden input",
            )
            .await?;
            let record = fixture
                .agent
                .start(request("shared", "alpha", "same-turn", "alpha-input"))
                .await?;
            settle(&fixture.tasks, &record).await?;
            panic!("parent must stop this process during tool execution");
        }
        let record = fixture
            .agent
            .inspect(lookup("shared", "alpha", "same-turn"))
            .await?
            .unwrap();
        settle(&fixture.tasks, &record).await?;
        let record = fixture
            .agent
            .inspect(lookup("shared", "alpha", "same-turn"))
            .await?
            .unwrap();
        assert_eq!(record.conversation_id, "alpha");
        assert_eq!(record.task.checkpoint["conversation_id"], "alpha");
        assert!(matches!(
            record.task.outcome,
            Some(TaskOutcome::Completed { .. })
        ));
        let calls = fixture.model.requests.borrow().clone();
        assert_eq!(calls.len(), 1);
        let context = texts(&calls[0].messages);
        assert!(context.contains(&"alpha retained input"));
        assert!(!context.iter().any(|text| text.contains("beta")));
        drop(calls);
        fixture.host.close().await
    });
    Ok(())
}

#[test]
fn unsupported_newer_definition_preserves_current_checkpoint_without_running_effects() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(driver, directory.path(), vec![], Default::default(), true).await?;
        let request = request("session", "conversation", "turn", "input");
        let input = serde_json::to_value(&request).unwrap();
        let checkpoint = json!({
            "version": 3,
            "session_id": request.session_id,
            "conversation_id": request.conversation_id,
            "turn_id": request.turn_id,
            "step": 0,
            "tool_index": 0,
            "tool_calls": 0,
            "output_tokens": 0,
            "started_at_ms": 1,
            "input_sequence": 1,
            "request_event": null,
            "response_event": null,
            "tool_event": null,
            "ending": null,
        });
        let unsupported_version = 99;
        let record = fixture
            .tasks
            .create(TaskSpec {
                id: "unsupported-agent-format".into(),
                kind: swarmie_agent_loop_default::TURN_KIND.into(),
                definition_version: unsupported_version,
                input: input.clone(),
                checkpoint: checkpoint.clone(),
                phase: "start".into(),
            })
            .await?;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let saved = fixture
                    .tasks
                    .get(swarmie_task_contracts::TaskLookup {
                        id: record.id.clone(),
                    })
                    .await?
                    .unwrap();
                if saved.status == TaskStatus::Blocked {
                    assert_eq!(saved.definition_version, unsupported_version);
                    assert_eq!(saved.input, input);
                    assert_eq!(saved.checkpoint, checkpoint);
                    assert_eq!(saved.phase, "start");
                    assert!(matches!(
                        saved.blocking_reason,
                        Some(BlockingReason::IncompatibleDefinition { target_version: 3, detail })
                            if detail.contains("registered definition is older")
                    ));
                    assert!(saved.outcome.is_none());
                    assert!(!saved.cancellation_requested);
                    return Ok::<_, tembra::RuntimeError>(());
                }
                assert!(!saved.is_terminal());
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("unsupported formats are refused without rewriting task data")?;
        assert!(fixture.model.requests.borrow().is_empty());
        assert!(fixture.tool.calls.borrow().is_empty());
        assert!(
            fixture
                .sessions
                .open(SessionLookup {
                    session_id: request.session_id
                })
                .await?
                .is_none()
        );
        fixture.host.close().await
    });
}
