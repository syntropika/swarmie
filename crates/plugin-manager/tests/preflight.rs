use std::path::PathBuf;

use swarmie_counter::{DynamicConsumer, DynamicConsumerInput};
use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{CorePluginDeployment, LocalSource, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

struct FixtureConfiguration;

impl PluginConfigurationProvider for FixtureConfiguration {
    async fn read(&self, _context: Context) -> Result<ConfiguredValue> {
        Ok(ConfiguredValue {
            instance_id: "configured-counter".into(),
            configuration: serde_json::json!({}),
        })
    }
}

fn release(version: &str) -> InstallRequest {
    InstallRequest {
        plugin_id: "dynamic-consumer".into(),
        provider_id: "local".into(),
        version: version.into(),
        confirmed: true,
    }
}

#[test]
fn preflight_preserves_selection_and_approved_cache_supports_offline_install_update_and_reload() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source");
            std::fs::create_dir(&source).unwrap();
            let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/fixtures/dynamic-consumer");
            for version in ["1.0.0", "1.1.0", "1.4.0"] {
                let destination = source.join(version);
                std::fs::create_dir(&destination).unwrap();
                for entry in std::fs::read_dir(fixtures.join(version)).unwrap() {
                    let entry = entry.unwrap();
                    std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
                }
            }
            let host = Host::with_driver(RUNTIME_HOST, "preflight", driver.clone()).unwrap();
            host.install(
                swarmie_counter::facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            host.install(
                LocalSource::new("local", source.clone()).facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("administration.sqlite"),
            )
            .await
            .unwrap();
            manager.approve_trusted_source("local".into()).unwrap();
            manager
                .record_core_facets(
                    "swarmie.counter".into(),
                    "0.1.0".into(),
                    vec![swarmie_counter::FACET_ID.into()],
                )
                .await
                .unwrap();
            let before = manager.inventory().unwrap();
            assert_eq!(
                manager
                    .validate_release(release("1.0.0"))
                    .await
                    .unwrap()
                    .version,
                "1.0.0"
            );
            assert_eq!(
                manager
                    .validate_release(release("1.1.0"))
                    .await
                    .unwrap()
                    .version,
                "1.1.0"
            );
            assert!(manager.validate_release(release("1.4.0")).await.is_err());
            assert_eq!(manager.inventory().unwrap(), before);
            assert!(manager.composition().unwrap().is_none());
            assert!(
                host.get(DynamicConsumer, DynamicConsumerInput {})
                    .await
                    .is_err()
            );

            std::fs::rename(&source, directory.path().join("offline")).unwrap();
            manager.install(release("1.0.0")).await.unwrap();
            manager
                .activate(ActivateRequest {
                    plugin_id: "dynamic-consumer".into(),
                })
                .await
                .unwrap();
            let consumer = host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap();
            assert_eq!(consumer.run().await.unwrap(), 1);
            manager.reload_cached("dynamic-consumer").await.unwrap();
            assert_eq!(consumer.run().await.unwrap(), 2);
            manager
                .update(UpdateRequest {
                    plugin_id: "dynamic-consumer".into(),
                    provider_id: "local".into(),
                    version: "1.1.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            assert_eq!(consumer.run().await.unwrap(), 12);
            let installed = manager.installed_packages().unwrap();
            assert_eq!(installed.len(), 1);
            assert_eq!(installed[0].version, "1.1.0");
            consumer.core().release();
            host.close().await.unwrap();
        }));
}

#[test]
fn live_additions_declare_consumers_and_providers_together_without_resetting_unrelated_plugins() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let host = Host::with_driver(RUNTIME_HOST, "live-additions", driver.clone()).unwrap();
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("administration.sqlite"),
            )
            .await
            .unwrap();
            manager
                .install_core_batch(vec![
                    CorePluginDeployment {
                        plugin_id: "acquisition-package".into(),
                        version: "1.0.0".into(),
                        facets: vec![LocalSource::new("local", source).facet().unwrap()],
                    },
                    CorePluginDeployment {
                        plugin_id: "configuration-package".into(),
                        version: "1.0.0".into(),
                        facets: vec![
                            NativeFacet::new("fixture.configuration", vec![], |_, setup| {
                                setup.serve(PluginConfiguration, |_, _, _| Ok(FixtureConfiguration))
                            })
                            .unwrap(),
                        ],
                    },
                ])
                .await
                .unwrap();
            manager.approve_trusted_source("local".into()).unwrap();
            manager.recover_composed(vec![], &[]).await.unwrap();
            let permit = manager.begin_composition_reconciliation().unwrap();
            let selected = std::collections::BTreeSet::from([
                "dynamic-consumer".into(),
                "alternative-counter".into(),
            ]);
            manager.set_live_composition_selection(&selected).unwrap();
            manager
                .save_composition(&serde_json::json!({"profile":"live-additions"}))
                .unwrap();
            manager.install(release("1.0.0")).await.unwrap();
            manager
                .install(InstallRequest {
                    plugin_id: "alternative-counter".into(),
                    ..release("1.0.0")
                })
                .await
                .unwrap();
            // Consumer first is intentional: Tembra receives the complete declaration batch.
            let inventory = manager
                .activate_batch(&["dynamic-consumer".into(), "alternative-counter".into()])
                .await
                .unwrap();
            let consumer = host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap();
            assert_eq!(consumer.run().await.unwrap(), 11);
            assert!(
                inventory
                    .plugins
                    .iter()
                    .all(|plugin| plugin.observed == ObservedState::Active)
            );
            assert_eq!(
                manager.activate_batch(&[]).await.unwrap(),
                manager.inventory().unwrap()
            );
            consumer.core().release();
            drop(permit);
            host.close().await.unwrap();
        }));
}
