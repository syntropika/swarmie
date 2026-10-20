use std::{
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use serde_json::json;
use swarmie_composition::{
    Capability, CapabilityRequirement, FacetIdentity, FacetTarget, InstanceState, Origin,
    PluginKind, Release, Selection,
};
use swarmie_plugin_contracts::{
    Counter, CounterInput, DynamicConsumer, DynamicConsumerInput, InstallRequest, ObservedState,
};
use swarmie_plugin_manager::{ManagerError, PluginManager};
use swarmie_runtime::{
    Application, CompiledPlugin, Configuration, Runtime, application_contracts, start,
    start_with_catalog,
};
use tembra::NativeFacet;
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
            tokio::time::timeout(Duration::from_secs(20), test(driver.clone()))
                .await
                .expect("composition acceptance did not settle");
        }));
}

fn selection(id: &str, plugin: &str, version: &str, provider: Option<&str>) -> Selection {
    Selection {
        id: id.into(),
        plugin: plugin.into(),
        version: version.into(),
        provider: provider.map(String::from),
        config: json!({}),
        requires: vec![],
    }
}

fn dynamic_release(plugin: &str, contracts: &[&str]) -> Release {
    Release {
        plugin: plugin.into(),
        version: "1.0.0".into(),
        kind: PluginKind::Dynamic,
        facets: vec![FacetIdentity {
            id: plugin.into(),
            target: FacetTarget::Server,
        }],
        provides: contracts
            .iter()
            .map(|contract| Capability {
                contract: (*contract).into(),
                version: 1,
            })
            .collect(),
        requires: vec![],
    }
}

fn with_consumer() -> Application {
    let mut app = Application::default();
    app.composition.profile.id = "counter-acceptance".into();
    app.composition.profile.plugins.push(selection(
        "counter-client",
        "dynamic-consumer",
        "1.0.0",
        Some("local"),
    ));
    let mut consumer = dynamic_release("dynamic-consumer", &["swarmie.dynamic-consumer"]);
    consumer.requires.push(CapabilityRequirement {
        contract: "swarmie.counter".into(),
        version: 1,
        optional: false,
    });
    app.releases.push(consumer);
    app
}

fn replace_counter(mut app: Application) -> Application {
    app.composition.profile.overrides.push(selection(
        "swarmie.counter",
        "alternative-counter",
        "1.0.0",
        Some("local"),
    ));
    app.releases
        .push(dynamic_release("alternative-counter", &["swarmie.counter"]));
    app
}

fn fixtures() -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    for plugin in ["alternative-counter", "dynamic-consumer"] {
        assert!(
            source.join(plugin).join("1.0.0/manifest.json").is_file(),
            "build genuine plugin artifacts with just tembra-plugins"
        );
    }
    source
}

fn copy_fixtures(destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for plugin in ["alternative-counter", "dynamic-consumer"] {
        let package = destination.join(plugin);
        std::fs::create_dir(&package).unwrap();
        for entry in std::fs::read_dir(fixtures().join(plugin).join("1.0.0")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), package.join(entry.file_name())).unwrap();
        }
    }
}

async fn exercise_consumer(runtime: &Runtime, expected_initial: u32) {
    let counter = runtime.host.get(Counter, CounterInput {}).await.unwrap();
    assert_eq!(counter.read().await.unwrap(), expected_initial);
    let consumer = runtime
        .host
        .get(DynamicConsumer, DynamicConsumerInput {})
        .await
        .unwrap();
    assert_eq!(consumer.run().await.unwrap(), expected_initial + 1);
    assert_eq!(counter.read().await.unwrap(), expected_initial + 1);
}

#[test]
fn profile_override_substitutes_dynamic_service_without_changing_the_consumer() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let app = with_consumer();
        let baseline = start(
            driver.clone(),
            Configuration {
                database: root.path().join("native.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app.clone()),
            },
        )
        .await
        .unwrap();
        assert!(
            baseline.deployment_errors.is_empty(),
            "{:?}",
            baseline.deployment_errors
        );
        exercise_consumer(&baseline, 0).await;
        let consumer_selection = baseline.effective.instances["counter-client"]
            .selection
            .clone();
        baseline.host.close().await.unwrap();
        drop(baseline);

        let replacement = start(
            driver,
            Configuration {
                database: root.path().join("dynamic.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(replace_counter(app)),
            },
        )
        .await
        .unwrap();
        assert!(
            replacement.deployment_errors.is_empty(),
            "{:?}",
            replacement.deployment_errors
        );
        assert_eq!(
            replacement.effective.instances["counter-client"].selection,
            consumer_selection
        );
        assert_eq!(
            replacement.effective.instances["swarmie.counter"].origin,
            Origin::Override
        );
        assert_eq!(
            replacement.effective.instances["swarmie.counter"]
                .selection
                .plugin,
            "alternative-counter"
        );
        exercise_consumer(&replacement, 10).await;
        let inventory = replacement.manager.inventory(&proof).unwrap();
        assert!(
            inventory
                .plugins
                .iter()
                .any(|record| record.plugin_id == "alternative-counter"
                    && record.observed == ObservedState::Active)
        );
        assert!(
            !inventory
                .plugins
                .iter()
                .any(|record| record.plugin_id == "swarmie.counter")
        );
        replacement.host.close().await.unwrap();
    });
}

#[test]
fn dynamic_provider_reads_overridden_configuration_through_its_contract() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = replace_counter(with_consumer());
        app.composition.profile.overrides[0].config = json!({ "initial": 42 });
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof,
                application: Some(app),
            },
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        exercise_consumer(&runtime, 42).await;
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn dynamic_configuration_refusal_keeps_administration_and_never_reports_active() {
    run(|driver| async move {
        for config in [json!({ "initial": -1 }), json!({ "unexpected": true })] {
            let root = tempfile::tempdir().unwrap();
            let proof = PluginManager::issue_proof();
            let mut app = replace_counter(with_consumer());
            app.composition.profile.overrides[0].config = config;
            let runtime = start(
                driver.clone(),
                Configuration {
                    database: root.path().join("manager.sqlite"),
                    source: fixtures(),
                    proof: proof.clone(),
                    application: Some(app),
                },
            )
            .await
            .unwrap();
            assert!(runtime.deployment_errors.contains_key("swarmie.counter"));
            let inventory = runtime.manager.inventory(&proof).unwrap();
            let provider = inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "alternative-counter")
                .unwrap();
            assert_eq!(provider.observed, ObservedState::Unavailable);
            assert!(
                !inventory
                    .plugins
                    .iter()
                    .any(|plugin| plugin.plugin_id == "dynamic-consumer"
                        && plugin.observed == ObservedState::Active)
            );
            runtime.host.close().await.unwrap();
        }
    });
}

#[test]
fn compiled_provider_applies_valid_typed_configuration() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = with_consumer();
        let mut override_selection = selection(
            "swarmie.counter",
            "swarmie.counter",
            env!("CARGO_PKG_VERSION"),
            None,
        );
        override_selection.config = json!({ "initial": 17 });
        app.composition.profile.overrides.push(override_selection);
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof,
                application: Some(app),
            },
        )
        .await
        .unwrap();
        exercise_consumer(&runtime, 17).await;
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn acquisition_failure_is_inspectable_without_claiming_a_runtime_activation() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = Application::default();
        app.composition.profile.plugins.push(selection(
            "unavailable",
            "missing-package",
            "1.0.0",
            Some("local"),
        ));
        app.releases.push(dynamic_release("missing-package", &[]));
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app),
            },
        )
        .await
        .unwrap();
        assert!(runtime.deployment_errors.contains_key("unavailable"));
        assert!(
            !runtime
                .manager
                .inventory(&proof)
                .unwrap()
                .plugins
                .iter()
                .any(|plugin| plugin.plugin_id == "missing-package"
                    && plugin.observed == ObservedState::Active)
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn catalog_artifact_identity_mismatch_refuses_execution_before_recovery() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = replace_counter(with_consumer());
        app.composition.profile.plugins.clear();
        app.releases
            .retain(|release| release.plugin == "alternative-counter");
        app.releases[0].facets[0].id = "unapproved-facet".into();
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app),
            },
        )
        .await
        .unwrap();
        assert!(runtime.deployment_errors["swarmie.counter"].contains("differs"));
        assert!(
            !runtime
                .manager
                .inventory(&proof)
                .unwrap()
                .plugins
                .iter()
                .any(|plugin| plugin.plugin_id == "alternative-counter"
                    && plugin.observed == ObservedState::Active)
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn a_release_cannot_claim_another_plugins_live_capability() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = Application::default();
        app.composition.profile.plugins.push(selection(
            "false-counter-claim",
            "dynamic-consumer",
            "1.0.0",
            Some("local"),
        ));
        app.releases
            .push(dynamic_release("dynamic-consumer", &["swarmie.counter"]));
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app),
            },
        )
        .await
        .unwrap();
        assert!(
            runtime
                .deployment_errors
                .contains_key("false-counter-claim")
        );
        let inventory = runtime.manager.inventory(&proof).unwrap();
        let claimant = inventory
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "dynamic-consumer")
            .unwrap();
        assert_ne!(claimant.observed, ObservedState::Active);
        assert_eq!(
            runtime
                .host
                .get(Counter, CounterInput {})
                .await
                .unwrap()
                .read()
                .await
                .unwrap(),
            0
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn remote_capability_observation_without_owned_evidence_is_refused() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = replace_counter(Application::default());
        app.releases[0].provides.push(Capability {
            contract: "swarmie.remote-counter".into(),
            version: 1,
        });
        let result = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof,
                application: Some(app),
            },
        )
        .await;
        assert!(result.is_err());
    });
}

#[test]
fn persisted_composition_recovers_admitted_artifacts_without_the_original_source() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        copy_fixtures(&source);
        let proof = PluginManager::issue_proof();
        let first = start(
            driver.clone(),
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: source.clone(),
                proof: proof.clone(),
                application: Some(replace_counter(with_consumer())),
            },
        )
        .await
        .unwrap();
        assert!(
            first.deployment_errors.is_empty(),
            "{:?}",
            first.deployment_errors
        );
        let effective = first.effective.clone();
        exercise_consumer(&first, 10).await;
        first.host.close().await.unwrap();
        drop(first);
        std::fs::remove_dir_all(&source).unwrap();
        let recovered = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source,
                proof: proof.clone(),
                application: None,
            },
        )
        .await
        .unwrap();
        assert!(recovered.recovery_error.is_none());
        assert!(
            recovered.deployment_errors.is_empty(),
            "{:?}",
            recovered.deployment_errors
        );
        assert_eq!(recovered.effective, effective);
        exercise_consumer(&recovered, 10).await;
        assert!(recovered.manager.composition(&proof).unwrap().is_some());
        recovered.host.close().await.unwrap();
    });
}

#[test]
fn omitted_cached_consumer_cannot_veto_the_current_profiles_provider_upgrade() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        copy_fixtures(&source);
        let consumer_manifest = source.join("dynamic-consumer/manifest.json");
        let mut consumer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&consumer_manifest).unwrap()).unwrap();
        consumer["dependencies"] = json!([{ "id": "alternative-counter", "version": "^1.0.0" }]);
        std::fs::write(consumer_manifest, serde_json::to_vec(&consumer).unwrap()).unwrap();
        let successor = source.join("alternative-counter-v2");
        std::fs::create_dir(&successor).unwrap();
        for entry in std::fs::read_dir(source.join("alternative-counter")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), successor.join(entry.file_name())).unwrap();
        }
        let manifest = successor.join("manifest.json");
        let mut release: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        release["version"] = json!("2.0.0");
        std::fs::write(manifest, serde_json::to_vec(&release).unwrap()).unwrap();
        let proof = PluginManager::issue_proof();
        let first = start(
            driver.clone(),
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: source.clone(),
                proof: proof.clone(),
                application: Some(replace_counter(with_consumer())),
            },
        )
        .await
        .unwrap();
        assert!(
            first.deployment_errors.is_empty(),
            "{:?}",
            first.deployment_errors
        );
        exercise_consumer(&first, 10).await;
        first.host.close().await.unwrap();
        drop(first);

        let mut application = replace_counter(Application::default());
        application.composition.profile.overrides[0].version = "2.0.0".into();
        application.composition.profile.overrides[0].config = json!({ "initial": 42 });
        application.releases[0].version = "2.0.0".into();
        let next = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source,
                proof: proof.clone(),
                application: Some(application),
            },
        )
        .await
        .unwrap();
        assert!(
            next.deployment_errors.is_empty(),
            "{:?}",
            next.deployment_errors
        );
        assert_eq!(
            next.host
                .get(Counter, CounterInput {})
                .await
                .unwrap()
                .read()
                .await
                .unwrap(),
            42
        );
        let inventory = next.manager.inventory(&proof).unwrap();
        let old_consumer = inventory
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "dynamic-consumer")
            .unwrap();
        assert_eq!(
            old_consumer.desired,
            swarmie_plugin_contracts::DesiredState::Installed
        );
        assert_eq!(old_consumer.observed, ObservedState::Installed);
        assert_eq!(
            inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "alternative-counter")
                .unwrap()
                .version,
            "2.0.0"
        );
        next.host.close().await.unwrap();
    });
}

#[test]
fn unselected_cached_package_does_not_reserve_a_replacement_facets_identity() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let first = start(
            driver.clone(),
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(replace_counter(Application::default())),
            },
        )
        .await
        .unwrap();
        assert!(first.deployment_errors.is_empty());
        assert_eq!(
            first
                .host
                .get(Counter, CounterInput {})
                .await
                .unwrap()
                .read()
                .await
                .unwrap(),
            10
        );
        first.host.close().await.unwrap();
        drop(first);

        let mut application = Application::default();
        application.composition.profile.bundles.clear();
        application.composition.profile.plugins.push(selection(
            "replacement",
            "example.compiled-replacement",
            "1.0.0",
            None,
        ));
        let compiled = CompiledPlugin::new(
            Release {
                plugin: "example.compiled-replacement".into(),
                version: "1.0.0".into(),
                kind: PluginKind::Core,
                facets: vec![FacetIdentity {
                    id: "alternative-counter".into(),
                    target: FacetTarget::Server,
                }],
                provides: vec![],
                requires: vec![],
            },
            |_| {
                Ok(vec![NativeFacet::new(
                    "alternative-counter",
                    vec![],
                    |_, _| Ok(()),
                )?])
            },
        );
        let next = start_with_catalog(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(application),
            },
            vec![compiled],
            application_contracts().unwrap(),
        )
        .await
        .unwrap();
        assert!(next.deployment_errors.is_empty());
        let inventory = next.manager.inventory(&proof).unwrap();
        assert_eq!(
            inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "alternative-counter")
                .unwrap()
                .observed,
            ObservedState::Installed
        );
        assert_eq!(
            inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "example.compiled-replacement")
                .unwrap()
                .observed,
            ObservedState::Active
        );
        next.host.close().await.unwrap();
    });
}

#[test]
fn real_provider_removal_revokes_availability_and_restart_reconciles_desired_intent() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let runtime = start(
            driver.clone(),
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(replace_counter(with_consumer())),
            },
        )
        .await
        .unwrap();
        assert!(runtime.deployment_errors.is_empty());
        let counter = runtime.host.get(Counter, CounterInput {}).await.unwrap();
        let consumer = runtime
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.run().await.unwrap(), 11);
        runtime
            .host
            .core()
            .reload(
                vec![],
                tembra::core::ReloadOptions {
                    remove: vec!["alternative-counter".into()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(counter.core().status(), tembra::core::RefStatus::Gone);
        assert!(consumer.run().await.is_err());
        let inventory = runtime.manager.inventory(&proof).unwrap();
        for plugin_id in ["alternative-counter", "dynamic-consumer"] {
            let plugin = inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == plugin_id)
                .unwrap();
            assert_ne!(
                plugin.observed,
                ObservedState::Active,
                "{plugin_id} must reflect lost dependency availability"
            );
        }
        counter.core().release();
        consumer.core().release();
        runtime.host.close().await.unwrap();
        drop(runtime);

        let restored = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: None,
            },
        )
        .await
        .unwrap();
        assert!(
            restored.deployment_errors.is_empty(),
            "{:?}",
            restored.deployment_errors
        );
        exercise_consumer(&restored, 10).await;
        let inventory = restored.manager.inventory(&proof).unwrap();
        for plugin_id in ["alternative-counter", "dynamic-consumer"] {
            assert_eq!(
                inventory
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == plugin_id)
                    .unwrap()
                    .observed,
                ObservedState::Active
            );
        }
        restored.host.close().await.unwrap();
    });
}

#[test]
fn invalid_compiled_configuration_is_refused_before_becoming_durable_intent() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = Application::default();
        let mut invalid = selection(
            "swarmie.counter",
            "swarmie.counter",
            env!("CARGO_PKG_VERSION"),
            None,
        );
        invalid.config = json!({ "initial": -1 });
        app.composition.profile.overrides.push(invalid);
        let result = start(
            driver.clone(),
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app),
            },
        )
        .await;
        assert!(result.is_err());
        let recovered = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: None,
            },
        )
        .await
        .unwrap();
        assert!(recovered.manager.composition(&proof).unwrap().is_none());
        let counter = recovered.host.get(Counter, CounterInput {}).await.unwrap();
        assert_eq!(counter.read().await.unwrap(), 0);
        recovered.host.close().await.unwrap();
    });
}

#[test]
fn profile_managed_node_refuses_imperative_installation() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(Application::default()),
            },
        )
        .await
        .unwrap();
        let outcome = runtime
            .manager
            .install(InstallRequest {
                proof: proof.clone(),
                provider_id: "local".into(),
                plugin_id: "dynamic-consumer".into(),
                version: "1.0.0".into(),
                confirmed: true,
            })
            .await;
        assert!(matches!(outcome, Err(ManagerError::CompositionManaged)));
        assert!(
            runtime
                .manager
                .installed_packages(&proof)
                .unwrap()
                .is_empty()
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn blocked_capability_does_not_publish_an_active_consumer() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = with_consumer();
        app.composition
            .profile
            .disable
            .push("swarmie.counter".into());
        let runtime = start(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            runtime.effective.instances["counter-client"].state,
            InstanceState::Blocked { .. }
        ));
        assert!(
            !runtime
                .effective
                .activation_order
                .contains(&"counter-client".into())
        );
        assert!(
            !runtime
                .manager
                .inventory(&proof)
                .unwrap()
                .plugins
                .iter()
                .any(|plugin| plugin.plugin_id == "dynamic-consumer"
                    && plugin.observed == ObservedState::Active)
        );
        assert!(
            runtime
                .manager
                .installed_packages(&proof)
                .unwrap()
                .is_empty()
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn configured_instance_package_and_multiple_compiled_facets_have_separate_identities() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let mut app = Application::default();
        app.composition.profile.bundles.clear();
        app.composition.profile.plugins.push(selection(
            "configured-counter",
            "swarmie.counter",
            "1.0.0",
            None,
        ));
        let plugin = CompiledPlugin::new(
            Release {
                plugin: "swarmie.counter".into(),
                version: "1.0.0".into(),
                kind: PluginKind::Core,
                facets: vec![
                    FacetIdentity {
                        id: swarmie_counter::FACET_ID.into(),
                        target: FacetTarget::Server,
                    },
                    FacetIdentity {
                        id: "example.extra-facet".into(),
                        target: FacetTarget::Server,
                    },
                ],
                provides: ["swarmie.counter"]
                    .map(|contract| Capability {
                        contract: contract.into(),
                        version: 1,
                    })
                    .into(),
                requires: vec![],
            },
            |_| {
                Ok(vec![
                    swarmie_counter::facet_with_initial(42)?,
                    NativeFacet::new("example.extra-facet", vec![], |_, _| Ok(()))?,
                ])
            },
        );
        let runtime = start_with_catalog(
            driver,
            Configuration {
                database: root.path().join("manager.sqlite"),
                source: fixtures(),
                proof: proof.clone(),
                application: Some(app),
            },
            vec![plugin],
            application_contracts().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(runtime.effective.activation_order, ["configured-counter"]);
        assert_eq!(
            runtime.effective.instances["configured-counter"]
                .release
                .as_ref()
                .unwrap()
                .facets
                .len(),
            2
        );
        assert_eq!(
            runtime
                .host
                .get(Counter, CounterInput {})
                .await
                .unwrap()
                .read()
                .await
                .unwrap(),
            42
        );
        assert!(
            runtime
                .manager
                .inventory(&proof)
                .unwrap()
                .plugins
                .iter()
                .any(|record| record.plugin_id == "swarmie.counter"
                    && record.observed == ObservedState::Active)
        );
        runtime.host.close().await.unwrap();
    });
}
