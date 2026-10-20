//! Real JavaScript packages replace a provider while an unchanged compiled consumer holds it.

use std::{collections::BTreeMap, future::Future, path::PathBuf, rc::Rc, time::Duration};

use serde_json::json;
use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_counter::{
    Counter, CounterInput, CounterProvider, DynamicConsumer, DynamicConsumerInput,
    DynamicConsumerRef,
};
use swarmie_plugin_contracts::{
    ObservedState, OperationStatus, PluginAdministration, PluginAdministrationInput,
    PluginConfiguration, PluginConfigurationInput,
};
use swarmie_runtime::{
    Application, ChangeKind, CompiledPlugin, Configuration, Runtime, TransitionOutcome,
    compiled_plugins, start_with_catalog,
};
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

const PROVIDER_A: &str = "counter-provider-a";
const PROVIDER_B: &str = "counter-provider-b";
const INCOMPATIBLE: &str = "counter-provider-incompatible";
const FAILING: &str = "counter-provider-failing";
const CONSUMER: &str = "stable-provider-consumer";
const CONSUMER_FACET: &str = "stable-provider-consumer.runtime";

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
                .expect("provider replacement acceptance did not settle");
        }));
}

fn fixture_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    for package in [PROVIDER_A, PROVIDER_B, INCOMPATIBLE, FAILING] {
        assert!(
            root.join(package).join("1.0.0/manifest.json").is_file(),
            "build real provider artifacts with just tembra-plugins"
        );
    }
    root
}

fn facet(package: &str) -> String {
    format!("{package}.runtime")
}

fn release(package: &str, kind: PluginKind) -> Release {
    Release {
        plugin: package.into(),
        version: "1.0.0".into(),
        kind,
        facets: vec![FacetIdentity {
            id: facet(package),
            target: FacetTarget::Server,
        }],
    }
}

fn application(package: &str) -> Application {
    let mut app = Application::default();
    app.composition
        .profile
        .disable
        .push(swarmie_memory_store::PLUGIN_ID.into());
    app.composition.profile.plugins.extend([
        Selection {
            id: "provider".into(),
            plugin: package.into(),
            version: "1.0.0".into(),
            provider: Some("local".into()),
            config: json!({"marker":"shared"}),
        },
        Selection {
            id: "consumer".into(),
            plugin: CONSUMER.into(),
            version: "1.0.0".into(),
            provider: None,
            config: json!({}),
        },
    ]);
    app.releases.push(release(package, PluginKind::Dynamic));
    app
}

fn compiled() -> Vec<CompiledPlugin> {
    let mut catalog = compiled_plugins(fixture_root());
    catalog.push(CompiledPlugin::new(
        release(CONSUMER, PluginKind::Core),
        |value| {
            if value != &json!({}) {
                return Err(RuntimeError::invalid(
                    "consumer_configuration",
                    "no settings are supported",
                ));
            }
            Ok(vec![NativeFacet::new(
                CONSUMER_FACET,
                vec![],
                |_, setup| {
                    setup.serve_with_options(
                        Counter,
                        |_, _, declaration| {
                            Ok(StableConsumer {
                                provider: declaration
                                    .get(DynamicConsumer, DynamicConsumerInput {})?,
                                calls: declaration.state("calls", 0_u32)?,
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
    ));
    catalog
}

struct StableConsumer {
    provider: DynamicConsumerRef,
    calls: Cell<u32>,
}

impl CounterProvider for StableConsumer {
    fn count(&self) -> Readable<u32> {
        self.calls.readable()
    }
    async fn increment(&self, by: u32, context: Context) -> Result<u32> {
        let candidate = self.calls.value()?.checked_add(by).ok_or_else(|| {
            RuntimeError::invalid("counter_overflow", "consumer invocation count overflow")
        })?;
        for _ in 0..by {
            self.provider.with_context(&context).run().await?;
        }
        self.calls.replace(&context, candidate)?;
        self.calls.value()
    }
    async fn read(&self, _context: Context) -> Result<u32> {
        self.calls.value()
    }
}

fn configuration(root: &std::path::Path, application: Option<Application>) -> Configuration {
    Configuration {
        database: root.join("administration.sqlite"),
        source: fixture_root(),
        application,
    }
}

fn actual_facets(runtime: &Runtime) -> BTreeMap<String, core::FacetKind> {
    runtime
        .host
        .core()
        .installed_facets()
        .into_iter()
        .map(|facet| (facet.name().to_owned(), facet.kind()))
        .collect()
}

fn registered_provider(runtime: &Runtime) -> serde_json::Value {
    runtime
        .host
        .core()
        .registry(
            &core::Registry::new(
                swarmie_counter::CAPABILITY_REGISTRY,
                core::ConflictPolicy::Error,
            )
            .unwrap(),
        )
        .unwrap()
        .table(None)
        .unwrap()
        .get("replacement-provider/run")
        .expect("lifecycle-owned provider contribution")
        .data()
        .clone()
}

async fn state_value(state: Readable<u32>, expected: u32) {
    use std::cell::RefCell;
    let (sender, receiver) = futures::channel::oneshot::channel();
    let sender = RefCell::new(Some(sender));
    let _subscription = state
        .subscribe(move |snapshot| {
            if snapshot.value()? == expected
                && let Some(sender) = sender.borrow_mut().take()
            {
                let _ = sender.send(());
            }
            Ok(())
        })
        .unwrap();
    receiver.await.expect("state observation");
}

#[test]
fn one_cross_package_transaction_retains_references_states_and_the_unchanged_consumer() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = start_with_catalog(
            driver,
            configuration(root.path(), Some(application(PROVIDER_A))),
            compiled(),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        let consumer = runtime.host.get(Counter, CounterInput {}).await.unwrap();
        let provider = runtime
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let consumer_id = consumer.core().object_id();
        let provider_id = provider.core().object_id();
        let old_configuration = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: facet(PROVIDER_A),
                },
            )
            .await
            .unwrap();
        let input = old_configuration.read().await.unwrap();
        assert_eq!(input.instance_id, "provider");
        assert_eq!(input.configuration, json!({"marker":"shared"}));
        let latest = Rc::new(std::cell::Cell::new(0));
        let observed = latest.clone();
        let _subscription = provider
            .invocations()
            .subscribe(move |snapshot| {
                observed.set(snapshot.value()?);
                Ok(())
            })
            .unwrap();
        assert_eq!(consumer.increment(1).await.unwrap(), 1);
        state_value(provider.invocations(), 1).await;
        let before = actual_facets(&runtime);
        let plan = runtime
            .inspect_application(&application(PROVIDER_B))
            .unwrap();
        assert_eq!(
            plan.changes
                .iter()
                .find(|change| change.instance_id == "provider")
                .unwrap()
                .kind,
            ChangeKind::Replaced
        );
        assert_eq!(
            plan.changes
                .iter()
                .find(|change| change.instance_id == "consumer")
                .unwrap()
                .kind,
            ChangeKind::Unchanged
        );
        let report = runtime
            .apply_application(application(PROVIDER_B))
            .await
            .unwrap();
        assert!(
            report.desired_persisted && !report.recovery_required,
            "{report:?}"
        );
        assert_eq!(
            report.results["provider"].outcome,
            TransitionOutcome::Applied,
            "{report:?}"
        );
        assert_eq!(
            report.results["consumer"].outcome,
            TransitionOutcome::Unchanged
        );
        assert_eq!(consumer.core().object_id(), consumer_id);
        assert_eq!(provider.core().object_id(), provider_id);
        assert_eq!(consumer.core().status(), core::RefStatus::Live);
        assert_eq!(provider.core().status(), core::RefStatus::Live);
        state_value(provider.invocations(), 1).await;
        assert_eq!(consumer.increment(1).await.unwrap(), 2);
        state_value(provider.invocations(), 11).await;
        assert_eq!(provider.run().await.unwrap(), 21);
        state_value(provider.invocations(), 21).await;
        assert_eq!(latest.get(), 21);
        assert_eq!(consumer.count().value().unwrap(), 2);
        assert!(old_configuration.read().await.is_err());
        let configuration = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: facet(PROVIDER_B),
                },
            )
            .await
            .unwrap();
        assert_eq!(configuration.read().await.unwrap(), input);
        let mut expected = before;
        assert_eq!(
            expected.remove(&facet(PROVIDER_A)),
            Some(core::FacetKind::Foreign)
        );
        expected.insert(facet(PROVIDER_B), core::FacetKind::Foreign);
        assert_eq!(actual_facets(&runtime), expected);
        assert_eq!(registered_provider(&runtime)["facet_id"], facet(PROVIDER_B));
        let inventory = runtime.manager.inventory().unwrap();
        assert!(inventory.plugins.iter().any(
            |record| record.plugin_id == PROVIDER_A && record.observed != ObservedState::Active
        ));
        assert!(inventory.plugins.iter().any(
            |record| record.plugin_id == PROVIDER_B && record.observed == ObservedState::Active
        ));
        let desired: Application =
            serde_json::from_value(runtime.manager.composition().unwrap().unwrap()).unwrap();
        assert_eq!(desired, application(PROVIDER_B));
        assert_eq!(runtime.applied["provider"].selection.plugin, PROVIDER_B);
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn incompatible_and_failed_candidate_factories_preserve_old_bindings_and_acknowledgement() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = start_with_catalog(
            driver,
            configuration(root.path(), Some(application(PROVIDER_A))),
            compiled(),
        )
        .await
        .unwrap();
        let consumer = runtime.host.get(Counter, CounterInput {}).await.unwrap();
        let provider = runtime
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let old_configuration = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: facet(PROVIDER_A),
                },
            )
            .await
            .unwrap();
        let old_input = old_configuration.read().await.unwrap();
        let actual = actual_facets(&runtime);
        let acknowledged = runtime.applied["provider"].clone();
        for (index, candidate) in [INCOMPATIBLE, FAILING].into_iter().enumerate() {
            let epoch = runtime.host.core().code_version();
            let report = runtime
                .apply_application(application(candidate))
                .await
                .unwrap();
            assert!(
                report.desired_persisted && !report.recovery_required,
                "{report:?}"
            );
            assert_eq!(
                report.results["provider"].outcome,
                TransitionOutcome::Failed,
                "{report:?}"
            );
            let expected_error = if candidate == INCOMPATIBLE {
                "contract_mismatch"
            } else {
                "injected_replacement_provider_factory_failure"
            };
            assert!(
                report.results["provider"]
                    .diagnostic
                    .contains(expected_error),
                "{report:?}"
            );
            assert_eq!(runtime.applied["provider"], acknowledged);
            assert_eq!(runtime.host.core().code_version(), epoch);
            assert_eq!(actual_facets(&runtime), actual);
            assert_eq!(registered_provider(&runtime)["facet_id"], facet(PROVIDER_A));
            assert_eq!(consumer.core().status(), core::RefStatus::Live);
            assert_eq!(provider.core().status(), core::RefStatus::Live);
            assert_eq!(old_configuration.read().await.unwrap(), old_input);
            let candidate_input = runtime
                .host
                .get(
                    PluginConfiguration,
                    PluginConfigurationInput {
                        facet_id: facet(candidate),
                    },
                )
                .await
                .unwrap();
            assert!(candidate_input.read().await.is_err());
            assert_eq!(consumer.increment(1).await.unwrap(), (index + 1) as u32);
            state_value(provider.invocations(), (index + 1) as u32).await;
            let inventory = runtime.manager.inventory().unwrap();
            assert!(
                inventory
                    .plugins
                    .iter()
                    .any(|record| record.plugin_id == PROVIDER_A
                        && record.observed == ObservedState::Active)
            );
            assert!(
                inventory
                    .plugins
                    .iter()
                    .all(|record| record.plugin_id != candidate
                        || record.observed != ObservedState::Active)
            );
            assert!(
                inventory
                    .operations
                    .iter()
                    .any(|operation| operation.status == OperationStatus::Failed)
            );
            let desired: Application =
                serde_json::from_value(runtime.manager.composition().unwrap().unwrap()).unwrap();
            assert_eq!(desired, application(candidate));
        }
        runtime.host.close().await.unwrap();
    });
}

#[test]
fn restart_uses_the_failed_replacements_desired_package_without_resurrecting_the_old_provider() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut first = start_with_catalog(
            driver.clone(),
            configuration(root.path(), Some(application(PROVIDER_A))),
            compiled(),
        )
        .await
        .unwrap();
        let held = first
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(held.run().await.unwrap(), 1);
        let report = first.apply_application(application(FAILING)).await.unwrap();
        assert_eq!(
            report.results["provider"].outcome,
            TransitionOutcome::Failed,
            "{report:?}"
        );
        assert_eq!(first.applied["provider"].selection.plugin, PROVIDER_A);
        assert_eq!(held.run().await.unwrap(), 2);
        first.host.close().await.unwrap();
        assert!(held.run().await.is_err());
        drop(first);
        let second =
            start_with_catalog(driver.clone(), configuration(root.path(), None), compiled())
                .await
                .unwrap();
        assert!(second.recovery_error.is_some() || !second.deployment_errors.is_empty());
        assert_eq!(second.desired_application(), &application(FAILING));
        assert!(!actual_facets(&second).contains_key(&facet(PROVIDER_A)));
        assert!(
            second
                .manager
                .inventory()
                .unwrap()
                .plugins
                .iter()
                .all(|record| record.plugin_id != PROVIDER_A
                    || record.observed != ObservedState::Active)
        );
        let administration = second
            .host
            .get(PluginAdministration, PluginAdministrationInput {})
            .await
            .unwrap();
        administration.inventory().await.unwrap();
        second.host.close().await.unwrap();
        drop(second);
        let third = start_with_catalog(
            driver,
            configuration(root.path(), Some(application(PROVIDER_B))),
            compiled(),
        )
        .await
        .unwrap();
        assert!(third.recovery_error.is_none(), "{:?}", third.recovery_error);
        assert!(
            third.deployment_errors.is_empty(),
            "{:?}",
            third.deployment_errors
        );
        assert!(!actual_facets(&third).contains_key(&facet(PROVIDER_A)));
        let consumer = third.host.get(Counter, CounterInput {}).await.unwrap();
        let provider = third
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.increment(1).await.unwrap(), 1);
        state_value(provider.invocations(), 10).await;
        third.host.close().await.unwrap();
    });
}

#[test]
fn same_package_code_replacement_can_change_its_fixed_facet_with_unchanged_configuration() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = start_with_catalog(
            driver,
            configuration(root.path(), Some(application(PROVIDER_A))),
            compiled(),
        )
        .await
        .unwrap();
        let consumer = runtime.host.get(Counter, CounterInput {}).await.unwrap();
        let provider = runtime
            .host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let old_input = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: facet(PROVIDER_A),
                },
            )
            .await
            .unwrap();
        let input = old_input.read().await.unwrap();
        let provider_id = provider.core().object_id();
        let consumer_id = consumer.core().object_id();
        assert_eq!(consumer.increment(1).await.unwrap(), 1);
        state_value(provider.invocations(), 1).await;
        let mut next = application(PROVIDER_A);
        next.composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "provider")
            .unwrap()
            .version = "1.1.0".into();
        let release = next
            .releases
            .iter_mut()
            .find(|release| release.plugin == PROVIDER_A)
            .unwrap();
        release.version = "1.1.0".into();
        release.facets[0].id = facet(PROVIDER_B);
        let report = runtime.apply_application(next).await.unwrap();
        assert!(
            report.desired_persisted && !report.recovery_required,
            "{report:?}"
        );
        assert_eq!(
            report.results["provider"].outcome,
            TransitionOutcome::Applied,
            "{report:?}"
        );
        assert_eq!(consumer.core().object_id(), consumer_id);
        assert_eq!(provider.core().object_id(), provider_id);
        assert_eq!(consumer.core().status(), core::RefStatus::Live);
        assert_eq!(provider.core().status(), core::RefStatus::Live);
        assert_eq!(consumer.increment(1).await.unwrap(), 2);
        state_value(provider.invocations(), 11).await;
        assert!(old_input.read().await.is_err());
        let new_input = runtime
            .host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: facet(PROVIDER_B),
                },
            )
            .await
            .unwrap();
        assert_eq!(new_input.read().await.unwrap(), input);
        let actual = actual_facets(&runtime);
        assert!(!actual.contains_key(&facet(PROVIDER_A)));
        assert_eq!(actual[&facet(PROVIDER_B)], core::FacetKind::Foreign);
        assert_eq!(registered_provider(&runtime)["facet_id"], facet(PROVIDER_B));
        assert_eq!(runtime.applied["provider"].selection.plugin, PROVIDER_A);
        assert_eq!(runtime.applied["provider"].selection.version, "1.1.0");
        assert!(
            runtime
                .manager
                .inventory()
                .unwrap()
                .plugins
                .iter()
                .any(|record| {
                    record.plugin_id == PROVIDER_A
                        && record.version == "1.1.0"
                        && record.observed == ObservedState::Active
                })
        );
        runtime.host.close().await.unwrap();
    });
}
