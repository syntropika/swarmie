#[path = "../../../tests/plugins/fixtures/runtime.rs"]
mod fixture_runtime;

use std::{
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use fixture_runtime::start;
use serde_json::json;
use swarmie_composition::{
    FacetIdentity, FacetTarget, Origin, PluginKind, Release, Selection, SelectionState,
};
use swarmie_counter::{
    Counter, CounterInput, CounterRef, DynamicConsumer, DynamicConsumerInput,
    DynamicConsumerProvider,
};
use swarmie_plugin_contracts::{ObservedState, PluginConfiguration, PluginConfigurationInput};
use swarmie_runtime::{Application, CompiledPlugin, Configuration, Runtime, start_with_catalog};
use tembra::{core, prelude::*};
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
    }
}

fn release(plugin: &str, version: &str, kind: PluginKind, facet: &str) -> Release {
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

fn with_consumer() -> Application {
    let mut app = fixture_runtime::application();
    app.composition.profile.id = "counter-acceptance".into();
    app.composition.profile.plugins.push(selection(
        "counter-client",
        "dynamic-consumer",
        "1.0.0",
        Some("local"),
    ));
    app.releases.push(release(
        "dynamic-consumer",
        "1.0.0",
        PluginKind::Dynamic,
        "dynamic-consumer",
    ));
    app
}

fn replace_counter(mut app: Application) -> Application {
    app.composition.profile.overrides.push(selection(
        "swarmie.counter",
        "alternative-counter",
        "1.0.0",
        Some("local"),
    ));
    app.releases.push(release(
        "alternative-counter",
        "1.0.0",
        PluginKind::Dynamic,
        "alternative-counter",
    ));
    app
}

fn fixtures() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    for plugin in ["alternative-counter", "dynamic-consumer"] {
        assert!(
            root.join(plugin).join("1.0.0/manifest.json").is_file(),
            "build real plugin artifacts with just tembra-plugins"
        );
    }
    root
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

fn config(database: &Path, source: &Path, application: Option<Application>) -> Configuration {
    Configuration {
        database: database.to_path_buf(),
        source: source.to_path_buf(),
        application,
    }
}

async fn exercise(runtime: &Runtime, initial: u32) {
    let counter = runtime.host.get(Counter, CounterInput {}).await.unwrap();
    assert_eq!(counter.read().await.unwrap(), initial);
    let consumer = runtime
        .host
        .get(DynamicConsumer, DynamicConsumerInput {})
        .await
        .unwrap();
    assert_eq!(consumer.run().await.unwrap(), initial + 1);
    assert_eq!(counter.read().await.unwrap(), initial + 1);
}

fn source_adapter(source: PathBuf) -> CompiledPlugin {
    fixture_runtime::catalog(source)
        .into_iter()
        .find(|adapter| adapter.release.plugin == "swarmie.local-plugin-source")
        .expect("source adapter")
}

fn native_consumer() -> CompiledPlugin {
    CompiledPlugin::new(
        release(
            "example.native-consumer",
            "1.0.0",
            PluginKind::Core,
            "example.native-consumer.facet",
        ),
        |_| {
            Ok(vec![NativeFacet::new(
                "example.native-consumer.facet",
                vec![],
                |_, setup| {
                    let invocations_definition = setup.state("invocations")?;
                    setup.serve_with_options(
                        DynamicConsumer,
                        move |_, _, declaration| {
                            Ok(NativeConsumer {
                                counter: declaration.get(Counter, CounterInput {})?,
                                invocations: declaration
                                    .state(invocations_definition.value(0_u32)?)?,
                            })
                        },
                        core::ServeOptions {
                            lifetime: core::Lifetime::Host,
                            ..Default::default()
                        },
                    )
                },
            )?])
        },
    )
}

struct NativeConsumer {
    counter: CounterRef,
    invocations: Cell<u32>,
}
impl DynamicConsumerProvider for NativeConsumer {
    fn invocations(&self) -> Readable<u32> {
        self.invocations.readable()
    }
    async fn run(&self, context: Context) -> Result<u32> {
        let value = self.counter.with_context(&context).increment(1).await?;
        self.invocations.change(&context, |count| {
            *count += 1;
            Ok(())
        })?;
        Ok(value)
    }
}

#[test]
fn profile_substitution_preserves_consumers_and_uses_actual_tembra_contracts() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let app = with_consumer();
        let baseline = start(
            driver.clone(),
            config(
                &root.path().join("native.sqlite"),
                &fixtures(),
                Some(app.clone()),
            ),
        )
        .await
        .unwrap();
        assert!(
            baseline.deployment_errors.is_empty(),
            "{:?}",
            baseline.deployment_errors
        );
        exercise(&baseline, 0).await;
        let consumer = baseline.effective.instances["counter-client"]
            .selection
            .clone();
        baseline.host.close().await.unwrap();
        drop(baseline);
        let replacement = start(
            driver,
            config(
                &root.path().join("dynamic.sqlite"),
                &fixtures(),
                Some(replace_counter(app)),
            ),
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
            consumer
        );
        assert_eq!(
            replacement.effective.instances["swarmie.counter"].origin,
            Origin::Override
        );
        exercise(&replacement, 10).await;
        replacement.host.close().await.unwrap();
    });
}

#[test]
fn native_consumer_and_dynamic_provider_are_declared_in_one_real_tembra_batch() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut app = replace_counter(fixture_runtime::application());
        app.composition.profile.plugins.push(selection(
            "native-client",
            "example.native-consumer",
            "1.0.0",
            None,
        ));
        app.composition.profile.overrides[0].config = json!({"initial":42});
        let runtime = start_with_catalog(
            driver,
            config(&root.path().join("admin.sqlite"), &fixtures(), Some(app)),
            vec![source_adapter(fixtures()), native_consumer()],
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        exercise(&runtime, 42).await;
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn actual_missing_dependency_refuses_factory_and_restart_restores_it() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("admin.sqlite");
        let mut missing = with_consumer();
        missing
            .composition
            .profile
            .disable
            .push("swarmie.counter".into());
        let failed = start(
            driver.clone(),
            config(&database, &fixtures(), Some(missing)),
        )
        .await
        .unwrap();
        assert_eq!(
            failed.effective.instances["counter-client"].state,
            SelectionState::Enabled
        );
        assert!(failed.recovery_error.is_some() || !failed.deployment_errors.is_empty());
        assert!(
            !failed
                .deployment_errors
                .contains_key("swarmie.local-plugin-source")
        );
        assert!(
            failed
                .host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .is_err()
        );
        assert!(
            !failed
                .manager
                .inventory()
                .unwrap()
                .plugins
                .iter()
                .any(|p| p.plugin_id == "dynamic-consumer" && p.observed == ObservedState::Active)
        );
        failed.host.close().await.unwrap();
        drop(failed);
        let restored = start(
            driver,
            config(&database, &fixtures(), Some(with_consumer())),
        )
        .await
        .unwrap();
        assert!(
            restored.deployment_errors.is_empty(),
            "{:?}",
            restored.deployment_errors
        );
        exercise(&restored, 0).await;
        restored.host.close().await.unwrap();
    });
}

#[tembra::contract(local, name = "swarmie.counter", version = 1)]
trait DifferentCounter {
    #[portable]
    async fn different(&self) -> Result<bool>;
}
struct DifferentCounterService;
impl DifferentCounterProvider for DifferentCounterService {
    async fn different(&self, _context: Context) -> Result<bool> {
        Ok(true)
    }
}

#[test]
fn same_nominal_contract_with_an_incompatible_fingerprint_is_rejected_by_tembra() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut app = with_consumer();
        app.composition.profile.overrides.push(selection(
            "swarmie.counter",
            "example.different-counter",
            "1.0.0",
            None,
        ));
        let different = CompiledPlugin::new(
            release(
                "example.different-counter",
                "1.0.0",
                PluginKind::Core,
                "different-counter.facet",
            ),
            |_| {
                Ok(vec![NativeFacet::new(
                    "different-counter.facet",
                    vec![],
                    |_, setup| setup.serve(DifferentCounter, |_, _, _| Ok(DifferentCounterService)),
                )?])
            },
        );
        let runtime = start_with_catalog(
            driver,
            config(&root.path().join("admin.sqlite"), &fixtures(), Some(app)),
            vec![source_adapter(fixtures()), different],
        )
        .await
        .unwrap();
        assert!(runtime.recovery_error.is_some() || !runtime.deployment_errors.is_empty());
        assert!(runtime.host.get(Counter, CounterInput {}).await.is_err());
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn failed_acquisition_source_startup_prevents_package_acquisition_until_restart() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let source = CompiledPlugin::new(
            release(
                "swarmie.local-plugin-source",
                env!("CARGO_PKG_VERSION"),
                PluginKind::Core,
                "swarmie.local-plugin-source",
            ),
            |_| {
                Ok(vec![NativeFacet::new(
                    "swarmie.local-plugin-source",
                    vec![],
                    |_, setup| {
                        setup.serve_with_options(
                            DynamicConsumer,
                            |_, _, _| {
                                Err::<NativeConsumer, _>(RuntimeError::invalid(
                                    "source_startup",
                                    "source initialization failed",
                                ))
                            },
                            core::ServeOptions {
                                lifetime: core::Lifetime::Host,
                                ..Default::default()
                            },
                        )
                    },
                )?])
            },
        )
        .with_trusted_source("local".into());
        let runtime = start_with_catalog(
            driver,
            config(
                &root.path().join("admin.sqlite"),
                &fixtures(),
                Some(replace_counter(fixture_runtime::application())),
            ),
            vec![source],
        )
        .await
        .unwrap();
        assert!(
            runtime
                .deployment_errors
                .contains_key("swarmie.local-plugin-source")
        );
        let inventory = runtime.manager.inventory().unwrap();
        assert!(inventory.plugins.iter().any(|plugin| {
            plugin.plugin_id == "swarmie.local-plugin-source"
                && plugin.observed == ObservedState::Unavailable
                && !plugin.errors.is_empty()
        }));
        assert!(
            inventory
                .plugins
                .iter()
                .all(|plugin| plugin.plugin_id != "alternative-counter")
        );
        assert!(!runtime.applied.contains_key("swarmie.local-plugin-source"));
        assert!(!runtime.applied.contains_key("swarmie.counter"));
        assert!(inventory.installed_facets.iter().any(|facet| {
            facet.facet_id == "swarmie.local-plugin-source"
                && facet.kind == swarmie_plugin_contracts::InstalledFacetKind::Native
        }));
        assert!(
            inventory
                .installed_facets
                .iter()
                .all(|facet| facet.facet_id != "alternative-counter")
        );
        let source_input = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: "swarmie.local-plugin-source".into(),
                },
            )
            .await
            .unwrap()
            .read()
            .await
            .unwrap();
        assert_eq!(source_input.instance_id, "swarmie.local-plugin-source");
        let untouched = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: "alternative-counter".into(),
                },
            )
            .await
            .unwrap();
        assert!(untouched.read().await.is_err());
        let permit = runtime.manager.begin_composition_reconciliation().unwrap();
        let result = runtime
            .manager
            .install(swarmie_plugin_contracts::InstallRequest {
                provider_id: "local".into(),
                plugin_id: "unacquired".into(),
                version: "1.0.0".into(),
                confirmed: true,
            })
            .await;
        assert!(matches!(
            result,
            Err(swarmie_plugin_manager::ManagerError::RecoveryRequired)
        ));
        drop(permit);
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn configuration_boundaries_remain_with_the_actual_provider() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut configured = replace_counter(with_consumer());
        configured.composition.profile.overrides[0].config = json!({"initial":42});
        let runtime = start(
            driver.clone(),
            config(
                &root.path().join("valid.sqlite"),
                &fixtures(),
                Some(configured),
            ),
        )
        .await
        .unwrap();
        exercise(&runtime, 42).await;
        runtime.host.close().await.unwrap();
        drop(runtime);
        let mut invalid = fixture_runtime::application();
        let mut override_selection = selection(
            "swarmie.counter",
            "swarmie.counter",
            env!("CARGO_PKG_VERSION"),
            None,
        );
        override_selection.config = json!({"initial":-1});
        invalid
            .composition
            .profile
            .overrides
            .push(override_selection);
        assert!(
            start(
                driver.clone(),
                config(
                    &root.path().join("invalid-core.sqlite"),
                    &fixtures(),
                    Some(invalid)
                )
            )
            .await
            .is_err()
        );
        for (index, value) in [json!({"initial":-1}), json!({"unexpected":true})]
            .into_iter()
            .enumerate()
        {
            let mut invalid = replace_counter(with_consumer());
            invalid.composition.profile.overrides[0].config = value;
            let runtime = start(
                driver.clone(),
                config(
                    &root.path().join(format!("invalid-dynamic-{index}.sqlite")),
                    &fixtures(),
                    Some(invalid),
                ),
            )
            .await
            .unwrap();
            assert!(runtime.recovery_error.is_some() || !runtime.deployment_errors.is_empty());
            assert!(runtime.host.get(Counter, CounterInput {}).await.is_err());
            runtime.host.close().await.unwrap();
        }
    });
}

#[test]
fn persisted_composition_replays_immutable_artifacts_offline() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        copy_fixtures(&source);
        let database = root.path().join("admin.sqlite");
        let first = start(
            driver.clone(),
            config(&database, &source, Some(replace_counter(with_consumer()))),
        )
        .await
        .unwrap();
        exercise(&first, 10).await;
        let effective = first.effective.clone();
        first.host.close().await.unwrap();
        drop(first);
        std::fs::remove_dir_all(&source).unwrap();
        let recovered = start(driver, config(&database, &source, None))
            .await
            .unwrap();
        assert_eq!(recovered.effective, effective);
        assert!(
            recovered.deployment_errors.is_empty(),
            "{:?}",
            recovered.deployment_errors
        );
        exercise(&recovered, 10).await;
        recovered.host.close().await.unwrap();
    });
}

#[test]
fn failed_provider_stage_never_replays_a_stale_cached_version() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("admin.sqlite");
        let first = start(
            driver.clone(),
            config(
                &database,
                &fixtures(),
                Some(replace_counter(with_consumer())),
            ),
        )
        .await
        .unwrap();
        exercise(&first, 10).await;
        first.host.close().await.unwrap();
        drop(first);
        let mut missing = replace_counter(with_consumer());
        missing.composition.profile.overrides[0].version = "9.9.9".into();
        missing
            .releases
            .iter_mut()
            .find(|r| r.plugin == "alternative-counter")
            .unwrap()
            .version = "9.9.9".into();
        let refused = start(driver, config(&database, &fixtures(), Some(missing)))
            .await
            .unwrap();
        assert!(refused.deployment_errors.contains_key("swarmie.counter"));
        assert!(refused.host.get(Counter, CounterInput {}).await.is_err());
        assert!(
            !refused
                .manager
                .inventory()
                .unwrap()
                .plugins
                .iter()
                .any(
                    |p| p.plugin_id == "alternative-counter" && p.observed == ObservedState::Active
                )
        );
        refused.host.close().await.unwrap();
    });
}

#[test]
fn actual_reference_retirement_and_fresh_host_reconciliation_preserve_desired_input() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("admin.sqlite");
        let runtime = start(
            driver.clone(),
            config(
                &database,
                &fixtures(),
                Some(replace_counter(with_consumer())),
            ),
        )
        .await
        .unwrap();
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
                core::ReloadOptions {
                    remove: vec!["alternative-counter".into()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(counter.core().status(), core::RefStatus::Gone);
        assert!(consumer.run().await.is_err());
        counter.core().release();
        consumer.core().release();
        runtime.host.close().await.unwrap();
        drop(runtime);
        let restored = start(driver, config(&database, &fixtures(), None))
            .await
            .unwrap();
        exercise(&restored, 10).await;
        restored.host.close().await.unwrap();
    });
}

#[test]
fn omitted_cached_consumer_cannot_veto_the_current_profiles_provider_upgrade() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        copy_fixtures(&source);
        let database = root.path().join("admin.sqlite");
        let path = source.join("dynamic-consumer/manifest.json");
        let mut consumer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        consumer["dependencies"] = json!([{"id":"alternative-counter","version":"^1.0.0"}]);
        std::fs::write(path, serde_json::to_vec(&consumer).unwrap()).unwrap();
        let successor = source.join("alternative-v2");
        std::fs::create_dir(&successor).unwrap();
        for entry in std::fs::read_dir(source.join("alternative-counter")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), successor.join(entry.file_name())).unwrap();
        }
        let path = successor.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["version"] = json!("2.0.0");
        std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let first = start(
            driver.clone(),
            config(&database, &source, Some(replace_counter(with_consumer()))),
        )
        .await
        .unwrap();
        assert!(
            first.deployment_errors.is_empty(),
            "{:?}",
            first.deployment_errors
        );
        first.host.close().await.unwrap();
        drop(first);
        let mut app = replace_counter(fixture_runtime::application());
        app.composition.profile.overrides[0].version = "2.0.0".into();
        app.composition.profile.overrides[0].config = json!({"initial":42});
        app.releases[0].version = "2.0.0".into();
        let next = start(driver, config(&database, &source, Some(app)))
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
        next.host.close().await.unwrap();
    });
}
