#[path = "../../../tests/plugins/fixtures/runtime.rs"]
mod fixture_runtime;

use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_counter::{Counter, CounterInput, DynamicConsumer, DynamicConsumerInput};
use swarmie_plugin_contracts::{ObservedState, PluginAdministration, PluginAdministrationInput};
use swarmie_runtime::{Application, Configuration};
use tembra_native::TokioDriver;

fn application(version: &str) -> Application {
    let mut application = fixture_runtime::application();
    application.composition.profile.plugins.push(Selection {
        id: "consumer".into(),
        plugin: "dynamic-consumer".into(),
        version: version.into(),
        provider: Some("local".into()),
        config: serde_json::json!({}),
    });
    application.releases.push(Release {
        plugin: "dynamic-consumer".into(),
        version: version.into(),
        kind: PluginKind::Dynamic,
        facets: vec![FacetIdentity {
            id: "dynamic-consumer".into(),
            target: FacetTarget::Server,
        }],
    });
    application
}

#[test]
fn headless_bootstrap_recovers_profile_selected_cached_code_without_its_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/dynamic-consumer/1.0.0");
    assert!(
        fixture.exists(),
        "build genuine fixtures through just tembra-plugins"
    );
    let package = source.join("consumer");
    std::fs::create_dir(&package).unwrap();
    for entry in std::fs::read_dir(fixture).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), package.join(entry.file_name())).unwrap();
    }
    let driver = TokioDriver::new();
    let scheduler = driver.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scheduler.run_until(async {
            let configuration = |application| Configuration {
                database: directory.path().join("manager.sqlite"),
                source: source.clone(),
                application,
            };
            let first =
                fixture_runtime::start(driver.clone(), configuration(Some(application("1.0.0"))))
                    .await
                    .unwrap();
            assert!(
                first.deployment_errors.is_empty(),
                "{:?}",
                first.deployment_errors
            );
            assert!(first.recovery_error.is_none());
            let desired = first.manager.composition().unwrap();
            first.host.close().await.unwrap();
            drop(first);
            std::fs::remove_dir_all(&source).unwrap();
            let second = fixture_runtime::start(driver, configuration(None))
                .await
                .unwrap();
            assert!(
                second.deployment_errors.is_empty(),
                "{:?}",
                second.deployment_errors
            );
            assert!(second.recovery_error.is_none());
            assert_eq!(second.manager.composition().unwrap(), desired);
            assert!(
                second
                    .manager
                    .inventory()
                    .unwrap()
                    .plugins
                    .iter()
                    .any(|plugin| plugin.plugin_id == "dynamic-consumer"
                        && plugin.observed == ObservedState::Active)
            );
            let consumer = second
                .host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap();
            assert_eq!(consumer.run().await.unwrap(), 1);
            let counter = second.host.get(Counter, CounterInput {}).await.unwrap();
            assert_eq!(counter.read().await.unwrap(), 1);
            second.host.close().await.unwrap();
        }));
}

#[test]
fn failed_cached_setup_keeps_administration_available_and_fresh_profile_repairs_it() {
    let directory = tempfile::tempdir().unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/dynamic-consumer/1.2.0");
    assert!(
        source.exists(),
        "build genuine fixtures through just tembra-plugins"
    );
    let driver = TokioDriver::new();
    let scheduler = driver.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scheduler.run_until(async {
            let configuration = |application| Configuration {
                database: directory.path().join("manager.sqlite"),
                source: source.clone(),
                application,
            };
            let first =
                fixture_runtime::start(driver.clone(), configuration(Some(application("1.2.0"))))
                    .await
                    .unwrap();
            assert!(first.recovery_error.is_some() || !first.deployment_errors.is_empty());
            first.host.close().await.unwrap();
            drop(first);
            let second = fixture_runtime::start(driver.clone(), configuration(None))
                .await
                .unwrap();
            assert!(
                second
                    .recovery_error
                    .as_ref()
                    .expect("mixed native and foreign startup failure")
                    .requires_restart()
            );
            let administration = second
                .host
                .get(PluginAdministration, PluginAdministrationInput {})
                .await
                .unwrap();
            let inventory = administration.inventory().await.unwrap();
            let consumer = inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "dynamic-consumer")
                .expect("cached failed consumer");
            assert_eq!(consumer.observed, ObservedState::Unavailable);
            assert!(!consumer.errors.is_empty());
            assert!(
                second
                    .host
                    .get(DynamicConsumer, DynamicConsumerInput {})
                    .await
                    .is_err()
            );
            second.host.close().await.unwrap();
            drop(second);
            let repaired =
                fixture_runtime::start(driver, configuration(Some(fixture_runtime::application())))
                    .await
                    .unwrap();
            assert!(repaired.recovery_error.is_none());
            assert!(
                repaired.deployment_errors.is_empty(),
                "{:?}",
                repaired.deployment_errors
            );
            assert_eq!(
                repaired
                    .host
                    .get(Counter, CounterInput {})
                    .await
                    .unwrap()
                    .read()
                    .await
                    .unwrap(),
                0
            );
            assert!(
                repaired
                    .manager
                    .inventory()
                    .unwrap()
                    .plugins
                    .iter()
                    .any(|plugin| plugin.plugin_id == "dynamic-consumer"
                        && plugin.observed != ObservedState::Active)
            );
            repaired.host.close().await.unwrap();
        }));
}
