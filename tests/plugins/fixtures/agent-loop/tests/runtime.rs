//! End-to-end Agent Contract tests with real Hosts, native hooks and SQLite Durable Tasks.

use futures::FutureExt;
use serde_json::json;
use std::{cell::RefCell, rc::Rc, time::Duration};
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply, tool::ToolConfiguration};
use swarmie_model_contracts::ContentPart;
use swarmie_session_contracts::{AppendEvents, Correlation, NewEvent};
use swarmie_task_contracts::{TaskOutcome, TaskStatus};
use tembra::core;
use tembra_hooks::HookArgs;
pub mod support;
use support::*;

#[test]
fn simple_turn_streams_and_preserves_session_history_outside_checkpoints() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("The answer is 42.")],
            Default::default(),
            true,
        )
        .await?;
        let streamed = Rc::new(RefCell::new(Vec::new()));
        let observed = streamed.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    on_model_event()?,
                    "stream-observer",
                    0,
                    move |args: HookArgs<ModelStreamObservation, ()>, _| {
                        observed.borrow_mut().push(args.input().event.clone());
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture.start("simple", Default::default()).await?;
        let record = fixture.terminal("simple").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert_eq!(streamed.borrow().len(), 1);
        let events = fixture.events("simple").await?;
        assert!(
            events
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.model-confirmed")
        );
        assert!(
            !record
                .task
                .checkpoint
                .to_string()
                .contains("The answer is 42.")
        );
        assert!(
            !record
                .task
                .checkpoint
                .to_string()
                .contains("Compute the result.")
        );
        fixture.host.close().await
    });
}

#[test]
fn functional_tool_turn_runs_multiple_steps_and_enforces_final_policy() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(21), Reply::text("42")],
            Default::default(),
            true,
        )
        .await?;
        fixture.start("tool", Default::default()).await?;
        let record = fixture.terminal("tool").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 2);
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        assert_eq!(fixture.policy.calls.borrow().len(), 1);
        assert_eq!(
            fixture.tool.calls.borrow()[0].arguments,
            json!({"value":21})
        );
        assert!(fixture.model.requests.borrow()[1].messages.iter().flat_map(|message| &message.content).any(|part| matches!(part, ContentPart::ToolResult { value, .. } if *value == json!({"value":42}))));
        fixture.host.close().await
    });
}

#[test]
fn step_limit_stops_repeated_model_tool_calls() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1), Reply::double(2), Reply::double(3)],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .start(
                "limited",
                TurnLimits {
                    max_steps: 2,
                    ..Default::default()
                },
            )
            .await?;
        let record = fixture.terminal("limited").await?;
        assert!(
            !matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 2);
        assert_eq!(fixture.tool.calls.borrow().len(), 2);
        fixture.host.close().await
    });
}

#[test]
fn tool_errors_are_recorded_without_repeating_the_external_invocation() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1), Reply::text("tool failed")],
            ToolConfiguration {
                fail: true,
                ..Default::default()
            },
            true,
        )
        .await?;
        fixture.start("tool-failed", Default::default()).await?;
        fixture.terminal("tool-failed").await?;
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        let events = fixture.events("tool-failed").await?;
        assert!(
            events
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.tool-confirmed"
                    && entry.event.payload.to_string().contains("true"))
        );
        fixture.host.close().await
    });
}

#[test]
fn no_tool_turn_succeeds_without_tool_or_policy_provider() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open_selected(
            driver,
            directory.path(),
            vec![Reply::text("plain answer")],
            Default::default(),
            None,
            false,
        )
        .await?;
        fixture.start("no-tools", Default::default()).await?;
        let record = fixture.terminal("no-tools").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert!(fixture.model.requests.borrow()[0].tools.is_empty());
        fixture.host.close().await
    });
}

#[test]
fn raw_tool_failure_remains_uncertain_without_a_fabricated_confirmed_result() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1)],
            ToolConfiguration {
                raw_error: true,
                ..Default::default()
            },
            true,
        )
        .await?;
        fixture.start("raw-tool-error", Default::default()).await?;
        loop {
            let record = fixture
                .agent
                .inspect(Harness::lookup("raw-tool-error"))
                .await?
                .unwrap();
            if record.task.status == TaskStatus::Parked {
                break;
            }
            assert!(
                !record.task.is_terminal(),
                "raw effect uncertainty must remain recoverable: {record:?}"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        let events = fixture.events("raw-tool-error").await?;
        assert!(
            !events
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.tool-confirmed")
        );
        fixture.host.close().await
    });
}

#[test]
fn unsafe_uncertainty_continues_only_after_matching_committed_external_evidence() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(21), Reply::text("reconciled 42")],
            ToolConfiguration {
                replay: ReplayPolicy::Unsafe,
                raw_error: true,
                ..Default::default()
            },
            true,
        )
        .await?;
        fixture.start("reconcile", Default::default()).await?;
        let record = loop {
            let record = fixture
                .agent
                .inspect(Harness::lookup("reconcile"))
                .await?
                .unwrap();
            if record.task.status == TaskStatus::Parked {
                break record;
            }
            assert!(!record.task.is_terminal());
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        // A blind retry is not evidence that an unsafe effect did not occur.
        fixture
            .agent
            .continue_turn(ContinueTurn {
                session_id: format!("session-{}", "reconcile"),
                conversation_id: "conversation".into(),
                turn_id: "reconcile".into(),
                resolution: None,
            })
            .await?;
        loop {
            let state = fixture
                .agent
                .inspect(Harness::lookup("reconcile"))
                .await?
                .unwrap();
            if state.task.status == TaskStatus::Parked {
                break;
            }
            assert!(!state.task.is_terminal());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        let call = fixture.tool.calls.borrow()[0].clone();
        let result = ToolResult {
            operation_id: call.operation_id.clone(),
            tool_call_id: call.tool_call_id.clone(),
            output: json!({"value":42}),
            is_error: false,
        };
        fixture
            .sessions
            .append(AppendEvents {
                session_id: "session-reconcile".into(),
                batch_id: "external-evidence".into(),
                expected_head: None,
                events: vec![NewEvent {
                    event_id: "external-result".into(),
                    kind: "swarmie.agent.tool-confirmed".into(),
                    version: 2,
                    correlation: Correlation {
                        conversation_id: Some("conversation".into()),
                        turn_id: Some("reconcile".into()),
                        task_id: Some(record.task.id),
                        step_id: Some("reconcile:0".into()),
                        ..Default::default()
                    },
                    occurred_at_ms: None,
                    payload: json!({"operation_id":call.operation_id,"result":result}),
                }],
            })
            .await?;
        fixture
            .agent
            .continue_turn(ContinueTurn {
                session_id: format!("session-{}", "reconcile"),
                conversation_id: "conversation".into(),
                turn_id: "reconcile".into(),
                resolution: Some(KnownResult {
                    operation_id: call.operation_id,
                    result_event_id: "external-result".into(),
                }),
            })
            .await?;
        let completed = fixture.terminal("reconcile").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{completed:?}"
        );
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        fixture.host.close().await
    });
}

#[test]
fn maximal_turn_identity_keeps_generated_session_correlation_within_limits() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("bounded identity")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .sessions
            .create(swarmie_session_contracts::CreateSession {
                session_id: "long-turn-session".into(),
                metadata: json!({}),
            })
            .await?;
        fixture.sessions.append(AppendEvents {
            session_id:"long-turn-session".into(),batch_id:"long-turn-input".into(),expected_head:None,
            events:vec![NewEvent {event_id:"long-turn-input".into(),kind:swarmie_session_contracts::MESSAGE.into(),version:1,correlation:Correlation { conversation_id: Some("conversation".into()), ..Default::default() },occurred_at_ms:None,payload:json!({"role":"user","content":"Hello","reply_to":null,"tool_call_id":null})}],
        }).await?;
        let turn_id = "t".repeat(256);
        let turn = fixture
            .agent
            .start(StartTurn {
                turn_id: turn_id.clone(),
                session_id: "long-turn-session".into(),
                conversation_id: "conversation".into(),
                input_event_id: "long-turn-input".into(),
                model: swarmie_model_contracts::ModelIdentity {
                    connection: "fixture".into(),
                    model: "scripted".into(),
                },
                limits: Default::default(),
            })
            .await?;
        let outcome = fixture
            .tasks
            .wait(swarmie_task_contracts::TaskLookup { id: turn.task.id })
            .await?;
        assert!(matches!(outcome, TaskOutcome::Completed { .. }));
        let events = fixture
            .sessions
            .events(swarmie_session_contracts::EventQuery::new(
                "long-turn-session",
            ))
            .await?;
        for event in events.events {
            if let Some(step) = event.event.correlation.step_id {
                assert!(step.len() <= 256);
            }
        }
        fixture.host.close().await
    });
}
