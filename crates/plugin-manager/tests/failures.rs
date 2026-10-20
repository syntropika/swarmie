use std::{future::Future, path::PathBuf, rc::Rc};

use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{LocalSource, ManagerError, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

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
        .block_on(driver.run_until(test(driver.clone())));
}

async fn active_node(
    driver: Rc<TokioDriver>,
    directory: &std::path::Path,
) -> (Host, Rc<PluginManager>, AdminProof) {
    let proof = PluginManager::issue_proof();
    let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
    host.install(
        swarmie_counter::facet().unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    host.install(
        LocalSource::new(
            "local",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures"),
        )
        .facet()
        .unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    let manager = PluginManager::start(
        host.clone(),
        driver,
        directory.join("admin.sqlite"),
        proof.clone(),
    )
    .await
    .unwrap();
    manager
        .record_core(&proof, "swarmie.counter".into(), "0.1.0".into())
        .await
        .unwrap();
    manager
        .install(InstallRequest {
            proof: proof.clone(),
            provider_id: "local".into(),
            plugin_id: "dynamic-consumer".into(),
            version: "1.0.0".into(),
            confirmed: true,
        })
        .await
        .unwrap();
    manager
        .activate(ActivateRequest {
            proof: proof.clone(),
            plugin_id: "dynamic-consumer".into(),
        })
        .await
        .unwrap();
    (host, manager, proof)
}

fn replacement(proof: &AdminProof, version: &str) -> UpdateRequest {
    UpdateRequest {
        proof: proof.clone(),
        provider_id: "local".into(),
        plugin_id: "dynamic-consumer".into(),
        version: version.into(),
        confirmed: true,
    }
}

#[test]
fn actual_reload_refuses_incompatible_provider_and_module_evaluation_failure() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, proof) = active_node(driver, directory.path()).await;
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.run().await.unwrap(), 1);
        assert!(manager.update(replacement(&proof, "1.7.0")).await.is_err());
        assert_eq!(consumer.run().await.unwrap(), 2);
        let failure = manager
            .update(replacement(&proof, "1.4.0"))
            .await
            .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("Candidate module evaluation failed"),
            "{failure}"
        );
        assert_eq!(consumer.run().await.unwrap(), 3);
        let plugin = manager
            .inventory(&proof)
            .unwrap()
            .plugins
            .into_iter()
            .find(|plugin| plugin.plugin_id == "dynamic-consumer")
            .unwrap();
        assert_eq!(plugin.version, "1.0.0");
        assert_eq!(plugin.observed, ObservedState::Active);
        consumer.core().release();
        host.close().await.unwrap();
    });
}

#[test]
fn exhausted_executor_budget_is_reported_without_claiming_a_successful_update() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, proof) = active_node(driver, directory.path()).await;
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let mut failure = None;
        for _ in 0..33 {
            if let Err(error) = manager.update(replacement(&proof, "1.0.0")).await {
                failure = Some(error);
                break;
            }
        }
        let failure = failure.expect("executor admission budget must refuse replacement");
        let ManagerError::Runtime(error) = failure else {
            panic!("unexpected budget error: {failure:?}")
        };
        assert_eq!(error.reason, "rotation_required");
        assert_eq!(consumer.run().await.unwrap(), 1);
        let inventory = manager.inventory(&proof).unwrap();
        assert_eq!(
            inventory.operations.last().unwrap().status,
            OperationStatus::Failed
        );
        assert!(
            inventory
                .operations
                .last()
                .unwrap()
                .error
                .contains("rotation_required")
        );
        consumer.core().release();
        host.close().await.unwrap();
    });
}

#[test]
fn sqlite_failure_after_runtime_publication_never_claims_old_code_is_still_active() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, proof) = active_node(driver, directory.path()).await;
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.run().await.unwrap(), 1);
        let connection = rusqlite::Connection::open(directory.path().join("admin.sqlite")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_selection BEFORE UPDATE OF version ON installed_plugins
            WHEN NEW.version='1.1.0' BEGIN SELECT RAISE(ABORT,'injected selection failure'); END;",
            )
            .unwrap();
        let failure = manager
            .update(replacement(&proof, "1.1.0"))
            .await
            .unwrap_err();
        assert!(
            matches!(failure, ManagerError::PublicationUnrecorded { .. }),
            "{failure:?}"
        );
        assert_eq!(consumer.run().await.unwrap(), 11);
        let plugin = manager
            .inventory(&proof)
            .unwrap()
            .plugins
            .into_iter()
            .find(|plugin| plugin.plugin_id == "dynamic-consumer")
            .unwrap();
        assert_eq!(plugin.version, "1.1.0");
        assert_eq!(plugin.observed, ObservedState::Unavailable);
        assert!(plugin.errors[0].contains("runtime publication committed"));
        let selected: String = connection
            .query_row(
                "SELECT version FROM installed_plugins WHERE plugin_id='dynamic-consumer'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(selected, "1.0.0");
        assert!(matches!(
            manager.active_package(&proof, "dynamic-consumer"),
            Err(ManagerError::RecoveryRequired)
        ));
        assert!(matches!(
            manager
                .deactivate(DeactivateRequest {
                    proof: proof.clone(),
                    plugin_id: "dynamic-consumer".into()
                })
                .await,
            Err(ManagerError::RecoveryRequired)
        ));
        consumer.core().release();
        drop(connection);
        host.close().await.unwrap();
    });
}

#[test]
fn portable_argument_and_result_refusals_preserve_native_counter() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, proof) = active_node(driver, directory.path()).await;
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let counter = host.get(Counter, CounterInput {}).await.unwrap();
        assert_eq!(counter.read().await.unwrap(), 0);
        manager.update(replacement(&proof, "1.5.0")).await.unwrap();
        assert!(consumer.run().await.is_err());
        assert_eq!(counter.read().await.unwrap(), 0);
        manager.update(replacement(&proof, "1.6.0")).await.unwrap();
        assert!(consumer.run().await.is_err());
        assert_eq!(counter.read().await.unwrap(), 0);
        consumer.core().release();
        counter.core().release();
        host.close().await.unwrap();
    });
}
