//! Root-scoped Session access: one current-turn history read per root, and generation admission
//! after the model terminal's started receipt.

use futures::FutureExt;
use std::{collections::BTreeSet, path::Path, time::Duration};
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::hooks;
use swarmie_agent_loop_fixture::model::Reply;
use swarmie_model_contracts::{ContentPart, FinishReason, GenerateResult, Role};
use swarmie_session_contracts::*;
use swarmie_task_contracts::{TaskLookup, TaskOutcome, TaskStatus};
use tembra::{core, prelude::*};
use tembra_hooks::HookArgs;
use tembra_native::TokioDriver;

pub mod session_probe;
pub mod support;
use session_probe::{CancelAfter, Probe};
use support::*;

/// Serves the loop's SessionStore through the probe over an independent SQLite store.
async fn open_probed(
    driver: std::rc::Rc<TokioDriver>,
    root: &Path,
    probe: &Probe,
    replies: Vec<Reply>,
) -> Result<(Harness, Host)> {
    let backend = Host::with_driver("session-backend", "application", driver.clone())?;
    backend
        .install(
            swarmie_session_store_sqlite::facet(
                &swarmie_session_store_sqlite::Configuration {
                    database: root.join("probed-sessions.sqlite"),
                },
                driver.clone(),
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
    let source = backend.get(SessionStore, SessionStoreInput {}).await?;
    let initial = source.publication_status().await?.durable;
    let fixture = open_with_session_provider_replies(
        driver,
        root,
        session_probe::facet(source, initial, probe.clone())?,
        replies,
    )
    .await?;
    *probe.storage.borrow_mut() = Some(fixture.storage.clone());
    Ok((fixture, backend))
}

#[test]
fn folded_roots_refresh_nested_receipts_without_extra_scheduler_handoffs() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let probe = Probe::default();
        let (fixture, backend) = open_probed(
            driver,
            directory.path(),
            &probe,
            vec![Reply::double(1), Reply::text("done")],
        )
        .await?;
        let started = fixture.start("reads", Default::default()).await?;
        // Wait without inspecting, so every counted read belongs to a phase or terminal root.
        fixture
            .tasks
            .wait(TaskLookup {
                id: started.task.id,
            })
            .await?;
        let reads = probe.reads.borrow().clone();
        let mut phases = BTreeSet::new();
        for ((revision, phase), count) in &reads {
            // The model terminal invoked by a model phase is a root of its own.
            // A model phase reads its prefix, its terminal has a separate prefix, and
            // result interpretation refreshes once to include the terminal's actual request.
            let expected = if phase == "model" { 3 } else { 1 };
            assert_eq!(
                *count, expected,
                "phase {phase} at revision {revision} read current-turn history {count} times"
            );
            phases.insert(phase.as_str());
        }
        assert!(
            ["start", "model", "tool"]
                .iter()
                .all(|phase| phases.contains(phase)),
            "{reads:?}"
        );
        assert_eq!(
            reads.keys().filter(|(_, phase)| phase == "model").count(),
            2,
            "each model step is one phase root: {reads:?}"
        );
        let record = fixture.terminal("reads").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        assert_eq!(record.task.attempts, 4, "start, model, tool, model");
        assert_eq!(
            record.task.revision, 6,
            "create, reserve, three continuations, complete"
        );
        assert_eq!(reads.len(), 4, "one owned invocation per real boundary");
        {
            let batches = probe.batches.borrow();
            let effect_fences = batches
                .iter()
                .filter(|batch| {
                    batch
                        .events
                        .iter()
                        .any(|event| event.kind == "swarmie.agent.model-started")
                })
                .collect::<Vec<_>>();
            assert_eq!(effect_fences.len(), 2);
            for batch in effect_fences {
                assert_eq!(
                    batch
                        .events
                        .iter()
                        .map(|event| event.kind.as_str())
                        .collect::<Vec<_>>(),
                    ["swarmie.agent.model-actual", "swarmie.agent.model-started"]
                );
                assert_eq!(batch.batch_id, batch.events[0].event_id);
            }
        }
        probe.release();
        fixture.host.close().await?;
        backend.close().await
    });
}

#[test]
fn cancellation_after_the_started_receipt_never_activates_a_generation() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let probe = Probe::default();
        let (fixture, backend) = open_probed(
            driver,
            directory.path(),
            &probe,
            vec![Reply::text("must not be generated")],
        )
        .await?;
        *probe.cancel_after.borrow_mut() = Some(CancelAfter {
            kind: "swarmie.agent.model-started".into(),
            turn: Harness::lookup("window"),
            agent: fixture.agent.clone(),
        });
        fixture.start("window", Default::default()).await?;
        let record = fixture.terminal("window").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Aborted { .. })),
            "{record:?}"
        );
        assert!(
            matches!(*probe.cancelled.borrow(), Some(Ok(true))),
            "cancellation must commit after the durable started receipt"
        );
        let kinds = fixture
            .events("window")
            .await?
            .into_iter()
            .map(|entry| entry.event.kind)
            .collect::<BTreeSet<_>>();
        assert!(kinds.contains("swarmie.agent.model-actual"), "{kinds:?}");
        assert!(kinds.contains("swarmie.agent.model-started"), "{kinds:?}");
        assert!(
            !kinds.contains("swarmie.agent.model-provider-confirmed")
                && !kinds.contains("swarmie.agent.model-confirmed"),
            "a refused generation admission has no provider receipt: {kinds:?}"
        );
        assert!(fixture.model.requests.borrow().is_empty());
        assert_eq!(
            (
                fixture.model.dropped.get(),
                fixture.model.recovery_calls.get(),
                fixture.model.cancel_calls.get()
            ),
            (0, 0, 0),
            "no generation object was activated, recovered or cancelled"
        );
        probe.release();
        fixture.host.close().await?;
        backend.close().await
    });
}

#[test]
fn nested_actual_request_remains_authoritative_for_unknown_usage_after_folding() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("done")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    wrap_model_call()?,
                    "narrow-actual-cap",
                    0,
                    |args: HookArgs<ModelCall, GenerateResult>, _| {
                        async move {
                            let (mut call, next) = args.into_parts();
                            call.request.options.max_output_tokens = Some(1);
                            let mut result = next.unwrap().call(call).await?;
                            if let GenerateResult::Response { response } = &mut result {
                                response.usage.output_tokens = None;
                            }
                            Ok(result)
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start(
                "actual-cap",
                TurnLimits {
                    max_output_tokens: 10,
                    ..Default::default()
                },
            )
            .await?;
        let record = fixture.terminal("actual-cap").await?;
        let Some(TaskOutcome::Completed { value }) = &record.task.outcome else {
            panic!("{record:?}")
        };
        assert_eq!(record.task.attempts, 2, "start and one model effect");
        assert_eq!(
            record.task.revision, 4,
            "create, reserve, continue, complete"
        );
        assert_eq!(
            value["output_tokens"], 1,
            "unknown usage consumes the effective cap, not the prepared request's cap"
        );
        assert_eq!(
            fixture.model.requests.borrow()[0].options.max_output_tokens,
            Some(1)
        );
        fixture.host.close().await
    });
}

#[test]
fn deadline_during_folded_preparation_retains_completed_model_bookkeeping() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(3)],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    before_tool_call()?,
                    "wait-after-confirmed-model",
                    0,
                    |_: HookArgs<ToolCall, ToolCall>, context| {
                        async move { Err(context.cancellation().cancelled().await) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start(
                "prepare-deadline",
                TurnLimits {
                    max_duration_ms: 2_000,
                    ..Default::default()
                },
            )
            .await?;
        let record = fixture.terminal("prepare-deadline").await?;
        assert!(
            matches!(&record.task.outcome, Some(TaskOutcome::Failed { error }) if error.contains("duration")),
            "{record:?}"
        );
        assert_eq!(record.task.checkpoint["output_tokens"], 1);
        assert_eq!(record.task.checkpoint["tool_calls"], 0);
        let response = record.task.checkpoint["response_event"]
            .as_str()
            .expect("completed model bookkeeping survives its following hook timeout");
        assert!(
            fixture
                .events("prepare-deadline")
                .await?
                .iter()
                .any(|entry| entry.event.event_id == response
                    && entry.event.kind == "swarmie.agent.model-confirmed")
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert!(fixture.tool.calls.borrow().is_empty());
        assert_eq!(fixture.model.premature_drop.get(), 0);
        fixture.host.close().await
    });
}

#[test]
fn confirmed_result_with_lost_reply_replays_bookkeeping_without_resubmitting_model() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let probe = Probe::default();
        probe.repeat_effect_fences.set(true);
        *probe.fail_after.borrow_mut() = Some("swarmie.agent.model-confirmed".into());
        let (fixture, backend) = open_probed(
            driver,
            directory.path(),
            &probe,
            vec![Reply::text("one result")],
        )
        .await?;
        fixture.start("lost-result", Default::default()).await?;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let record = fixture
                    .agent
                    .inspect(Harness::lookup("lost-result"))
                    .await?
                    .unwrap();
                if matches!(record.task.status, TaskStatus::Parked | TaskStatus::Blocked) {
                    return Ok::<_, RuntimeError>(());
                }
                assert!(!record.task.is_terminal(), "{record:?}");
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("the refused reply parks its model root")?;
        fixture
            .agent
            .continue_turn(ContinueTurn {
                session_id: "session-lost-result".into(),
                conversation_id: "conversation".into(),
                turn_id: "lost-result".into(),
                resolution: None,
            })
            .await?;
        let record = fixture.terminal("lost-result").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        let events = fixture.events("lost-result").await?;
        for kind in [
            "swarmie.agent.model-actual",
            "swarmie.agent.model-started",
            "swarmie.agent.model-provider-confirmed",
            "swarmie.agent.model-confirmed",
        ] {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.event.kind == kind)
                    .count(),
                1,
                "{kind} remains unique across lost replies and exact batch retries"
            );
        }
        probe.release();
        fixture.host.close().await?;
        backend.close().await
    });
}

#[test]
fn multiple_tools_keep_individual_effect_checkpoints_and_committed_context() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(3), Reply::text("done")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    wrap_model_call()?,
                    "three-distinct-tools",
                    0,
                    |args: HookArgs<ModelCall, GenerateResult>, _| {
                        async move {
                            let (call, next) = args.into_parts();
                            let mut result = next.unwrap().call(call).await?;
                            if let GenerateResult::Response { response } = &mut result
                                && response.finish_reason == FinishReason::ToolCalls
                            {
                                let ContentPart::ToolCall { call } =
                                    response.message.content[0].clone()
                                else {
                                    unreachable!()
                                };
                                for index in 1..3 {
                                    let mut extra = call.clone();
                                    extra.id = format!("extra-{index}");
                                    response
                                        .message
                                        .content
                                        .push(ContentPart::ToolCall { call: extra });
                                }
                            }
                            Ok(result)
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start(
                "three-tools",
                TurnLimits {
                    max_tool_calls: 3,
                    max_steps: 2,
                    ..Default::default()
                },
            )
            .await?;
        let record = fixture.terminal("three-tools").await?;
        let Some(TaskOutcome::Completed { value }) = &record.task.outcome else {
            panic!("{record:?}")
        };
        assert_eq!(value["tool_calls"], 3);
        assert_eq!(value["steps"], 2);
        assert_eq!(record.task.attempts, 6, "start, model, three tools, model");
        assert_eq!(record.task.revision, 8);
        {
            let calls = fixture.tool.calls.borrow();
            assert_eq!(calls.len(), 3);
            assert_eq!(
                calls
                    .iter()
                    .map(|call| &call.operation_id)
                    .collect::<BTreeSet<_>>()
                    .len(),
                3
            );
        }
        {
            let requests = fixture.model.requests.borrow();
            assert_eq!(requests.len(), 2);
            assert_eq!(
                requests[1]
                    .messages
                    .iter()
                    .filter(|message| message.role == Role::Tool)
                    .count(),
                3,
                "the next model sees every committed result"
            );
        }
        fixture.host.close().await
    });
}

#[test]
fn committed_turn_decision_survives_a_lost_reply_after_deadline() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let probe = Probe::default();
        *probe.fail_after.borrow_mut() = Some("swarmie.agent.turn-ended".into());
        let (fixture, backend) = open_probed(
            driver,
            directory.path(),
            &probe,
            vec![Reply::text("completed before deadline")],
        )
        .await?;
        fixture
            .start(
                "lost-completion",
                TurnLimits {
                    max_duration_ms: 2_000,
                    ..Default::default()
                },
            )
            .await?;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let record = fixture
                    .agent
                    .inspect(Harness::lookup("lost-completion"))
                    .await?
                    .unwrap();
                if record.task.status == TaskStatus::Parked {
                    return Ok::<_, RuntimeError>(());
                }
                assert!(!record.task.is_terminal(), "{record:?}");
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("the lost reply parks the uncommitted task decision")?;
        let events = fixture.events("lost-completion").await?;
        let decision = events
            .iter()
            .find(|event| event.event.kind == "swarmie.agent.turn-ended")
            .expect("terminal Session evidence committed before the reply was lost")
            .event
            .payload
            .clone();
        assert_eq!(decision["type"], "completed");
        tokio::time::sleep(Duration::from_millis(2_100)).await;
        fixture
            .agent
            .continue_turn(ContinueTurn {
                session_id: "session-lost-completion".into(),
                conversation_id: "conversation".into(),
                turn_id: "lost-completion".into(),
                resolution: None,
            })
            .await?;
        let record = fixture.terminal("lost-completion").await?;
        assert_eq!(
            serde_json::to_value(&record.task.outcome).unwrap(),
            decision,
            "an elapsed deadline cannot replace an already committed terminal decision"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert_eq!(
            fixture
                .events("lost-completion")
                .await?
                .iter()
                .filter(|event| event.event.kind == "swarmie.agent.turn-ended")
                .count(),
            1
        );
        probe.release();
        fixture.host.close().await?;
        backend.close().await
    });
}

#[test]
fn committed_turn_decision_survives_its_inflight_reply_timeout() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let probe = Probe::default();
        *probe.pause_after.borrow_mut() = Some("swarmie.agent.turn-ended".into());
        let (fixture, backend) = open_probed(
            driver,
            directory.path(),
            &probe,
            vec![Reply::text("committed final answer")],
        )
        .await?;
        fixture
            .start(
                "completion-timeout",
                TurnLimits {
                    max_duration_ms: 2_000,
                    ..Default::default()
                },
            )
            .await?;
        let record = fixture.terminal("completion-timeout").await?;
        assert!(
            matches!(&record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        let events = fixture.events("completion-timeout").await?;
        let decisions = events
            .iter()
            .filter(|event| event.event.kind == "swarmie.agent.turn-ended")
            .collect::<Vec<_>>();
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].event.payload,
            serde_json::to_value(&record.task.outcome).unwrap()
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert_eq!(fixture.model.premature_drop.get(), 0);
        probe.release();
        fixture.host.close().await?;
        backend.close().await
    });
}
