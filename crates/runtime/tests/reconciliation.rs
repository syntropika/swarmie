use std::{
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use serde_json::json;
use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_memory_store::{GetRequest, GetResult, MemoryStore, MemoryStoreInput};
use swarmie_plugin_contracts::{PluginConfiguration, PluginConfigurationInput};
use swarmie_runtime::{Application, Configuration, Runtime, TransitionOutcome, start};
use tembra_native::TokioDriver;

#[path = "../../../tests/plugins/fixtures/runtime.rs"]
mod fixture;

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
            tokio::time::timeout(Duration::from_secs(45), test(driver.clone()))
                .await
                .expect("reconciliation acceptance did not settle");
        }));
}
fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures")
}
fn config(root: &Path, application: Application) -> Configuration {
    Configuration {
        database: root.join("administration.sqlite"),
        source: source(),
        application: Some(application),
    }
}
fn storage_application() -> Application {
    let mut app = Application::default();
    app.composition
        .profile
        .disable
        .push(swarmie_memory_store::PLUGIN_ID.into());
    app
}
fn add(app: &mut Application, id: &str, package: &str, version: &str, facet: &str) {
    app.composition.profile.plugins.push(Selection {
        id: id.into(),
        plugin: package.into(),
        version: version.into(),
        provider: Some("local".into()),
        config: json!({}),
    });
    app.releases.push(Release {
        plugin: package.into(),
        version: version.into(),
        kind: PluginKind::Dynamic,
        facets: vec![FacetIdentity {
            id: facet.into(),
            target: FacetTarget::Server,
        }],
    });
}
async fn initial(runtime: &Runtime) -> GetResult {
    runtime
        .host
        .get(MemoryStore, MemoryStoreInput {})
        .await
        .unwrap()
        .get(GetRequest {
            namespace: "configuration".into(),
            key: "initial".into(),
        })
        .await
        .unwrap()
}
fn replace_version(app: &mut Application, version: &str) {
    app.composition
        .profile
        .plugins
        .iter_mut()
        .find(|selection| selection.id == "consumer")
        .unwrap()
        .version = version.into();
    app.releases
        .iter_mut()
        .find(|release| release.plugin == "dynamic-consumer")
        .unwrap()
        .version = version.into();
}

#[test]
fn live_dynamic_add_configuration_refusal_and_retirement_keep_acknowledged_input_separate() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let base = storage_application();
        let mut runtime = start(driver.clone(), config(directory.path(), base.clone()))
            .await
            .unwrap();
        let mut next = base;
        add(
            &mut next,
            "scratch",
            "dynamic-memory-store",
            "1.0.0",
            "dynamic-memory-store",
        );
        next.composition.profile.plugins[0].config = json!({"initial":42});
        let plan = runtime.inspect_application(&next).unwrap();
        assert_eq!(
            plan.changes
                .iter()
                .find(|change| change.instance_id == "scratch")
                .unwrap()
                .kind,
            swarmie_runtime::ChangeKind::Added
        );
        assert!(
            runtime
                .manager
                .inventory()
                .unwrap()
                .plugins
                .iter()
                .all(|plugin| plugin.plugin_id != "dynamic-memory-store")
        );
        let report = runtime.apply_application(next.clone()).await.unwrap();
        assert_eq!(
            report.results["scratch"].outcome,
            TransitionOutcome::Applied,
            "{report:?}"
        );
        let reference = runtime
            .host
            .get(MemoryStore, MemoryStoreInput {})
            .await
            .unwrap();
        assert_eq!(
            initial(&runtime).await,
            GetResult::Present { value: json!(42) }
        );
        let delivery = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: "dynamic-memory-store".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(delivery.read().await.unwrap().instance_id, "scratch");
        let epoch = runtime.host.core().code_version();
        next.composition.profile.plugins[0].config = json!({"initial":{"revised":true}});
        let report = runtime.apply_application(next.clone()).await.unwrap();
        assert_eq!(
            report.results["scratch"].outcome,
            TransitionOutcome::Applied,
            "{report:?}"
        );
        assert_eq!(
            runtime.host.core().code_version(),
            epoch,
            "configuration uses owning Tembra State, not a fabricated reload"
        );
        assert_eq!(
            reference
                .get(GetRequest {
                    namespace: "configuration".into(),
                    key: "initial".into()
                })
                .await
                .unwrap(),
            GetResult::Present {
                value: json!({"revised":true})
            }
        );
        assert_eq!(
            delivery.read().await.unwrap().configuration,
            json!({"initial":{"revised":true}})
        );
        let desired = runtime.desired_application().clone();
        let mut invalid = next.clone();
        invalid.composition.profile.plugins[0].config = json!({"unexpected":1});
        let report = runtime.apply_application(invalid).await.unwrap();
        assert!(!report.desired_persisted);
        assert_eq!(report.results["scratch"].outcome, TransitionOutcome::Failed);
        assert_eq!(runtime.desired_application(), &desired);
        assert_eq!(
            initial(&runtime).await,
            GetResult::Present {
                value: json!({"revised":true})
            }
        );
        next.composition.profile.disable.push("scratch".into());
        let report = runtime.apply_application(next.clone()).await.unwrap();
        assert_eq!(
            report.results["scratch"].outcome,
            TransitionOutcome::Applied,
            "{report:?}"
        );
        assert!(
            reference
                .get(GetRequest {
                    namespace: "configuration".into(),
                    key: "initial".into()
                })
                .await
                .is_err()
        );
        assert!(delivery.read().await.is_err());
        runtime.host.close().await.unwrap();
        drop(runtime);
        let recovered = start(
            driver,
            Configuration {
                database: directory.path().join("administration.sqlite"),
                source: source(),
                application: None,
            },
        )
        .await
        .unwrap();
        assert!(!recovered.applied.contains_key("scratch"));
        assert!(
            recovered
                .host
                .get(MemoryStore, MemoryStoreInput {})
                .await
                .is_err()
        );
        recovered.host.close().await.unwrap();
    });
}

#[test]
fn dynamic_reload_retains_references_while_native_reconfiguration_requires_restart() {
    use swarmie_counter::{Counter, CounterInput, DynamicConsumer, DynamicConsumerInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let mut app = fixture::application();
        add(
            &mut app,
            "consumer",
            "dynamic-consumer",
            "1.0.0",
            "dynamic-consumer",
        );
        let mut runtime = fixture::start(driver, config(directory.path(), app.clone()))
            .await
            .unwrap();
        let counter = runtime.host.get(Counter, CounterInput {}).await.unwrap();
        let consumer = runtime
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.run().await.unwrap(), 1);
        replace_version(&mut app, "1.1.0");
        app.composition.profile.overrides.push(Selection {
            id: "swarmie.counter".into(),
            plugin: "swarmie.counter".into(),
            version: "0.1.0".into(),
            provider: None,
            config: json!({"initial":99}),
        });
        let report = runtime.apply_application(app.clone()).await.unwrap();
        assert_eq!(
            report.results["consumer"].outcome,
            TransitionOutcome::Applied,
            "{report:?}"
        );
        assert_eq!(
            report.results["swarmie.counter"].outcome,
            TransitionOutcome::RestartRequired
        );
        assert_eq!(
            runtime.applied["swarmie.counter"].selection.config,
            json!({})
        );
        assert_eq!(consumer.run().await.unwrap(), 11);
        assert_eq!(counter.read().await.unwrap(), 11);
        let current = runtime.applied["consumer"].clone();
        replace_version(&mut app, "1.2.0");
        let report = runtime.apply_application(app).await.unwrap();
        assert_eq!(
            report.results["consumer"].outcome,
            TransitionOutcome::Failed,
            "{report:?}"
        );
        assert_eq!(runtime.applied["consumer"], current);
        assert_eq!(consumer.run().await.unwrap(), 21);
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn package_preflight_failure_never_publishes_desired_intent_or_mutates_existing_capabilities() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let app = storage_application();
        let mut runtime = start(driver, config(directory.path(), app.clone()))
            .await
            .unwrap();
        let inventory = runtime.manager.inventory().unwrap();
        let mut missing = app.clone();
        add(
            &mut missing,
            "missing",
            "dynamic-memory-store",
            "9.9.9",
            "dynamic-memory-store",
        );
        let report = runtime.apply_application(missing).await.unwrap();
        assert!(!report.desired_persisted);
        assert_eq!(report.results["missing"].outcome, TransitionOutcome::Failed);
        assert_eq!(runtime.desired_application(), &app);
        assert_eq!(runtime.manager.inventory().unwrap(), inventory);
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn configuration_publication_failure_is_uncertain_and_requires_recovery() {
    use swarmie_counter::{ConfigurationControl, ConfigurationControlInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let mut app = storage_application();
        add(
            &mut app,
            "scratch",
            "dynamic-memory-store",
            "1.1.0",
            "dynamic-memory-store",
        );
        let mut runtime = start(driver, config(directory.path(), app.clone()))
            .await
            .unwrap();
        let owner = runtime
            .host
            .get(ConfigurationControl, ConfigurationControlInput {})
            .await
            .unwrap();
        app.composition.profile.plugins[0].config = json!({"initial":17});
        let report = runtime.apply_application(app.clone()).await.unwrap();
        assert!(report.recovery_required, "{report:?}");
        assert_eq!(
            report.results["scratch"].outcome,
            TransitionOutcome::RecoveryRequired
        );
        assert!(
            report.results["scratch"]
                .diagnostic
                .contains("injected_configuration_apply_failure")
        );
        assert_eq!(runtime.applied["scratch"].selection.config, json!({}));
        // The owning State did commit before its observer failed. An error never proves rollback.
        assert_eq!(
            owner.configuration().value().unwrap(),
            json!({"initial":17})
        );
        let next = runtime.apply_application(app).await.unwrap();
        assert_eq!(
            next.results["scratch"].outcome,
            TransitionOutcome::RecoveryRequired
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn failed_foreign_addition_can_be_reconciled_to_a_valid_release_without_restarting() {
    use swarmie_counter::{DynamicConsumer, DynamicConsumerInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let mut app = fixture::application();
        let mut runtime = fixture::start(driver, config(directory.path(), app.clone()))
            .await
            .unwrap();
        add(
            &mut app,
            "consumer",
            "dynamic-consumer",
            "1.2.0",
            "dynamic-consumer",
        );
        let failed = runtime.apply_application(app.clone()).await.unwrap();
        assert_eq!(
            failed.results["consumer"].outcome,
            TransitionOutcome::Failed,
            "{failed:?}"
        );
        assert!(
            !failed.recovery_required,
            "foreign work was logically retired"
        );
        assert!(!runtime.applied.contains_key("consumer"));
        replace_version(&mut app, "1.1.0");
        let repaired = runtime.apply_application(app).await.unwrap();
        assert_eq!(
            repaired.results["consumer"].outcome,
            TransitionOutcome::Applied,
            "{repaired:?}"
        );
        assert_eq!(
            runtime
                .host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap()
                .run()
                .await
                .unwrap(),
            10
        );
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn a_later_configuration_failure_does_not_publish_an_earlier_unactivated_addition() {
    use swarmie_counter::{DynamicConsumer, DynamicConsumerInput};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let mut app = fixture::application();
        add(
            &mut app,
            "z-store",
            "dynamic-memory-store",
            "1.1.0",
            "dynamic-memory-store",
        );
        let mut runtime = fixture::start(driver.clone(), config(directory.path(), app.clone()))
            .await
            .unwrap();
        add(
            &mut app,
            "a-consumer",
            "dynamic-consumer",
            "1.0.0",
            "dynamic-consumer",
        );
        app.composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "z-store")
            .unwrap()
            .config = json!({"initial":23});
        let report = runtime.apply_application(app).await.unwrap();
        assert!(report.desired_persisted && report.recovery_required);
        assert_eq!(
            report.results["a-consumer"].outcome,
            TransitionOutcome::RecoveryRequired
        );
        assert!(!runtime.applied.contains_key("a-consumer"));
        assert!(
            runtime
                .host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .is_err()
        );
        let input = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: "dynamic-consumer".into(),
                },
            )
            .await
            .unwrap();
        assert!(
            input.read().await.is_err(),
            "unacknowledged candidate input was withdrawn"
        );
        runtime.host.close().await.unwrap();
        drop(runtime);
        let recovered = fixture::start(
            driver,
            Configuration {
                database: directory.path().join("administration.sqlite"),
                source: source(),
                application: None,
            },
        )
        .await
        .unwrap();
        assert!(
            recovered.recovery_error.is_none(),
            "{:?}",
            recovered.recovery_error
        );
        assert_eq!(
            recovered
                .host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap()
                .run()
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            initial(&recovered).await,
            GetResult::Present { value: json!(23) }
        );
        recovered.host.close().await.unwrap();
    });
}
