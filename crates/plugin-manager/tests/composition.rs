use swarmie_counter::{Counter, CounterInput};
use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{ManagerError, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

#[test]
fn durable_composition_requires_scoped_reconciliation_and_keeps_provider_settings_independent() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();

            let database = directory.path().join("admin.sqlite");
            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            let manager = PluginManager::start(host.clone(), driver.clone(), &database)
                .await
                .unwrap();
            let declared = serde_json::json!({"profile":{"id":"headless"}, "bundles":[]});
            assert!(matches!(
                manager.save_composition(&declared),
                Err(ManagerError::CompositionManaged)
            ));
            let permit = manager.begin_composition_reconciliation().unwrap();
            manager.save_composition(&declared).unwrap();
            manager.recover_composed(vec![], &[]).await.unwrap();
            assert!(matches!(
                manager.begin_composition_reconciliation(),
                Err(ManagerError::Busy)
            ));
            drop(permit);
            assert_eq!(manager.composition().unwrap(), Some(declared.clone()));
            let refused = manager
                .install(InstallRequest {
                    provider_id: "local".into(),
                    plugin_id: "example".into(),
                    version: "1.0.0".into(),
                    confirmed: true,
                })
                .await;
            assert!(matches!(refused, Err(ManagerError::CompositionManaged)));
            manager
                .configure_provider(ConfigureProviderRequest {
                    provider_id: "local".into(),
                    configuration: serde_json::json!("sources"),
                })
                .await
                .unwrap();
            host.close().await.unwrap();
            drop(manager);
            let restarted_host =
                Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            let restarted = PluginManager::start(restarted_host.clone(), driver.clone(), &database)
                .await
                .unwrap();
            assert_eq!(restarted.composition().unwrap(), Some(declared));
            assert!(matches!(
                restarted.recover().await,
                Err(ManagerError::CompositionManaged)
            ));
            restarted_host.close().await.unwrap();
        }));
}

#[test]
fn actual_reference_observation_overrides_administrative_success_and_can_be_restored() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();

            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            host.install(
                swarmie_counter::facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("admin.sqlite"),
            )
            .await
            .unwrap();
            manager.approve_trusted_source("local".into()).unwrap();
            manager
                .record_core_facets(
                    "counter-package".into(),
                    "1.0.0".into(),
                    vec!["swarmie.counter".into()],
                )
                .await
                .unwrap();
            let counter = host.get(Counter, CounterInput {}).await.unwrap();
            manager
                .observe_reference("counter-package", counter.core().clone())
                .unwrap();
            let observed = || {
                manager
                    .inventory()
                    .unwrap()
                    .plugins
                    .into_iter()
                    .find(|plugin| plugin.plugin_id == "counter-package")
                    .unwrap()
            };
            assert_eq!(observed().observed, ObservedState::Active);
            counter.core().release();
            assert_eq!(observed().observed, ObservedState::Unavailable);
            manager
                .observe_failure("counter-package", "required contract absent".into())
                .unwrap();
            assert!(
                observed()
                    .errors
                    .iter()
                    .any(|error| error.contains("required contract absent"))
            );
            let restored = host.get(Counter, CounterInput {}).await.unwrap();
            manager
                .observe_reference("counter-package", restored.core().clone())
                .unwrap();
            assert_eq!(observed().observed, ObservedState::Active);
            assert!(
                manager
                    .record_core_facets(
                        "other-package".into(),
                        "1.0.0".into(),
                        vec!["swarmie.counter".into()]
                    )
                    .await
                    .is_err()
            );
            restored.core().release();
            host.close().await.unwrap();
        }));
}
