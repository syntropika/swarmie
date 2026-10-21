//! Actual Tembra observers cannot hold confirmed operation processing hostage.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use futures::FutureExt;
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply};
use swarmie_task_contracts::TaskOutcome;
use tembra::{RuntimeError, core};
use tembra_hooks::HookArgs;

pub mod support;
use support::*;

#[test]
fn every_failing_observer_is_best_effort_after_confirmed_results() {
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
        macro_rules! fail {
            ($definition:ident, $payload:ty, $key:literal) => {
                fixture
                    .host
                    .install(
                        hooks::contribution(
                            $definition()?,
                            $key,
                            0,
                            |_: HookArgs<$payload, ()>, _| {
                                async {
                                    Err(RuntimeError::invalid(
                                        "fixture_observation_failure",
                                        "private telemetry detail",
                                    ))
                                }
                                .boxed_local()
                            },
                        )?,
                        core::LoadOptions::default(),
                    )?
                    .await?;
            };
        }
        fail!(on_turn_start, TurnContext, "start-fails");
        fail!(on_model_event, ModelStreamObservation, "stream-fails");
        fail!(after_model_call, ModelObservation, "model-fails");
        fail!(after_tool_call, ToolObservation, "tool-fails");
        fail!(on_step_end, StepEnd, "step-fails");
        fail!(on_turn_end, TurnEnd, "end-fails");
        fixture.start("observers-fail", Default::default()).await?;
        let terminal = fixture.terminal("observers-fail").await?;
        assert!(
            matches!(terminal.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{terminal:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 2);
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        let monitor = fixture
            .host
            .get(AgentObservations, AgentObservationsInput {})
            .await?;
        let diagnostics = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let diagnostics = monitor.diagnostics().await?;
                if diagnostics.failed >= 9 && !diagnostics.active && diagnostics.queued == 0 {
                    break Ok::<_, tembra::RuntimeError>(diagnostics);
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("all admitted observations settle")?;
        assert_eq!(
            diagnostics.last_error_reason.as_deref(),
            Some("fixture_observation_failure")
        );
        assert!(
            !serde_json::to_string(&diagnostics)
                .unwrap()
                .contains("private telemetry detail")
        );
        let events = fixture.events("observers-fail").await?;
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry.event.kind == "swarmie.agent.tool-confirmed")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
                .count(),
            2
        );
        assert!(
            !events
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.phase-error")
        );
        fixture.host.close().await
    });
}

#[test]
fn slow_observer_does_not_block_turn_or_following_notifications_after_settlement() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("answer")],
            Default::default(),
            true,
        )
        .await?;
        let entered = Rc::new(Cell::new(false));
        let settled = Rc::new(Cell::new(false));
        let cancellation_received = Rc::new(Cell::new(false));
        let (release_settlement, settlement) = futures::channel::oneshot::channel::<()>();
        let settlement = Rc::new(RefCell::new(Some(settlement)));
        let received = cancellation_received.clone();
        let (began, ended) = (entered.clone(), settled.clone());
        fixture
            .host
            .install(
                hooks::contribution(
                    on_turn_start()?,
                    "slow-observer",
                    0,
                    move |_: HookArgs<TurnContext, ()>, context| {
                        began.set(true);
                        let ended = ended.clone();
                        let received = received.clone();
                        let settlement = settlement
                            .borrow_mut()
                            .take()
                            .expect("one start observation");
                        async move {
                            let error = context.cancellation().cancelled().await;
                            received.set(true);
                            let _ = settlement.await;
                            ended.set(true);
                            Err(error)
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let last = Rc::new(Cell::new(false));
        let saw_last = last.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    on_turn_end()?,
                    "after-slow-observer",
                    0,
                    move |_: HookArgs<TurnEnd, ()>, _| {
                        saw_last.set(true);
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture.start("slow-observer", Default::default()).await?;
        let terminal = fixture.terminal("slow-observer").await?;
        assert!(matches!(
            terminal.task.outcome,
            Some(TaskOutcome::Completed { .. })
        ));
        assert!(entered.get());
        tokio::time::timeout(Duration::from_secs(3), async {
            while !cancellation_received.get() {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("observer deadline signals cancellation before physical settlement");
        assert!(
            !settled.get(),
            "turn must finish independently of its telemetry handler"
        );
        assert!(!last.get(), "one dispatcher preserves admission order");
        release_settlement
            .send(())
            .expect("release the still-owned physical observer");
        tokio::time::timeout(Duration::from_secs(3), async {
            while !last.get() {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("deadline cancels and joins, then next notification runs");
        assert!(settled.get());
        let monitor = fixture
            .host
            .get(AgentObservations, AgentObservationsInput {})
            .await?;
        assert_eq!(monitor.diagnostics().await?.timed_out, 1);
        fixture.host.close().await
    });
}

#[test]
fn observation_pressure_is_bounded_while_physical_observer_settlement_is_delayed() {
    use swarmie_agent_loop_fixture::gate::{self, AgentGate, AgentGateInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("answer")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(gate::facet()?, core::LoadOptions::default())?
            .await?;
        let gate = fixture.host.get(AgentGate, AgentGateInput {}).await?;
        let held = gate.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    on_turn_start()?,
                    "held-observer",
                    0,
                    move |_: HookArgs<TurnContext, ()>, context| {
                        let held = held.clone();
                        async move {
                            held.with_context(&tembra::Context::from_core(context))
                                .await_gate()
                                .await
                                .map(|_| ())
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        macro_rules! observe {
            ($hook:ident, $payload:ty, $key:literal) => {
                fixture
                    .host
                    .install(
                        hooks::contribution($hook()?, $key, 0, |_: HookArgs<$payload, ()>, _| {
                            async { Ok(()) }.boxed_local()
                        })?,
                        core::LoadOptions::default(),
                    )?
                    .await?;
            };
        }
        observe!(on_model_event, ModelStreamObservation, "queued-stream");
        observe!(after_model_call, ModelObservation, "queued-model");
        observe!(on_step_end, StepEnd, "queued-step");
        observe!(on_turn_end, TurnEnd, "queued-end");
        fixture.model.stream_events.set(100);
        fixture.start("pressure", Default::default()).await?;
        let completed = fixture.terminal("pressure").await?;
        assert!(matches!(
            completed.task.outcome,
            Some(TaskOutcome::Completed { .. })
        ));
        assert!(gate.entered().value()? > 0);
        let monitor = fixture
            .host
            .get(AgentObservations, AgentObservationsInput {})
            .await?;
        let diagnostics = monitor.diagnostics().await?;
        assert!(
            diagnostics.active,
            "entered observer is still physically held"
        );
        assert_eq!(diagnostics.queued, 64);
        assert!(diagnostics.queued_bytes <= 4 * 1024 * 1024);
        assert!(
            diagnostics.dropped > 0,
            "overload is reported instead of blocking turns"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        gate.release_gate().await?;
        fixture.host.close().await
    });
}
