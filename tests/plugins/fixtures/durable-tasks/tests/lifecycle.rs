//! Actual Hosts, Facets, public Contracts and durable records exercise application behavior.

use std::{
    future::Future,
    path::Path,
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use swarmie_durable_tasks_fixture::{facet, task};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

#[tembra::contract(local, name = "swarmie.acceptance-duplicate-workflow", version = 1)]
trait DuplicateWorkflow {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

struct Duplicate;

impl DuplicateWorkflowProvider for Duplicate {
    async fn step(&self, _invocation: TaskInvocation, _context: Context) -> Result<TaskTransition> {
        Ok(TaskTransition::Complete {
            value: json!("ambiguous provider"),
        })
    }
}

fn duplicate_definition() -> Result<NativeFacet> {
    NativeFacet::new("fixture.duplicate-workflow", vec![], |_, setup| {
        setup.serve_with_options(
            DuplicateWorkflow,
            |_, _, declaration| {
                declaration.core_mut().register_method(
                    &task_definitions()?,
                    "fixture.workflow",
                    json!({"kind":"fixture.workflow","version":1}),
                    "step",
                )?;
                Ok(Duplicate)
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

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
            tokio::time::timeout(Duration::from_secs(20), test(driver.clone()))
                .await
                .expect("durable task acceptance timed out")
                .unwrap();
        }));
}

async fn open(
    driver: Rc<TokioDriver>,
    database: &Path,
    definition: Option<u32>,
) -> Result<(Host, DurableTasksRef, TaskStorageRef)> {
    let host = Host::with_driver("durable-acceptance", "application", driver.clone())?;
    host.install(
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            database: database.into(),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    if let Some(version) = definition {
        host.install(facet(version)?, core::LoadOptions::default())?
            .await?;
    }
    host.install(
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                poll_interval_ms: 2,
                ..Default::default()
            },
            driver,
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
    Ok((host, tasks, storage))
}

async fn status(tasks: &DurableTasksRef, id: &str, expected: TaskStatus) -> Result<TaskRecord> {
    loop {
        let record = tasks
            .get(TaskLookup { id: id.into() })
            .await?
            .expect("committed task");
        if record.status == expected {
            return Ok(record);
        }
        if record.is_terminal() {
            return Err(RuntimeError::invalid(
                "fixture_unexpected_terminal",
                format!("expected {expected:?}, got {record:?}"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[test]
fn phases_checkpoints_results_and_committed_graph_use_public_contracts() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, tasks, storage) =
            open(driver, &directory.path().join("tasks.sqlite"), Some(1)).await?;
        let created = tasks.create(task("publish")).await?;
        assert_eq!(created.revision, 1);
        assert_eq!(
            tasks
                .wait(TaskLookup {
                    id: "publish".into()
                })
                .await?,
            TaskOutcome::Completed {
                value: json!({"published":true,"version":1})
            }
        );
        let graph = tasks.graph(TaskQuery::default()).await?;
        let durable = storage.scan(TaskQuery::default()).await?;
        assert_eq!(graph, durable);
        assert_eq!(tasks.committed_sequence().value()?, graph.sequence);
        let observed = Rc::new(std::cell::Cell::new(0));
        let snapshot = observed.clone();
        let _subscription = tasks.committed_sequence().subscribe(move |value| {
            snapshot.set(value.value()?);
            Ok(())
        })?;
        assert_eq!(observed.get(), graph.sequence);
        assert_eq!(graph.records.len(), 1);
        let record = &graph.records[0];
        assert_eq!(record.checkpoint, json!({"operation_id":"publish:publish"}));
        assert!(record.attempts >= 3);
        assert_eq!(record.status, TaskStatus::Terminal);
        host.close().await
    });
}

#[test]
fn duplicate_task_kind_is_refused_by_the_actual_registry() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, tasks, _) =
            open(driver, &directory.path().join("tasks.sqlite"), Some(1)).await?;
        let result = host
            .install(duplicate_definition()?, core::LoadOptions::default())?
            .await;
        assert!(
            result.is_err(),
            "two handlers must never silently compete for one task kind"
        );
        let table = host.core().registry(&task_definitions()?)?.table(None)?;
        assert_eq!(table.entries().len(), 1);
        tasks.create(task("unambiguous")).await?;
        assert_eq!(
            tasks
                .wait(TaskLookup {
                    id: "unambiguous".into()
                })
                .await?,
            TaskOutcome::Completed {
                value: json!({"published":true,"version":1})
            }
        );
        host.close().await
    });
}

#[test]
fn owned_children_wait_all_settled_and_cancellation_propagates() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, tasks, _) =
            open(driver, &directory.path().join("tasks.sqlite"), Some(1)).await?;
        let mut parent = task("parent");
        parent.phase = "spawn".into();
        tasks.create(parent).await?;
        assert!(matches!(
            tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        let graph = tasks.graph(TaskQuery::default()).await?;
        assert_eq!(graph.records.len(), 3);
        assert!(graph.records.iter().all(TaskRecord::is_terminal));
        assert_eq!(
            graph
                .records
                .iter()
                .filter(|record| record.parent.as_deref() == Some("parent"))
                .count(),
            2
        );

        let mut cancelled = task("cancel-parent");
        cancelled.phase = "spawn".into();
        cancelled.input = json!({"children_sleep":true});
        tasks.create(cancelled).await?;
        let waiting = status(&tasks, "cancel-parent", TaskStatus::Waiting).await?;
        assert_eq!(waiting.wait_for.len(), 2);
        assert_eq!(waiting.checkpoint, json!({"children_created":true}));
        // A consistent graph can never observe a committed parent without its atomic children.
        let graph = tasks.graph(TaskQuery::default()).await?;
        for child in &waiting.wait_for {
            assert!(graph.records.iter().any(|record| &record.id == child));
        }
        tasks
            .cancel(TaskLookup {
                id: "cancel-parent".into(),
            })
            .await?;
        assert!(matches!(
            tasks
                .wait(TaskLookup {
                    id: "cancel-parent".into()
                })
                .await?,
            TaskOutcome::Aborted { .. }
        ));
        for child in waiting.wait_for {
            let child = status(&tasks, &child, TaskStatus::Terminal).await?;
            assert!(child.cancellation_requested);
            assert!(matches!(child.outcome, Some(TaskOutcome::Aborted { .. })));
        }
        host.close().await
    });
}

#[test]
fn missing_definitions_block_without_loss_then_registration_resumes() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, tasks, storage) =
            open(driver, &directory.path().join("tasks.sqlite"), None).await?;
        let original = task("late-definition");
        tasks.create(original.clone()).await?;
        let blocked = status(&tasks, "late-definition", TaskStatus::Blocked).await?;
        assert_eq!(
            blocked.blocking_reason,
            Some(BlockingReason::MissingDefinition)
        );
        assert_eq!(blocked.checkpoint, original.checkpoint);
        assert_eq!(
            storage
                .get(TaskLookup {
                    id: original.id.clone()
                })
                .await?
                .unwrap(),
            blocked
        );
        host.install(facet(1)?, core::LoadOptions::default())?
            .await?;
        assert!(matches!(
            tasks.wait(TaskLookup { id: original.id }).await?,
            TaskOutcome::Completed { .. }
        ));
        host.close().await
    });
}

#[test]
fn incompatible_checkpoint_migration_is_inspectably_blocked() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, tasks, _) =
            open(driver, &directory.path().join("tasks.sqlite"), Some(2)).await?;
        let mut original = task("old-checkpoint");
        original.input = json!({"reject_migration":true});
        original.checkpoint = json!({"old_format":"preserve me"});
        tasks.create(original.clone()).await?;
        let blocked = status(&tasks, &original.id, TaskStatus::Blocked).await?;
        assert!(matches!(
            blocked.blocking_reason,
            Some(BlockingReason::MigrationRejected { .. })
        ));
        assert_eq!(blocked.checkpoint, original.checkpoint);
        assert_eq!(blocked.definition_version, 1);
        host.close().await
    });
}

#[test]
fn host_replacement_preserves_sleeping_work_and_releases_storage_claim() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let (host, tasks, storage) = open(driver.clone(), &database, Some(1)).await?;
        let mut sleeper = task("sleeper");
        sleeper.phase = "sleep".into();
        sleeper.input = json!({"wake_at_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + 500});
        tasks.create(sleeper).await?;
        let sleeping = status(&tasks, "sleeper", TaskStatus::Sleeping).await?;
        host.close().await?;
        assert!(
            tasks
                .get(TaskLookup {
                    id: "sleeper".into()
                })
                .await
                .is_err()
        );
        assert!(
            storage
                .get(TaskLookup {
                    id: "sleeper".into()
                })
                .await
                .is_err()
        );
        let inspection = Host::with_driver("inspection", "application", driver.clone())?;
        inspection
            .install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    database: database.clone(),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        let persisted = inspection.get(TaskStorage, TaskStorageInput {}).await?;
        assert_eq!(
            persisted
                .get(TaskLookup {
                    id: "sleeper".into()
                })
                .await?
                .unwrap(),
            sleeping
        );
        inspection.close().await?;
        let (next, resumed, _) = open(driver, &database, Some(1)).await?;
        assert!(matches!(
            resumed
                .wait(TaskLookup {
                    id: "sleeper".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        next.close().await
    });
}

#[test]
fn fail_fast_settles_failed_child_and_cancels_remaining_owned_work() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, tasks, _) =
            open(driver, &directory.path().join("tasks.sqlite"), Some(1)).await?;
        let mut parent = task("fail-fast");
        parent.phase = "spawn".into();
        parent.input = json!({"fail_fast":true});
        tasks.create(parent).await?;
        let outcome = tasks
            .wait(TaskLookup {
                id: "fail-fast".into(),
            })
            .await?;
        assert!(matches!(outcome, TaskOutcome::Failed { .. }));
        let graph = tasks.graph(TaskQuery::default()).await?;
        assert!(graph.records.iter().all(TaskRecord::is_terminal));
        assert!(matches!(
            graph
                .records
                .iter()
                .find(|record| record.id == "fail-fast:second")
                .unwrap()
                .outcome,
            Some(TaskOutcome::Aborted { .. })
        ));
        host.close().await
    });
}

#[test]
fn completing_outcome_waits_for_recovered_owned_work_without_reinvoking_parent() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let seed = Host::with_driver("seed-committed-recovery", "application", driver.clone())?;
        seed.install(
            swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                database: database.clone(),
            })?,
            core::LoadOptions::default(),
        )?
        .await?;
        let storage = seed.get(TaskStorage, TaskStorageInput {}).await?;
        storage
            .claim_owner(StorageOwner {
                id: "recovery-fixture".into(),
            })
            .await?;
        let mut parent = TaskRecord::new(task("recover-parent"), None);
        parent.phase = "finish".into();
        let mut child = TaskRecord::new(task("recover-child"), Some(parent.id.clone()));
        child.phase = "finish".into();
        child.status = TaskStatus::Sleeping;
        child.wake_at_ms = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                + 2_000,
        );
        assert!(matches!(
            storage
                .commit(TaskTransaction {
                    owner: "recovery-fixture".into(),
                    writes: vec![
                        TaskWrite {
                            expected_revision: None,
                            record: parent
                        },
                        TaskWrite {
                            expected_revision: None,
                            record: child
                        }
                    ]
                })
                .await?,
            CommitResult::Committed { .. }
        ));
        storage
            .release_owner(StorageOwner {
                id: "recovery-fixture".into(),
            })
            .await?;
        seed.close().await?;
        let (host, tasks, _) = open(driver, &database, Some(1)).await?;
        let completing = status(&tasks, "recover-parent", TaskStatus::Completing).await?;
        assert!(matches!(
            completing.outcome,
            Some(TaskOutcome::Completed { .. })
        ));
        assert_eq!(completing.attempts, 1);
        assert!(
            !tasks
                .get(TaskLookup {
                    id: "recover-child".into()
                })
                .await?
                .unwrap()
                .is_terminal()
        );
        let terminal = tasks
            .wait(TaskLookup {
                id: "recover-parent".into(),
            })
            .await?;
        assert_eq!(Some(terminal), completing.outcome);
        assert_eq!(
            tasks
                .get(TaskLookup {
                    id: "recover-parent".into()
                })
                .await?
                .unwrap()
                .attempts,
            1
        );
        assert!(
            tasks
                .get(TaskLookup {
                    id: "recover-child".into()
                })
                .await?
                .unwrap()
                .is_terminal()
        );
        host.close().await
    });
}
