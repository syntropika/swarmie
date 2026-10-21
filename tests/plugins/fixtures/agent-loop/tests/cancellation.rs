//! Cancellation authority, physical settlement and independent execution limits.

use futures::FutureExt;
use std::{rc::Rc, time::Duration};
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply, tool::ToolConfiguration};
use swarmie_task_contracts::{TaskOutcome, TaskStatus};
use tembra::core;
use tembra_hooks::HookArgs;
pub mod support;
use support::*;

#[test]
fn cancellation_while_model_is_running_records_aborted_task() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::Hang],
            Default::default(),
            true,
        )
        .await?;
        fixture.start("cancelled", Default::default()).await?;
        while fixture.model.requests.borrow().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        fixture
            .agent
            .cancel(TurnLookup {
                turn_id: "cancelled".into(),
            })
            .await?;
        let record = fixture.terminal("cancelled").await?;
        assert_eq!(record.task.status, TaskStatus::Terminal);
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Aborted { .. })),
            "{record:?}"
        );
        assert!(fixture.tool.calls.borrow().is_empty());
        let events = fixture.events("cancelled").await?;
        let request = events
            .iter()
            .find(|entry| entry.event.kind == "swarmie.agent.cancellation-requested")
            .expect("logical cancellation request");
        let settled = events
            .iter()
            .find(|entry| entry.event.kind == "swarmie.agent.task-settled")
            .expect("authoritative terminal outcome reconciliation");
        assert!(request.sequence < settled.sequence);
        let outcome: TaskOutcome = serde_json::from_value(settled.event.payload.clone()).unwrap();
        assert!(matches!(outcome, TaskOutcome::Aborted { .. }));
        fixture.host.close().await
    });
}

#[test]
fn cancellation_of_a_running_context_hook_settles_without_model_admission() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("unused")],
            Default::default(),
            true,
        )
        .await?;
        let entered = Rc::new(std::cell::Cell::new(false));
        let observed = entered.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    prepare_context()?,
                    "pending",
                    0,
                    move |_: HookArgs<PreparedContext, PreparedContext>, context| {
                        observed.set(true);
                        async move { Err(context.cancellation().cancelled().await) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture.start("hook-cancel", Default::default()).await?;
        while !entered.get() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        fixture
            .agent
            .cancel(TurnLookup {
                turn_id: "hook-cancel".into(),
            })
            .await?;
        let record = fixture.terminal("hook-cancel").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Aborted { .. })),
            "{record:?}"
        );
        assert!(fixture.model.requests.borrow().is_empty());
        fixture.host.close().await
    });
}

#[test]
fn independent_duration_output_and_tool_call_limits_bound_work() {
    for bound in ["duration", "tokens", "tools"] {
        run(|driver| async move {
            let directory = tempfile::tempdir().unwrap();
            let limits = match bound {
                "duration" => TurnLimits {
                    max_duration_ms: 100,
                    ..Default::default()
                },
                "tokens" => TurnLimits {
                    max_output_tokens: 1,
                    ..Default::default()
                },
                _ => TurnLimits {
                    max_tool_calls: 1,
                    ..Default::default()
                },
            };
            let replies = if bound == "duration" {
                vec![Reply::Hang]
            } else {
                vec![Reply::double(1), Reply::double(2), Reply::text("unused")]
            };
            let fixture = open(driver, directory.path(), replies, Default::default(), true).await?;
            fixture.start(bound, limits).await?;
            let completed = fixture.terminal(bound).await?;
            assert!(
                matches!(completed.task.outcome, Some(TaskOutcome::Failed { .. })),
                "{completed:?}"
            );
            if bound == "tokens" {
                assert!(fixture.tool.calls.borrow().is_empty());
                assert_eq!(fixture.model.requests.borrow().len(), 1);
            }
            if bound == "tools" {
                assert_eq!(fixture.tool.calls.borrow().len(), 1);
                assert!(fixture.model.requests.borrow().len() <= 2);
            }
            fixture.host.close().await
        });
    }
}

#[test]
fn cancellation_does_not_publish_terminal_before_tool_physically_settles() {
    use swarmie_agent_loop_fixture::gate::{self, AgentGate, AgentGateInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1)],
            ToolConfiguration {
                wait_for_cancellation: true,
                ..Default::default()
            },
            true,
        )
        .await?;
        fixture
            .host
            .install(gate::facet()?, core::LoadOptions::default())?
            .await?;
        let gate = fixture.host.get(AgentGate, AgentGateInput {}).await?;
        *fixture.tool.physical_gate.borrow_mut() = Some(gate.clone());
        fixture.start("physical-cancel", Default::default()).await?;
        while fixture.tool.calls.borrow().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        fixture
            .agent
            .cancel(TurnLookup {
                turn_id: "physical-cancel".into(),
            })
            .await?;
        while gate.entered().value()? == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let cancelling = fixture
            .agent
            .inspect(TurnLookup {
                turn_id: "physical-cancel".into(),
            })
            .await?
            .unwrap();
        assert!(cancelling.task.cancellation_requested);
        assert!(
            !cancelling.task.is_terminal(),
            "the retained physical invocation has not settled"
        );
        assert!(!fixture.tool.physically_settled.get());
        gate.release_gate().await?;
        let completed = fixture.terminal("physical-cancel").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Aborted { .. })),
            "{completed:?}"
        );
        assert!(fixture.tool.physically_settled.get());
        fixture.host.close().await
    });
}

#[test]
fn cancellation_during_required_authorization_prevents_late_allowed_tool_effect() {
    use swarmie_agent_loop_fixture::gate::{self, AgentGate, AgentGateInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1)],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(gate::facet()?, core::LoadOptions::default())?
            .await?;
        let gate = fixture.host.get(AgentGate, AgentGateInput {}).await?;
        *fixture.policy.gate.borrow_mut() = Some(gate.clone());
        fixture
            .start("permission-cancel", Default::default())
            .await?;
        while gate.entered().value()? == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        fixture
            .agent
            .cancel(TurnLookup {
                turn_id: "permission-cancel".into(),
            })
            .await?;
        gate.release_gate().await?;
        let completed = fixture.terminal("permission-cancel").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Aborted { .. })),
            "{completed:?}"
        );
        assert!(
            fixture.tool.calls.borrow().is_empty(),
            "an allowed policy result cannot revive cancelled execution authority"
        );
        fixture.host.close().await
    });
}
