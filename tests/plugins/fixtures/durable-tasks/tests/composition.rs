//! Ordinary Profile installation, distinct storage providers and actual compatible JS reload.

use std::{
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_durable_tasks_fixture::{
    gate::{self, TaskGate, TaskGateInput},
    outbox_consumer, task, volatile_storage,
};
use swarmie_runtime::{
    Application, CompiledPlugin, Configuration, compiled_plugins, start_with_catalog,
};
use swarmie_task_contracts::*;
use tembra_native::TokioDriver;

fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = ()>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(30), test(driver.clone()))
                .await
                .expect("Profile task acceptance timed out");
        }));
}

fn release(plugin: &str, facet: &str, version: &str, kind: PluginKind) -> Release {
    Release {
        plugin: plugin.into(),
        version: version.into(),
        kind,
        facets: vec![FacetIdentity {
            id: facet.into(),
            target: FacetTarget::Server,
        }],
    }
}

fn selection(id: &str, plugin: &str, config: serde_json::Value) -> Selection {
    Selection {
        id: id.into(),
        plugin: plugin.into(),
        version: "0.1.0".into(),
        provider: None,
        config,
    }
}

fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../target/fixtures")
}

fn application(database: &Path, alternate: bool, dynamic: bool) -> Application {
    let mut application = Application::default();
    application
        .composition
        .profile
        .disable
        .push("swarmie.memory-store".into());
    application.composition.profile.id = "durable-acceptance".into();
    application.composition.profile.plugins.extend([
        selection(
            "task-storage",
            if alternate {
                volatile_storage::PLUGIN_ID
            } else {
                swarmie_task_storage_sqlite::PLUGIN_ID
            },
            if alternate {
                json!({})
            } else {
                json!({"database":database})
            },
        ),
        selection(
            "tasks",
            swarmie_durable_tasks::PLUGIN_ID,
            json!({"poll_interval_ms":2}),
        ),
        selection("task-gate", gate::FACET_ID, json!({})),
    ]);
    if dynamic {
        application.composition.profile.plugins.push(Selection {
            id: "workflow".into(),
            plugin: "durable-workflow".into(),
            version: "1.0.0".into(),
            provider: Some("local".into()),
            config: json!({}),
        });
        for version in ["1.0.0", "1.1.0"] {
            application.releases.push(release(
                "durable-workflow",
                "fixture.durable-workflow",
                version,
                PluginKind::Dynamic,
            ));
        }
    } else {
        application.composition.profile.plugins.push(selection(
            "workflow",
            "fixture.durable-workflow",
            json!({}),
        ));
    }
    application
}

fn catalog(driver: Rc<TokioDriver>) -> Vec<CompiledPlugin> {
    let mut catalog = compiled_plugins(source(), driver);
    catalog.push(CompiledPlugin::new(
        release(
            volatile_storage::PLUGIN_ID,
            volatile_storage::FACET_ID,
            "0.1.0",
            PluginKind::Core,
        ),
        |_| Ok(vec![volatile_storage::facet()?]),
    ));
    catalog.push(CompiledPlugin::new(
        release(
            "fixture.durable-workflow",
            "fixture.durable-workflow",
            "0.1.0",
            PluginKind::Core,
        ),
        |_| Ok(vec![swarmie_durable_tasks_fixture::facet(1)?]),
    ));
    catalog.push(CompiledPlugin::new(
        release(gate::FACET_ID, gate::FACET_ID, "0.1.0", PluginKind::Core),
        |_| Ok(vec![gate::facet()?]),
    ));
    catalog
}

fn configuration(root: &Path, application: Application) -> Configuration {
    Configuration {
        database: root.join("administration.sqlite"),
        source: source(),
        application: Some(application),
    }
}

async fn terminal(tasks: &DurableTasksRef, id: &str) -> TaskRecord {
    tasks.wait(TaskLookup { id: id.into() }).await.unwrap();
    tasks
        .get(TaskLookup { id: id.into() })
        .await
        .unwrap()
        .unwrap()
}

#[test]
fn profile_selects_sqlite_then_a_distinct_provider_without_changing_scheduler_contract_usage() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let first = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), application(&database, false, false)),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            first.deployment_errors.is_empty(),
            "{:?}",
            first.deployment_errors
        );
        let tasks = first
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        tasks.create(task("original-provider")).await.unwrap();
        let original = terminal(&tasks, "original-provider").await;
        first.host.close().await.unwrap();
        drop(first);

        let alternate = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), application(&database, true, false)),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            alternate.deployment_errors.is_empty(),
            "{:?}",
            alternate.deployment_errors
        );
        assert_eq!(
            alternate.applied["task-storage"].selection.plugin,
            volatile_storage::PLUGIN_ID
        );
        let unchanged = alternate
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        assert!(
            unchanged
                .get(TaskLookup {
                    id: "original-provider".into()
                })
                .await
                .unwrap()
                .is_none()
        );
        unchanged
            .create(task("replacement-provider"))
            .await
            .unwrap();
        assert!(
            terminal(&unchanged, "replacement-provider")
                .await
                .is_terminal()
        );
        alternate.host.close().await.unwrap();
        drop(alternate);

        let restored = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), application(&database, false, false)),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            restored.deployment_errors.is_empty(),
            "{:?}",
            restored.deployment_errors
        );
        let tasks = restored
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        assert_eq!(
            tasks
                .get(TaskLookup {
                    id: "original-provider".into()
                })
                .await
                .unwrap(),
            Some(original)
        );
        assert!(
            tasks
                .get(TaskLookup {
                    id: "replacement-provider".into()
                })
                .await
                .unwrap()
                .is_none()
        );
        restored.host.close().await.unwrap();
    });
}

#[test]
fn compatible_javascript_replacement_uses_new_definition_between_committed_phases() {
    run(|driver| async move {
        assert!(
            source()
                .join("durable-workflow/1.1.0/manifest.json")
                .is_file(),
            "build real task artifacts before acceptance"
        );
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let mut runtime = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), application(&database, false, true)),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        let tasks = runtime
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        let mut spec = task("reload-between");
        spec.input = json!({"wake_at_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64+5_000});
        tasks.create(spec).await.unwrap();
        loop {
            let record = tasks
                .get(TaskLookup {
                    id: "reload-between".into(),
                })
                .await
                .unwrap()
                .unwrap();
            assert!(
                !record.is_terminal()
                    && !matches!(record.status, TaskStatus::Parked | TaskStatus::Blocked),
                "unexpected record while awaiting sleep: {record:?}"
            );
            if record.status == TaskStatus::Sleeping {
                assert_eq!(record.checkpoint["prepared_by"], 1);
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let mut replacement = runtime.desired_application().clone();
        replacement
            .composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "workflow")
            .unwrap()
            .version = "1.1.0".into();
        let report = runtime.apply_application(replacement).await.unwrap();
        assert!(matches!(
            report.results["workflow"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        ));
        let completed = terminal(&tasks, "reload-between").await;
        assert_eq!(completed.definition_version, 2);
        assert_eq!(
            completed.outcome,
            Some(TaskOutcome::Completed {
                value: json!({"published":true,"version":2})
            })
        );
        assert_eq!(completed.checkpoint["prepared_by"], 1);
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn admitted_old_javascript_phase_settles_before_next_phase_uses_replacement() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let mut runtime = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), application(&database, false, true)),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        let tasks = runtime
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        let gate = runtime.host.get(TaskGate, TaskGateInput {}).await.unwrap();
        let mut spec = task("reload-running");
        spec.phase = "gated".into();
        tasks.create(spec).await.unwrap();
        while gate.entered().value().unwrap() == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let mut replacement = runtime.desired_application().clone();
        replacement
            .composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "workflow")
            .unwrap()
            .version = "1.1.0".into();
        let report = runtime.apply_application(replacement).await.unwrap();
        assert!(matches!(
            report.results["workflow"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        ));
        assert_eq!(
            tasks
                .get(TaskLookup {
                    id: "reload-running".into()
                })
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Running
        );
        gate.release_gate().await.unwrap();
        let completed = terminal(&tasks, "reload-running").await;
        assert_eq!(completed.checkpoint, json!({"executed_by":1}));
        assert_eq!(completed.definition_version, 2);
        assert_eq!(
            completed.outcome,
            Some(TaskOutcome::Completed {
                value: json!({"published":true,"version":2})
            })
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn a_definition_change_before_a_continued_phase_commits_pending_and_migrates() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let mut runtime = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), application(&database, false, true)),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        let storage = runtime
            .host
            .get(TaskStorage, TaskStorageInput {})
            .await
            .unwrap();
        let consumer = outbox_consumer::install(
            &runtime.host,
            "fixture.continuation-migration-audit",
            storage,
        )
        .await
        .unwrap();
        consumer
            .register_outbox_consumer(TaskOutboxConsumer {
                id: "continuation-migration-audit".into(),
            })
            .await
            .unwrap();
        let tasks = runtime
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        let gate = runtime.host.get(TaskGate, TaskGateInput {}).await.unwrap();
        let mut spec = task("reload-continued");
        spec.phase = "gated-continue".into();
        tasks.create(spec).await.unwrap();
        while gate.entered().value().unwrap() == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let mut replacement = runtime.desired_application().clone();
        replacement
            .composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "workflow")
            .unwrap()
            .version = "1.1.0".into();
        let report = runtime.apply_application(replacement).await.unwrap();
        assert!(matches!(
            report.results["workflow"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        ));
        gate.release_gate().await.unwrap();
        let completed = terminal(&tasks, "reload-continued").await;
        assert_eq!(completed.checkpoint, json!({"executed_by":1}));
        assert_eq!(completed.definition_version, 2);
        assert_eq!(
            completed.outcome,
            Some(TaskOutcome::Completed {
                value: json!({"published":true,"version":2})
            })
        );
        let events = consumer
            .read_outbox(TaskOutboxQuery {
                consumer_id: "continuation-migration-audit".into(),
                after: None,
                limit: 256,
            })
            .await
            .unwrap()
            .events;
        let history: Vec<_> = events
            .iter()
            .filter(|event| event.task_id == "reload-continued")
            .map(|event| {
                (
                    event.phase.as_str(),
                    event.definition_version,
                    event.current_status,
                    event.provenance,
                )
            })
            .collect();
        let provenance = |origin, checkpoint_committed| {
            Some(TaskTransitionProvenance {
                origin,
                checkpoint_committed,
            })
        };
        // The old generation's continuation is committed as an ordinary Pending checkpoint at
        // version one, because the next phase resolves version two. Migration then runs through
        // ordinary admission before the finish phase.
        assert_eq!(
            history,
            [
                (
                    "gated-continue",
                    1,
                    TaskStatus::Pending,
                    provenance(TaskLifecycleOrigin::Creation, false)
                ),
                (
                    "gated-continue",
                    1,
                    TaskStatus::Running,
                    provenance(TaskLifecycleOrigin::Reservation, false)
                ),
                (
                    "finish",
                    1,
                    TaskStatus::Pending,
                    provenance(TaskLifecycleOrigin::Handler, true)
                ),
                (
                    "finish",
                    1,
                    TaskStatus::Running,
                    provenance(TaskLifecycleOrigin::Reservation, false)
                ),
                (
                    "finish",
                    2,
                    TaskStatus::Pending,
                    provenance(TaskLifecycleOrigin::Handler, true)
                ),
                (
                    "finish",
                    2,
                    TaskStatus::Running,
                    provenance(TaskLifecycleOrigin::Reservation, false)
                ),
                (
                    "finish",
                    2,
                    TaskStatus::Terminal,
                    provenance(TaskLifecycleOrigin::Handler, false)
                ),
            ]
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn profile_without_scheduler_preserves_committed_work_across_host_replacement() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let selected = application(&database, false, false);
        let first = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), selected.clone()),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            first.deployment_errors.is_empty(),
            "{:?}",
            first.deployment_errors
        );
        let tasks = first
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        let mut spec = task("profile-sleeper");
        spec.phase = "sleep".into();
        spec.input = json!({"wake_at_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64+5_000});
        tasks.create(spec).await.unwrap();
        let sleeping = loop {
            let record = tasks
                .get(TaskLookup {
                    id: "profile-sleeper".into(),
                })
                .await
                .unwrap()
                .unwrap();
            assert!(
                !record.is_terminal()
                    && !matches!(record.status, TaskStatus::Parked | TaskStatus::Blocked),
                "unexpected record while awaiting sleep: {record:?}"
            );
            if record.status == TaskStatus::Sleeping {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        first.host.close().await.unwrap();
        drop(first);
        let mut omitted = selected.clone();
        omitted
            .composition
            .profile
            .plugins
            .retain(|selection| selection.id != "tasks");
        let inspection = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), omitted),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            inspection.deployment_errors.is_empty(),
            "{:?}",
            inspection.deployment_errors
        );
        assert!(!inspection.applied.contains_key("tasks"));
        let storage = inspection
            .host
            .get(TaskStorage, TaskStorageInput {})
            .await
            .unwrap();
        assert_eq!(
            storage
                .get(TaskLookup {
                    id: "profile-sleeper".into()
                })
                .await
                .unwrap(),
            Some(sleeping)
        );
        inspection.host.close().await.unwrap();
        drop(inspection);
        let resumed = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), selected),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        let tasks = resumed
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        assert!(terminal(&tasks, "profile-sleeper").await.is_terminal());
        resumed.host.close().await.unwrap();
    });
}

#[test]
fn dynamic_definition_unload_blocks_saved_checkpoint_and_reinstall_resumes() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let selected = application(&database, false, true);
        let mut runtime = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), selected.clone()),
            catalog(driver.clone()),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        let tasks = runtime
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        let mut spec = task("removed-definition");
        spec.input = json!({"wake_at_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64+5_000});
        tasks.create(spec).await.unwrap();
        let saved = loop {
            let record = tasks
                .get(TaskLookup {
                    id: "removed-definition".into(),
                })
                .await
                .unwrap()
                .unwrap();
            assert!(
                !record.is_terminal()
                    && !matches!(record.status, TaskStatus::Parked | TaskStatus::Blocked),
                "unexpected record while awaiting sleep: {record:?}"
            );
            if record.status == TaskStatus::Sleeping {
                break record.checkpoint;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        let mut removed = selected.clone();
        removed
            .composition
            .profile
            .plugins
            .retain(|selection| selection.id != "workflow");
        let report = runtime.apply_application(removed).await.unwrap();
        assert_eq!(
            report.results["workflow"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        );
        let blocked = loop {
            let record = tasks
                .get(TaskLookup {
                    id: "removed-definition".into(),
                })
                .await
                .unwrap()
                .unwrap();
            if record.status == TaskStatus::Blocked {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        assert_eq!(
            blocked.blocking_reason,
            Some(BlockingReason::MissingDefinition)
        );
        assert_eq!(blocked.checkpoint, saved);
        let report = runtime.apply_application(selected).await.unwrap();
        assert_eq!(
            report.results["workflow"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        );
        assert!(terminal(&tasks, "removed-definition").await.is_terminal());
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn javascript_lifecycle_contribution_reloads_and_retires_with_physical_ownership() {
    run(|driver| async move {
        use swarmie_memory_store::{
            GetRequest, GetResult, ListRequest, MemoryStore, MemoryStoreInput,
        };

        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("tasks.sqlite");
        let mut desired = application(&database, false, false);
        desired
            .composition
            .profile
            .disable
            .retain(|id| id != "swarmie.memory-store");
        desired
            .composition
            .bundles
            .get_mut("swarmie.foundation")
            .unwrap()
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "swarmie.memory-store")
            .unwrap()
            .config = json!({"database": directory.path().join("observations.sqlite")});
        desired.composition.profile.plugins.push(Selection {
            id: "observer".into(),
            plugin: "durable-observer".into(),
            version: "1.0.0".into(),
            provider: Some("local".into()),
            config: json!({}),
        });
        for version in ["1.0.0", "1.1.0"] {
            desired.releases.push(release(
                "durable-observer",
                "fixture.durable-observer",
                version,
                PluginKind::Dynamic,
            ));
        }
        let mut runtime = start_with_catalog(
            driver.clone(),
            configuration(directory.path(), desired),
            catalog(driver),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        let tasks = runtime
            .host
            .get(DurableTasks, DurableTasksInput {})
            .await
            .unwrap();
        let lifecycle = runtime
            .host
            .get(TaskLifecycle, TaskLifecycleInput {})
            .await
            .unwrap();
        let memory = runtime
            .host
            .get(MemoryStore, MemoryStoreInput {})
            .await
            .unwrap();
        let gate = runtime.host.get(TaskGate, TaskGateInput {}).await.unwrap();
        tasks.create(task("observer-gated")).await.unwrap();
        while gate.entered().value().unwrap() == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        // The physical-ownership assertion must not race the observation deadline while
        // real package validation, JavaScript preparation and SQLite commits run. Timeout
        // behavior is tested separately with real time; only this reload window pauses time.
        let mut replacement = runtime.desired_application().clone();
        replacement
            .composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "observer")
            .unwrap()
            .version = "1.1.0".into();
        let host = runtime.host.clone();
        let before = host.core().code_version();
        let (report, released) = {
            tokio::time::pause();
            let wall_started = std::time::Instant::now();
            let reload = async {
                futures::join!(runtime.apply_application(replacement), async {
                    while host.core().code_version() == before {
                        tokio::task::yield_now().await;
                    }
                    gate.release_gate().await
                })
            };
            futures::pin_mut!(reload);
            let result = loop {
                assert!(
                    wall_started.elapsed() < Duration::from_secs(10),
                    "real reload did not progress while the test clock was paused"
                );
                // Keep a runnable future so Tokio does not automatically advance paused time.
                tokio::select! {
                    biased;
                    result = &mut reload => break result,
                    () = tokio::task::yield_now() => {}
                }
            };
            tokio::time::resume();
            result
        };
        let report = report.unwrap();
        released.unwrap();
        assert!(matches!(
            report.results["observer"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        ));
        terminal(&tasks, "observer-gated").await;
        loop {
            let diagnostics = lifecycle.diagnostics().await.unwrap();
            assert_eq!(diagnostics.failed, 0, "{diagnostics:?}");
            if !diagnostics.active && diagnostics.queued == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let GetResult::Present { value: first } = memory
            .get(GetRequest {
                namespace: "lifecycle-acceptance".into(),
                key: "observer-gated:1:committed".into(),
            })
            .await
            .unwrap()
        else {
            panic!("old admitted JavaScript observer must physically finish");
        };
        assert_eq!(first["version"], 1);
        assert_eq!(first["event"]["origin"], "creation");
        tasks.create(task("observer-new")).await.unwrap();
        let new = terminal(&tasks, "observer-new").await;
        let key = format!("observer-new:{}:committed", new.revision);
        let value = loop {
            if let GetResult::Present { value } = memory
                .get(GetRequest {
                    namespace: "lifecycle-acceptance".into(),
                    key: key.clone(),
                })
                .await
                .unwrap()
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        assert_eq!(value["version"], 2);
        assert_eq!(value["event"]["current_status"], "terminal");
        assert_eq!(value["event"]["outcome"], "completed");
        // Explicit removal retracts the contribution from subsequent managed plans.
        let mut removed = runtime.desired_application().clone();
        removed
            .composition
            .profile
            .plugins
            .retain(|selection| selection.id != "observer");
        let report = runtime.apply_application(removed).await.unwrap();
        assert!(matches!(
            report.results["observer"].outcome,
            swarmie_runtime::TransitionOutcome::Applied
        ));
        tasks.create(task("observer-removed")).await.unwrap();
        terminal(&tasks, "observer-removed").await;
        let keys = memory
            .list(ListRequest {
                namespace: "lifecycle-acceptance".into(),
            })
            .await
            .unwrap();
        assert!(keys.iter().all(|key| !key.starts_with("observer-removed:")));
        assert!(lifecycle.diagnostics().await.unwrap().skipped > 0);
        runtime.host.close().await.unwrap();
    });
}
