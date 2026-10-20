use std::path::PathBuf;

use sha2::{Digest, Sha256};
use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{LocalSource, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

/// Packages the genuine CLI browser artifact for administrative receipt qualification.
/// This test does not execute a Browser Host or qualify a renderer module.
fn source_fixture(root: &std::path::Path) {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let paths: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repository.join("target/plugins/artifacts.json")).unwrap(),
    )
    .unwrap();
    let bytes = std::fs::read(repository.join(paths["browser-widget"].as_str().unwrap())).unwrap();
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let manifest = PluginManifest {
        schema_version: 1,
        id: "browser-widget".into(),
        version: "1.0.0".into(),
        compatibility: Compatibility {
            swarmie: "^0.1.0".into(),
            tembra_revision: TEMBRA_REVISION.into(),
        },
        dependencies: vec![],
        facets: vec![PluginFacet {
            id: "browser-widget".into(),
            target: FacetTarget::Browser,
            format: FacetFormat::Bundled,
            artifact_path: "artifact.json".into(),
            sha256: sha256.clone(),
            selected_export: "default".into(),
        }],
        files: vec![ManifestFile {
            path: "artifact.json".into(),
            sha256,
        }],
        distribution: Distribution {
            provider: "local".into(),
            description: "Browser receipt qualification".into(),
        },
        ui: vec![],
    };
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("artifact.json"), bytes).unwrap();
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
}

fn report(
    proof: &AdminProof,
    activation: &str,
    observed: BrowserObservation,
) -> ReportBrowserRequest {
    ReportBrowserRequest {
        proof: proof.clone(),
        plugin_id: "browser-widget".into(),
        version: "1.0.0".into(),
        activation_id: activation.into(),
        session_id: "browser-session".into(),
        observed,
        error: String::new(),
    }
}

#[test]
fn browser_receipts_refuse_stale_activations_and_removal_requires_real_acknowledgement() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source");
            source_fixture(&source);
            let proof = PluginManager::issue_proof();
            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            host.install(
                LocalSource::new("local", source).facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("admin.sqlite"),
                proof.clone(),
            )
            .await
            .unwrap();
            manager
                .approve_trusted_source(&proof, "local".into())
                .unwrap();
            manager
                .install(InstallRequest {
                    proof: proof.clone(),
                    provider_id: "local".into(),
                    plugin_id: "browser-widget".into(),
                    version: "1.0.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            let inventory = manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert!(inventory.browser_sessions.is_empty());
            assert_eq!(
                inventory
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == "browser-widget")
                    .unwrap()
                    .observed,
                ObservedState::Installed
            );
            let first = manager.active_package(&proof, "browser-widget").unwrap();
            assert!(
                manager
                    .report_browser(report(&proof, "stale", BrowserObservation::Active))
                    .is_err()
            );
            manager
                .report_browser(report(
                    &proof,
                    &first.activation_id,
                    BrowserObservation::Active,
                ))
                .unwrap();
            let inventory = manager
                .deactivate(DeactivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert_eq!(
                inventory.browser_sessions[0].observed,
                BrowserObservation::Active
            );
            assert!(
                inventory.browser_sessions[0]
                    .error
                    .contains("awaiting browser acknowledgement")
            );
            assert!(
                manager
                    .report_browser(report(
                        &proof,
                        &first.activation_id,
                        BrowserObservation::Active
                    ))
                    .is_err()
            );
            manager
                .report_browser(report(
                    &proof,
                    &first.activation_id,
                    BrowserObservation::Inactive,
                ))
                .unwrap();
            manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            let second = manager.active_package(&proof, "browser-widget").unwrap();
            assert_ne!(first.activation_id, second.activation_id);
            assert!(
                manager
                    .report_browser(report(
                        &proof,
                        &first.activation_id,
                        BrowserObservation::Active
                    ))
                    .is_err()
            );
            manager
                .report_browser(report(
                    &proof,
                    &second.activation_id,
                    BrowserObservation::Active,
                ))
                .unwrap();
            let inventory = manager
                .uninstall(UninstallRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert_eq!(
                inventory
                    .browser_sessions
                    .iter()
                    .find(|record| record.activation_id == second.activation_id)
                    .unwrap()
                    .observed,
                BrowserObservation::Active
            );
            let inventory = manager
                .report_browser(report(
                    &proof,
                    &second.activation_id,
                    BrowserObservation::Inactive,
                ))
                .unwrap();
            assert_eq!(
                inventory
                    .browser_sessions
                    .iter()
                    .find(|record| record.activation_id == second.activation_id)
                    .unwrap()
                    .observed,
                BrowserObservation::Inactive
            );
            host.close().await.unwrap();
        }));
}

#[test]
fn bounded_package_reads_require_current_receipts_and_return_exact_cached_bytes() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source");
            source_fixture(&source);
            let expected = std::fs::read(source.join("artifact.json")).unwrap();
            let proof = PluginManager::issue_proof();
            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            host.install(
                LocalSource::new("local", &source).facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("admin.sqlite"),
                proof.clone(),
            )
            .await
            .unwrap();
            manager
                .approve_trusted_source(&proof, "local".into())
                .unwrap();
            manager
                .install(InstallRequest {
                    proof: proof.clone(),
                    provider_id: "local".into(),
                    plugin_id: "browser-widget".into(),
                    version: "1.0.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            std::fs::remove_dir_all(&source).unwrap();
            let administration = host
                .get(PluginAdministration, PluginAdministrationInput {})
                .await
                .unwrap();
            let approved = administration
                .active_package(ActivePackageRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            let metadata = serde_json::to_value(&approved).unwrap();
            assert!(metadata.get("files").is_none());
            assert!(metadata.get("package").is_none());
            assert_eq!(approved.manifest.id, "browser-widget");
            let request = ReadPackageFileRequest {
                proof: proof.clone(),
                plugin_id: "browser-widget".into(),
                activation_id: approved.activation_id.clone(),
                path: "artifact.json".into(),
                offset: 0,
                length: 32768,
            };
            let mut invalid = request.clone();
            invalid.proof = AdminProof::new("forged".into());
            let refused = administration.read_package_file(invalid).await.unwrap_err();
            assert_eq!(refused.code, core::ErrorCode::Forbidden);
            let mut invalid = request.clone();
            invalid.activation_id = "stale".into();
            assert!(administration.read_package_file(invalid).await.is_err());
            for length in [0, 32769, u32::MAX] {
                let mut invalid = request.clone();
                invalid.length = length;
                assert!(administration.read_package_file(invalid).await.is_err());
            }
            for path in ["missing.js", "../artifact.json", "./artifact.json"] {
                let mut invalid = request.clone();
                invalid.path = path.into();
                assert!(administration.read_package_file(invalid).await.is_err());
            }
            let mut invalid = request.clone();
            invalid.offset = u32::MAX;
            assert!(administration.read_package_file(invalid).await.is_err());
            let mut bytes = vec![];
            loop {
                let mut chunk_request = request.clone();
                chunk_request.offset = u32::try_from(bytes.len()).unwrap();
                let chunk = administration
                    .read_package_file(chunk_request)
                    .await
                    .unwrap();
                assert_eq!(chunk.total_length as usize, expected.len());
                assert!(chunk.bytes.len() <= 32768);
                assert!(serde_json::to_vec(&chunk).unwrap().len() < 1024 * 1024);
                bytes.extend(chunk.bytes);
                if bytes.len() == chunk.total_length as usize {
                    break;
                }
            }
            assert_eq!(bytes, expected);
            let mut end = request.clone();
            end.offset = u32::try_from(expected.len()).unwrap();
            assert!(
                administration
                    .read_package_file(end)
                    .await
                    .unwrap()
                    .bytes
                    .is_empty()
            );
            manager
                .deactivate(DeactivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert!(
                administration
                    .read_package_file(request.clone())
                    .await
                    .is_err()
            );
            manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert!(administration.read_package_file(request).await.is_err());
            administration.core().release();
            host.close().await.unwrap();
        }));
}

#[test]
fn a_failed_new_browser_receipt_cannot_erase_an_older_generation_removal_acknowledgement() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source");
            for version in ["1.1.0", "1.2.0"] {
                let package = source.join(version);
                source_fixture(&package);
                let manifest_path = package.join("manifest.json");
                let mut manifest: PluginManifest =
                    serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
                manifest.version = version.into();
                std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            }
            let proof = PluginManager::issue_proof();
            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            host.install(
                LocalSource::new("local", source).facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("admin.sqlite"),
                proof.clone(),
            )
            .await
            .unwrap();
            manager
                .approve_trusted_source(&proof, "local".into())
                .unwrap();
            manager
                .install(InstallRequest {
                    proof: proof.clone(),
                    provider_id: "local".into(),
                    plugin_id: "browser-widget".into(),
                    version: "1.1.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            let old = manager.active_package(&proof, "browser-widget").unwrap();
            let mut active = report(&proof, &old.activation_id, BrowserObservation::Active);
            active.version = "1.1.0".into();
            manager.report_browser(active.clone()).unwrap();
            manager
                .update(UpdateRequest {
                    proof: proof.clone(),
                    provider_id: "local".into(),
                    plugin_id: "browser-widget".into(),
                    version: "1.2.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            let candidate = manager.active_package(&proof, "browser-widget").unwrap();
            let mut failed = report(&proof, &candidate.activation_id, BrowserObservation::Failed);
            failed.version = "1.2.0".into();
            failed.error = "candidate setup failed".into();
            let inventory = manager.report_browser(failed).unwrap();
            assert_eq!(inventory.browser_sessions.len(), 2);
            assert_eq!(
                inventory.browser_sessions[0].activation_id,
                old.activation_id
            );
            assert_eq!(
                inventory.browser_sessions[0].observed,
                BrowserObservation::Active
            );
            assert_eq!(
                inventory.browser_sessions[1].activation_id,
                candidate.activation_id
            );
            assert_eq!(
                inventory.browser_sessions[1].observed,
                BrowserObservation::Failed
            );
            assert!(manager.report_browser(active).is_err());
            let inventory = manager
                .deactivate(DeactivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert_eq!(inventory.browser_sessions.len(), 2);
            assert_eq!(
                inventory.browser_sessions[0].observed,
                BrowserObservation::Active
            );
            assert!(
                inventory
                    .browser_sessions
                    .iter()
                    .all(|record| record.error.contains("awaiting browser acknowledgement"))
            );
            assert!(
                inventory.browser_sessions[1]
                    .error
                    .contains("candidate setup failed")
            );
            let mut retired = report(&proof, &old.activation_id, BrowserObservation::Inactive);
            retired.version = "1.1.0".into();
            let inventory = manager.report_browser(retired.clone()).unwrap();
            assert_eq!(inventory.browser_sessions.len(), 2);
            assert_eq!(
                inventory.browser_sessions[0].activation_id,
                old.activation_id
            );
            assert_eq!(
                inventory.browser_sessions[0].observed,
                BrowserObservation::Inactive
            );
            assert_eq!(
                inventory.browser_sessions[1].activation_id,
                candidate.activation_id
            );
            assert_eq!(
                inventory.browser_sessions[1].observed,
                BrowserObservation::Failed
            );
            assert_eq!(
                manager
                    .report_browser(retired)
                    .unwrap()
                    .browser_sessions
                    .len(),
                2
            );
            host.close().await.unwrap();
        }));
}

#[test]
fn browser_grants_separate_delivery_reporting_and_supervisor_authority() {
    use std::collections::BTreeSet;

    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source");
            source_fixture(&source);
            let proof = PluginManager::issue_proof();
            let database = directory.path().join("admin.sqlite");
            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            let manager =
                PluginManager::start(host.clone(), driver.clone(), &database, proof.clone())
                    .await
                    .unwrap();
            manager
                .approve_trusted_source(&proof, "local".into())
                .unwrap();
            host.install(
                LocalSource::new("local", &source).facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            manager
                .install(InstallRequest {
                    proof: proof.clone(),
                    provider_id: "local".into(),
                    plugin_id: "browser-widget".into(),
                    version: "1.0.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            let grant = manager
                .issue_browser_grant(
                    &proof,
                    "session-a".into(),
                    &BTreeSet::from(["browser-widget".into()]),
                )
                .unwrap();
            assert!(!format!("{grant:?}").contains(grant.as_str()));
            let browser = host
                .get(BrowserAccess, BrowserAccessInput {})
                .await
                .unwrap();
            let inventory = browser
                .inventory(BrowserInventoryRequest {
                    grant: grant.clone(),
                    session_id: "session-a".into(),
                })
                .await
                .unwrap();
            assert_eq!(inventory.packages.len(), 1);
            assert!(inventory.observations.is_empty());
            let json = serde_json::to_value(&inventory).unwrap();
            assert!(json.get("providers").is_none());
            assert!(json.get("operations").is_none());
            assert!(
                browser
                    .inventory(BrowserInventoryRequest {
                        grant: grant.clone(),
                        session_id: "session-b".into()
                    })
                    .await
                    .is_err()
            );
            assert!(
                browser
                    .active_package(BrowserActivePackageRequest {
                        grant: grant.clone(),
                        session_id: "session-a".into(),
                        plugin_id: "unapproved".into()
                    })
                    .await
                    .is_err()
            );
            assert!(matches!(
                manager
                    .deactivate(DeactivateRequest {
                        proof: AdminProof::new(grant.as_str().into()),
                        plugin_id: "browser-widget".into()
                    })
                    .await,
                Err(swarmie_plugin_manager::ManagerError::Unauthorized)
            ));
            let activation = inventory.packages[0].activation_id.clone();
            let observation = |session: &str, observed| BrowserReportRequest {
                grant: grant.clone(),
                session_id: session.into(),
                plugin_id: "browser-widget".into(),
                version: "1.0.0".into(),
                activation_id: activation.clone(),
                observed,
                error: String::new(),
            };
            assert!(
                browser
                    .report_browser(observation("session-b", BrowserObservation::Active))
                    .await
                    .is_err()
            );
            browser
                .report_browser(observation("session-a", BrowserObservation::Active))
                .await
                .unwrap();
            manager
                .deactivate(DeactivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            manager
                .activate(ActivateRequest {
                    proof: proof.clone(),
                    plugin_id: "browser-widget".into(),
                })
                .await
                .unwrap();
            assert!(
                browser
                    .active_package(BrowserActivePackageRequest {
                        grant: grant.clone(),
                        session_id: "session-a".into(),
                        plugin_id: "browser-widget".into()
                    })
                    .await
                    .is_err()
            );
            assert!(
                browser
                    .report_browser(observation("session-a", BrowserObservation::Active))
                    .await
                    .is_err()
            );
            browser
                .report_browser(observation("session-a", BrowserObservation::Inactive))
                .await
                .unwrap();
            let new_grant = manager
                .issue_browser_grant(
                    &proof,
                    "session-a".into(),
                    &BTreeSet::from(["browser-widget".into()]),
                )
                .unwrap();
            manager.revoke_browser_grant(&proof, &grant).unwrap();
            assert!(
                browser
                    .inventory(BrowserInventoryRequest {
                        grant,
                        session_id: "session-a".into()
                    })
                    .await
                    .is_err()
            );
            browser.core().release();
            host.close().await.unwrap();
            drop(manager);
            let restarted_host =
                Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            let restarted = PluginManager::start(
                restarted_host.clone(),
                driver.clone(),
                &database,
                proof.clone(),
            )
            .await
            .unwrap();
            assert!(
                restarted
                    .browser_inventory(BrowserInventoryRequest {
                        grant: new_grant,
                        session_id: "session-a".into()
                    })
                    .is_err()
            );
            restarted_host.close().await.unwrap();
        }));
}
