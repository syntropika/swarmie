//! Root-scoped Session access: one current-turn history read per root, and generation admission
//! after the model terminal's started receipt.

use std::{collections::BTreeSet, path::Path};
use swarmie_agent_loop_fixture::model::Reply;
use swarmie_session_contracts::*;
use swarmie_task_contracts::{TaskLookup, TaskOutcome};
use tembra::{core, prelude::*};
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
fn each_phase_root_reads_current_turn_history_at_most_once() {
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
            let expected = if phase == "model" { 2 } else { 1 };
            assert_eq!(
                *count, expected,
                "phase {phase} at revision {revision} read current-turn history {count} times"
            );
            phases.insert(phase.as_str());
        }
        assert!(
            ["model", "tool", "tool-result", "step-end"]
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
