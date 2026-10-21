//! Optional Agent Loop selection through the executable's actual compiled catalog.

use std::{path::Path, rc::Rc, time::Duration};

use serde_json::json;
use swarmie_composition::{FacetTarget, PluginKind, Selection, SelectionState};
use swarmie_plugin_contracts::ObservedState;
use swarmie_runtime::{Application, Configuration, compiled_plugins, resolve_application};
use tembra::core;
use tembra_native::TokioDriver;

fn select(application: &mut Application, plugin: &str, configuration: serde_json::Value) {
    application.composition.profile.plugins.push(Selection {
        id: plugin.into(),
        plugin: plugin.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        provider: None,
        config: configuration,
    });
}

fn agent_application(root: &Path) -> Application {
    let mut application = Application::default();
    for bundle in application.composition.bundles.values_mut() {
        for selection in &mut bundle.plugins {
            if selection.plugin == swarmie_memory_store::PLUGIN_ID {
                selection.config = json!({"database":root.join("memory.sqlite")});
            }
        }
    }
    select(
        &mut application,
        swarmie_task_storage_sqlite::PLUGIN_ID,
        json!({"database":root.join("tasks.sqlite")}),
    );
    select(
        &mut application,
        swarmie_durable_tasks::PLUGIN_ID,
        json!({}),
    );
    select(
        &mut application,
        swarmie_session_store_sqlite::PLUGIN_ID,
        json!({"database":root.join("sessions.sqlite")}),
    );
    select(
        &mut application,
        swarmie_agent_loop_default::PLUGIN_ID,
        json!({}),
    );
    application
}

#[test]
fn compiled_agent_catalog_is_available_without_selecting_it_by_default() {
    let catalog = compiled_plugins("unused-source".into(), core::ManualDriver::new());
    for (plugin, facet) in [
        (
            swarmie_agent_loop_default::PLUGIN_ID,
            swarmie_agent_loop_default::FACET_ID,
        ),
        (
            swarmie_session_store_sqlite::PLUGIN_ID,
            swarmie_session_store_sqlite::FACET_ID,
        ),
    ] {
        let release = &catalog
            .iter()
            .find(|candidate| candidate.release.plugin == plugin)
            .expect("optional compiled release")
            .release;
        assert_eq!(release.kind, PluginKind::Core);
        assert_eq!(release.facets.len(), 1);
        assert_eq!(release.facets[0].id, facet);
        assert_eq!(release.facets[0].target, FacetTarget::Server);
    }
    let effective = resolve_application(&Application::default()).unwrap();
    for plugin in [
        swarmie_agent_loop_default::PLUGIN_ID,
        swarmie_session_store_sqlite::PLUGIN_ID,
    ] {
        assert!(
            !effective
                .instances
                .values()
                .any(|instance| instance.selection.plugin == plugin)
        );
    }
}

#[test]
fn explicit_agent_selection_validates_configuration_without_opening_databases() {
    let root = tempfile::tempdir().unwrap();
    let application = agent_application(root.path());
    let effective = resolve_application(&application).unwrap();
    for plugin in [
        swarmie_agent_loop_default::PLUGIN_ID,
        swarmie_session_store_sqlite::PLUGIN_ID,
    ] {
        assert_eq!(effective.instances[plugin].state, SelectionState::Enabled);
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);

    let mut invalid = application.clone();
    invalid
        .composition
        .profile
        .plugins
        .iter_mut()
        .find(|selection| selection.plugin == swarmie_agent_loop_default::PLUGIN_ID)
        .unwrap()
        .config = json!({"unknown":true});
    assert!(resolve_application(&invalid).is_err());
    invalid = application;
    invalid
        .composition
        .profile
        .plugins
        .iter_mut()
        .find(|selection| selection.plugin == swarmie_session_store_sqlite::PLUGIN_ID)
        .unwrap()
        .config = json!({"database":":memory:"});
    assert!(resolve_application(&invalid).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn explicit_agent_profile_starts_without_an_implicit_model_provider() {
    let root = tempfile::tempdir().unwrap();
    let driver = TokioDriver::new();
    let scheduler = driver.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scheduler.run_until(async {
            let runtime = tokio::time::timeout(
                Duration::from_secs(10),
                swarmie_runtime::start(
                    driver as Rc<dyn core::Driver>,
                    Configuration {
                        database: root.path().join("manager.sqlite"),
                        source: root.path().join("source"),
                        application: Some(agent_application(root.path())),
                    },
                ),
            )
            .await
            .expect("optional agent profile startup did not settle")
            .unwrap();
            assert!(
                runtime.deployment_errors.is_empty(),
                "{:?}",
                runtime.deployment_errors
            );
            assert!(runtime.recovery_error.is_none());
            let inventory = runtime.manager.inventory().unwrap();
            for plugin in [
                swarmie_agent_loop_default::PLUGIN_ID,
                swarmie_session_store_sqlite::PLUGIN_ID,
            ] {
                assert!(
                    inventory
                        .plugins
                        .iter()
                        .any(|record| record.plugin_id == plugin
                            && record.observed == ObservedState::Active)
                );
            }
            runtime.host.close().await.unwrap();
        }));
}
