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
    task, volatile_storage,
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
