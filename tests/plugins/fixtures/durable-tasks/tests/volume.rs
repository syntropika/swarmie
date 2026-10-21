//! Real storage counters qualify cold retained history without timing-based performance claims.

use std::{
    future::Future,
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use swarmie_durable_tasks_fixture::{
    concurrency::{self, StorageControl},
    facet, task,
};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(180), test(driver.clone()))
                .await
                .expect("volume acceptance timed out")
                .unwrap();
        }));
}

async fn seed(storage: &TaskStorageRef) -> Result<()> {
    storage
        .claim_owner(StorageOwner {
            id: "archive-seed".into(),
        })
        .await?;
    for batch in 0..16 {
        let writes = (0..125)
            .map(|index| {
                let mut record =
                    TaskRecord::new(task(&format!("archive-{:04}", batch * 125 + index)), None);
                record.status = TaskStatus::Terminal;
                record.outcome = Some(TaskOutcome::Completed {
                    value: serde_json::json!({"retained":true}),
                });
                TaskWrite {
                    provenance: None,
                    expected_revision: None,
                    record,
                }
            })
            .collect();
        assert!(matches!(
            storage
                .commit(TaskTransaction {
                    owner: "archive-seed".into(),
                    writes
                })
                .await?,
            CommitResult::Committed { .. }
        ));
    }
    let mut sleeper = TaskRecord::new(task("future-sleeper"), None);
    sleeper.status = TaskStatus::Sleeping;
    sleeper.phase = "finish".into();
    sleeper.wake_at_ms = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 3_600_000,
    );
    let mut exhausted = TaskRecord::new(task("exhausted-restart-counter"), None);
    exhausted.status = TaskStatus::Running;
    exhausted.restarts = u32::MAX;
    storage
        .commit(TaskTransaction {
            owner: "archive-seed".into(),
            writes: vec![
                TaskWrite {
                    provenance: None,
                    expected_revision: None,
                    record: sleeper,
                },
                TaskWrite {
                    provenance: None,
                    expected_revision: None,
                    record: exhausted,
                },
            ],
        })
        .await?;
    storage
        .release_owner(StorageOwner {
            id: "archive-seed".into(),
        })
        .await
}

#[test]
fn retained_terminal_roots_stay_cold_during_idle_and_saturated_scheduling() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let backend_host = Host::with_driver("volume-backend", "application", driver.clone())?;
        backend_host
            .install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    database: directory.path().join("tasks.sqlite"),
                    outbox: Default::default(),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        let backend = backend_host.get(TaskStorage, TaskStorageInput {}).await?;
        seed(&backend).await?;
        let host = Host::with_driver("volume-scheduler", "application", driver.clone())?;
        let control = StorageControl::new(backend.clone());
        host.install(
            concurrency::storage_facet(control.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(facet(1)?, core::LoadOptions::default())?
            .await?;
        host.install(
            swarmie_durable_tasks::facet(
                &swarmie_durable_tasks::Configuration {
                    poll_interval_ms: 2,
                    max_parallel: 4,
                    ..Default::default()
                },
                driver,
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
        tokio::time::sleep(Duration::from_millis(60)).await;
        let exhausted = tasks
            .get(TaskLookup {
                id: "exhausted-restart-counter".into(),
            })
            .await?
            .unwrap();
        assert_eq!(exhausted.status, TaskStatus::Parked);
        assert_eq!(exhausted.restarts, u32::MAX);
        assert_eq!(
            exhausted.blocking_reason,
            Some(BlockingReason::RestartLimit)
        );
        assert_eq!(control.unfiltered_scans.get(), 0);
        assert_eq!(control.terminal_root_rows.get(), 0);
        control.reset_counts();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            control.scanned_rows.get(),
            0,
            "idle cycles must not rescan the sleeping payload or retained roots"
        );
        assert!(
            control.scans.get() <= 2,
            "idle scheduler should only need a bounded fallback sequence probe"
        );
        for index in 0..100 {
            tasks.create(task(&format!("live-{index:03}"))).await?;
        }
        for index in 0..100 {
            assert!(matches!(
                tasks
                    .wait(TaskLookup {
                        id: format!("live-{index:03}")
                    })
                    .await?,
                TaskOutcome::Completed { .. }
            ));
        }
        assert_eq!(control.unfiltered_scans.get(), 0);
        assert_eq!(
            control.terminal_root_rows.get(),
            0,
            "phase execution must not decode unrelated completed roots"
        );
        let retained = backend
            .get(TaskLookup {
                id: "archive-1337".into(),
            })
            .await?
            .unwrap();
        assert_eq!(retained.revision, 1);
        assert!(retained.is_terminal());
        control.reset_counts();
        // A legitimate owner-aware backend write bypasses the scheduler's local wake.
        // The bounded filtered fallback must discover it without a public changefeed.
        control.external_create(task("outside-commit")).await?;
        assert!(matches!(
            tasks
                .wait(TaskLookup {
                    id: "outside-commit".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(control.unfiltered_scans.get(), 0);
        assert_eq!(control.terminal_root_rows.get(), 0);
        host.close().await?;
        backend_host.close().await
    });
}
