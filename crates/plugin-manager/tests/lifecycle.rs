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

fn fixtures() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    assert!(
        path.join("dynamic-consumer/1.0.0/manifest.json").is_file(),
        "build the real plugin fixtures with just tembra-plugins before running lifecycle tests"
    );
    path
}

async fn node(
    driver: Rc<TokioDriver>,
    database: &std::path::Path,
    proof: AdminProof,
    sources: bool,
) -> (Host, Rc<PluginManager>) {
    let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
    host.install(
        swarmie_counter::facet().unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    if sources {
        host.install(
            LocalSource::new("local", fixtures()).facet().unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
    }
    let manager = PluginManager::start(host.clone(), driver, database, proof.clone())
        .await
        .unwrap();
    manager
        .approve_trusted_source(&proof, "local".into())
        .unwrap();
    manager
        .record_core(&proof, "swarmie.counter".into(), "0.1.0".into())
        .await
        .unwrap();
    (host, manager)
}

fn install(proof: &AdminProof, version: &str) -> InstallRequest {
    InstallRequest {
        proof: proof.clone(),
        provider_id: "local".into(),
        plugin_id: "dynamic-consumer".into(),
        version: version.into(),
        confirmed: true,
    }
}

fn update(proof: &AdminProof, version: &str) -> UpdateRequest {
    UpdateRequest {
        proof: proof.clone(),
        provider_id: "local".into(),
        plugin_id: "dynamic-consumer".into(),
        version: version.into(),
        confirmed: true,
    }
}

#[test]
fn real_javascript_reload_preserves_references_and_failed_update_reports_old_selection() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let (host, manager) = node(
            driver,
            &directory.path().join("admin.sqlite"),
            proof.clone(),
            true,
        )
        .await;
        manager.install(install(&proof, "1.0.0")).await.unwrap();
        assert!(
            host.get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .is_err()
        );
        manager
            .activate(ActivateRequest {
                proof: proof.clone(),
                plugin_id: "dynamic-consumer".into(),
            })
            .await
            .unwrap();
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let readable = consumer.invocations();
        let seen = Rc::new(std::cell::Cell::new(0));
        let observed = seen.clone();
        let subscription = readable
            .subscribe(move |snapshot| {
                observed.set(snapshot.value()?);
                Ok(())
            })
            .unwrap();
        assert_eq!(consumer.run().await.unwrap(), 1);
        assert_eq!(readable.value().unwrap(), 1);
        manager.update(update(&proof, "1.1.0")).await.unwrap();
        assert_eq!(consumer.run().await.unwrap(), 11);
        assert_eq!(readable.value().unwrap(), 2);
        assert_eq!(seen.get(), 2);
        let failure = manager.update(update(&proof, "1.2.0")).await.unwrap_err();
        assert!(
            failure.to_string().contains("Candidate setup failed"),
            "{failure}"
        );
        let inventory = manager.inventory(&proof).unwrap();
        let plugin = inventory
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "dynamic-consumer")
            .unwrap();
        assert_eq!(plugin.version, "1.1.0");
        assert_eq!(plugin.observed, ObservedState::Active);
        assert!(!plugin.errors.is_empty());
        assert_eq!(consumer.run().await.unwrap(), 21);
        assert_eq!(readable.value().unwrap(), 3);
        drop(subscription);
        consumer.core().release();
        manager
            .deactivate(DeactivateRequest {
                proof: proof.clone(),
                plugin_id: "dynamic-consumer".into(),
            })
            .await
            .unwrap();
        assert!(
            host.get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .is_err()
        );
        manager
            .activate(ActivateRequest {
                proof: proof.clone(),
                plugin_id: "dynamic-consumer".into(),
            })
            .await
            .unwrap();
        manager
            .uninstall(UninstallRequest {
                proof: proof.clone(),
                plugin_id: "dynamic-consumer".into(),
            })
            .await
            .unwrap();
        assert!(
            manager
                .inventory(&proof)
                .unwrap()
                .plugins
                .iter()
                .all(|plugin| plugin.plugin_id != "dynamic-consumer")
        );
        host.close().await.unwrap();
    });
}

#[test]
fn offline_restart_uses_cached_bytes_and_tampering_is_refused() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("admin.sqlite");
        let proof = PluginManager::issue_proof();
        let (host, manager) = node(driver.clone(), &database, proof.clone(), true).await;
        manager.install(install(&proof, "1.0.0")).await.unwrap();
        manager
            .activate(ActivateRequest {
                proof: proof.clone(),
                plugin_id: "dynamic-consumer".into(),
            })
            .await
            .unwrap();
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.run().await.unwrap(), 1);
        consumer.core().release();
        host.close().await.unwrap();
        drop(manager);
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection.execute("INSERT INTO operations(id,plugin_id,operation,status,error) VALUES (?1,?2,?3,?4,'')",
            rusqlite::params!["interrupted-update","dynamic-consumer",serde_json::to_string(&AdminOperation::Update).unwrap(),serde_json::to_string(&OperationStatus::Running).unwrap()]).unwrap();
        drop(connection);
        let (host, manager) = node(driver.clone(), &database, proof.clone(), false).await;
        assert!(manager.inventory(&proof).unwrap().providers.is_empty());
        let recovered = manager.recover(&proof).await.unwrap();
        assert_eq!(
            recovered
                .operations
                .iter()
                .find(|operation| operation.id == "interrupted-update")
                .unwrap()
                .status,
            OperationStatus::Interrupted
        );
        assert_eq!(
            manager
                .active_package(&proof, "dynamic-consumer")
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        let consumer = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(consumer.invocations().value().unwrap(), 0);
        assert_eq!(consumer.run().await.unwrap(), 1);
        consumer.core().release();
        host.close().await.unwrap();
        drop(manager);
        let connection = rusqlite::Connection::open(&database).unwrap();
        let bytes:Vec<u8>=connection.query_row("SELECT package FROM plugin_versions WHERE plugin_id='dynamic-consumer' AND version='1.0.0'",[],|row|row.get(0)).unwrap();
        let mut package: AcquiredPackage = serde_json::from_slice(&bytes).unwrap();
        package.files[0].bytes.push(b' ');
        connection.execute("UPDATE plugin_versions SET package=?1 WHERE plugin_id='dynamic-consumer' AND version='1.0.0'",[serde_json::to_vec(&package).unwrap()]).unwrap();
        drop(connection);
        let (host, manager) = node(driver, &database, proof.clone(), false).await;
        assert!(matches!(
            manager.recover(&proof).await,
            Err(ManagerError::Integrity(_))
        ));
        let inventory = manager.inventory(&proof).unwrap();
        assert_eq!(
            inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "dynamic-consumer")
                .unwrap()
                .observed,
            ObservedState::Failed
        );
        assert!(
            host.get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .is_err()
        );
        host.close().await.unwrap();
    });
}

#[test]
fn missing_native_dependency_prevents_installation() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let proof = PluginManager::issue_proof();
        let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
        let source = directory.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let fixture = fixtures().join("dynamic-consumer/1.0.0");
        let mut manifest: PluginManifest =
            serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
        manifest.dependencies = vec![PluginDependency {
            id: "swarmie.counter".into(),
            version: "^0.1.0".into(),
        }];
        std::fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::copy(fixture.join("artifact.json"), source.join("artifact.json")).unwrap();
        host.install(
            LocalSource::new("local", source).facet().unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
        let manager = PluginManager::start(
            host.clone(),
            driver,
            directory.path().join("admin.sqlite"),
            proof.clone(),
        )
        .await
        .unwrap();
        manager
            .approve_trusted_source(&proof, "local".into())
            .unwrap();
        assert!(matches!(
            manager.install(install(&proof, "1.0.0")).await,
            Err(ManagerError::Dependency(_))
        ));
        assert!(
            manager
                .inventory(&proof)
                .unwrap()
                .plugins
                .iter()
                .all(|plugin| plugin.kind == PluginKind::Core)
        );
        host.close().await.unwrap();
    });
}

#[test]
fn failed_initial_batch_retires_earlier_successful_facets_before_claiming_inactive() {
    use sha2::{Digest, Sha256};

    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let paths: serde_json::Value = serde_json::from_slice(
            &std::fs::read(repository.join("target/plugins/artifacts.json")).unwrap(),
        )
        .unwrap();
        let artifact =
            std::fs::read(repository.join(paths["partial-install"].as_str().unwrap())).unwrap();
        let sha256 = hex::encode(Sha256::digest(&artifact));
        let manifest = PluginManifest {
            schema_version: 1,
            id: "partial-package".into(),
            version: "1.0.0".into(),
            compatibility: Compatibility {
                swarmie: "^0.1.0".into(),
                tembra_revision: TEMBRA_REVISION.into(),
            },
            dependencies: vec![],
            facets: [("partial-first", "first"), ("partial-second", "second")]
                .into_iter()
                .map(|(id, export)| PluginFacet {
                    id: id.into(),
                    target: FacetTarget::Server,
                    format: FacetFormat::Bundled,
                    artifact_path: "artifact.json".into(),
                    sha256: sha256.clone(),
                    selected_export: export.into(),
                })
                .collect(),
            files: vec![ManifestFile {
                path: "artifact.json".into(),
                sha256,
            }],
            distribution: Distribution {
                provider: "local".into(),
                description: "Partial initial installation acceptance".into(),
            },
            ui: vec![],
        };
        std::fs::write(source.join("artifact.json"), artifact).unwrap();
        std::fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let proof = PluginManager::issue_proof();
        let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
        let manager = PluginManager::start(
            host.clone(),
            driver,
            directory.path().join("admin.sqlite"),
            proof.clone(),
        )
        .await
        .unwrap();
        manager
            .approve_trusted_source(&proof, "local".into())
            .unwrap();
        host.install(
            LocalSource::new("local", source).facet().unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
        manager
            .install(InstallRequest {
                proof: proof.clone(),
                provider_id: "local".into(),
                plugin_id: "partial-package".into(),
                version: "1.0.0".into(),
                confirmed: true,
            })
            .await
            .unwrap();
        let error = manager
            .activate(ActivateRequest {
                proof: proof.clone(),
                plugin_id: "partial-package".into(),
            })
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Second initial facet setup failed"),
            "{error}"
        );
        let partial = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(partial.run().await.unwrap(), 42);
        partial.core().release();
        let inventory = manager
            .deactivate(DeactivateRequest {
                proof: proof.clone(),
                plugin_id: "partial-package".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "partial-package")
                .unwrap()
                .observed,
            ObservedState::Inactive
        );
        assert!(
            host.get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .is_err()
        );
        host.close().await.unwrap();
    });
}
