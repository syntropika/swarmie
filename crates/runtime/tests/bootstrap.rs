use swarmie_plugin_contracts::{
    ActivateRequest, Counter, CounterInput, DynamicConsumer, DynamicConsumerInput, InstallRequest,
};
use swarmie_runtime::{Configuration, start};
use tembra_native::TokioDriver;

#[test]
fn headless_bootstrap_recovers_cached_code_without_its_source() {
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
            let configuration = || Configuration {
                database: directory.path().join("manager.sqlite"),
                source: source.clone(),
                application: None,
            };
            let first = start(driver.clone(), configuration()).await.unwrap();
            first
                .manager
                .install(InstallRequest {
                    provider_id: "local".into(),
                    plugin_id: "dynamic-consumer".into(),
                    version: "1.0.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            first
                .manager
                .activate(ActivateRequest {
                    plugin_id: "dynamic-consumer".into(),
                })
                .await
                .unwrap();
            first.host.close().await.unwrap();
            drop(first);
            std::fs::remove_dir_all(&source).unwrap();
            let second = start(driver, configuration()).await.unwrap();
            let inventory = second.manager.inventory().unwrap();
            assert_eq!(inventory.plugins.len(), 4);
            let consumer = second
                .host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap();
            consumer.run().await.unwrap();
            let counter = second.host.get(Counter, CounterInput {}).await.unwrap();
            assert_eq!(counter.read().await.unwrap(), 1);
            second.host.close().await.unwrap();
        }));
}

#[test]
fn failed_cached_setup_keeps_administration_available_for_repair() {
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
            let configuration = || Configuration {
                database: directory.path().join("manager.sqlite"),
                source: source.clone(),
                application: None,
            };
            let first = start(driver.clone(), configuration()).await.unwrap();
            first
                .manager
                .install(InstallRequest {
                    provider_id: "local".into(),
                    plugin_id: "dynamic-consumer".into(),
                    version: "1.2.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            assert!(
                first
                    .manager
                    .activate(ActivateRequest {
                        plugin_id: "dynamic-consumer".into(),
                    })
                    .await
                    .is_err()
            );
            first.host.close().await.unwrap();
            drop(first);
            let second = start(driver, configuration()).await.unwrap();
            assert!(second.recovery_error.is_some());
            let inventory = second.manager.inventory().unwrap();
            assert!(
                inventory
                    .plugins
                    .iter()
                    .any(|plugin| plugin.plugin_id == "dynamic-consumer"
                        && plugin.observed == swarmie_plugin_contracts::ObservedState::Failed)
            );
            second
                .manager
                .deactivate(swarmie_plugin_contracts::DeactivateRequest {
                    plugin_id: "dynamic-consumer".into(),
                })
                .await
                .unwrap();
            second
                .manager
                .uninstall(swarmie_plugin_contracts::UninstallRequest {
                    plugin_id: "dynamic-consumer".into(),
                })
                .await
                .unwrap();
            second.host.close().await.unwrap();
        }));
}
