use std::{cell::Cell as Flag, future::Future, path::Path, rc::Rc};

use swarmie_counter::{Counter, CounterProvider, DynamicConsumer, DynamicConsumerInput};
use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{LocalSource, ManagerError, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

const OLD: &str = "dynamic-consumer";
const NEW: &str = "counter-provider-gated";
const NEW_FACET: &str = "counter-provider-gated.runtime";

#[derive(Default)]
struct Gate {
    blocked: Flag<bool>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

struct GatedCounter {
    count: Cell<u32>,
    gate: Rc<Gate>,
}

impl CounterProvider for GatedCounter {
    fn count(&self) -> Readable<u32> {
        self.count.readable()
    }

    async fn increment(&self, by: u32, context: Context) -> Result<u32> {
        self.count.change(&context, |value| {
            *value = value
                .checked_add(by)
                .ok_or_else(|| RuntimeError::invalid("counter_overflow", "counter overflow"))?;
            Ok(())
        })?;
        self.count.value()
    }

    async fn read(&self, _context: Context) -> Result<u32> {
        if self.gate.blocked.get() {
            self.gate.entered.notify_one();
            self.gate.release.notified().await;
        }
        self.count.value()
    }
}

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

fn request(plugin_id: &str) -> InstallRequest {
    InstallRequest {
        plugin_id: plugin_id.into(),
        provider_id: "local".into(),
        version: "1.0.0".into(),
        confirmed: true,
    }
}

async fn node(
    driver: Rc<TokioDriver>,
    database: &Path,
    sources: &Path,
) -> (Host, Rc<PluginManager>, Rc<Gate>) {
    let host = Host::with_driver(RUNTIME_HOST, "replacement", driver.clone()).unwrap();
    let gate = Rc::new(Gate::default());
    let factory_gate = gate.clone();
    host.install(
        NativeFacet::new("gated-counter", vec![], move |_, setup| {
            let gate = factory_gate.clone();
            let count_definition = setup.state("count")?;
            setup.serve_with_options(
                Counter,
                move |_, _, declaration| {
                    Ok(GatedCounter {
                        count: declaration.state(count_definition.value(0)?)?,
                        gate: gate.clone(),
                    })
                },
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        })
        .unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    host.install(
        LocalSource::new("local", sources).facet().unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    let manager = PluginManager::start(host.clone(), driver, database)
        .await
        .unwrap();
    manager.approve_trusted_source("local".into()).unwrap();
    manager.install(request(OLD)).await.unwrap();
    manager
        .activate(ActivateRequest {
            plugin_id: OLD.into(),
        })
        .await
        .unwrap();
    (host, manager, gate)
}

fn fixtures() -> std::path::PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    assert!(
        root.join(NEW).join("1.0.0/artifact.json").is_file(),
        "build real fixtures first"
    );
    root
}

fn incompatible_sources(sources: &Path, candidate: &str) {
    for (source_version, id) in [("1.0.0", OLD), ("1.7.0", candidate)] {
        let original = fixtures().join(OLD).join(source_version);
        let target = sources.join(id).join("1.0.0");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::copy(original.join("artifact.json"), target.join("artifact.json")).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(original.join("manifest.json")).unwrap())
                .unwrap();
        manifest["id"] = id.into();
        manifest["version"] = "1.0.0".into();
        std::fs::write(
            target.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
}

fn assert_pair_uncertain(manager: &PluginManager) {
    let inventory = manager.inventory().unwrap();
    for id in [OLD, NEW] {
        let plugin = inventory
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == id)
            .unwrap();
        assert_eq!(plugin.observed, ObservedState::Unavailable, "{id}");
        assert!(!plugin.errors.is_empty());
    }
    assert!(manager.requires_recovery());
}

#[test]
fn cross_package_reload_preserves_actual_reference_state_and_settles_both_participants() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, _) =
            node(driver, &directory.path().join("admin.sqlite"), &fixtures()).await;
        let reference = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        let invocations = reference.invocations();
        assert_eq!(reference.run().await.unwrap(), 1);
        assert!(matches!(
            manager.replace_dynamic(OLD, request("swarmie.plugin-manager")).await,
            Err(ManagerError::CorePlugin(id)) if id == "swarmie.plugin-manager"
        ));
        let before = manager.active_package(OLD).unwrap();
        let result = manager.replace_dynamic(OLD, request(NEW)).await.unwrap();
        assert_eq!(reference.run().await.unwrap(), 11);
        assert_eq!(invocations.value().unwrap(), 11);
        let old = result
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == OLD)
            .unwrap();
        assert_eq!(old.desired, DesiredState::Installed);
        assert_eq!(old.observed, ObservedState::Inactive);
        let new = result
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == NEW)
            .unwrap();
        assert_eq!(new.observed, ObservedState::Active);
        assert!(
            !result
                .installed_facets
                .iter()
                .any(|facet| facet.facet_id == OLD)
        );
        assert!(
            result
                .installed_facets
                .iter()
                .any(|facet| facet.facet_id == NEW_FACET
                    && facet.kind == InstalledFacetKind::Foreign)
        );
        assert!(manager.active_package(OLD).is_err());
        let after = manager.active_package(NEW).unwrap();
        assert_ne!(before.activation_id, after.activation_id);
        assert!(!manager.requires_recovery());
        host.close().await.unwrap();
    });
}

#[test]
fn cancellation_keeps_both_participants_uncertain_even_without_new_installed_record() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, gate) =
            node(driver, &directory.path().join("admin.sqlite"), &fixtures()).await;
        gate.blocked.set(true);
        let mut replacement = Box::pin(manager.replace_dynamic(OLD, request(NEW)));
        tokio::select! {
            result = &mut replacement => panic!("candidate unexpectedly settled: {result:?}"),
            () = gate.entered.notified() => {}
            () = tokio::time::sleep(std::time::Duration::from_secs(30)) => panic!("candidate did not enter actual Counter readiness gate"),
        }
        assert_pair_uncertain(&manager);
        let inventory = manager.inventory().unwrap();
        assert!(
            inventory
                .installed_facets
                .iter()
                .any(|facet| facet.facet_id == OLD)
        );
        assert!(
            !inventory
                .installed_facets
                .iter()
                .any(|facet| facet.facet_id == NEW_FACET)
        );
        drop(replacement);
        assert_pair_uncertain(&manager);
        assert!(matches!(
            manager.install(request("unrelated")).await,
            Err(ManagerError::RecoveryRequired)
        ));
        gate.release.notify_one();
        host.close().await.unwrap();
    });
}

async fn failed_settlement(driver: Rc<TokioDriver>, trigger: &str) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("admin.sqlite");
    let (host, manager, _) = node(driver, &database, &fixtures()).await;
    let reference = host
        .get(DynamicConsumer, DynamicConsumerInput {})
        .await
        .unwrap();
    assert_eq!(reference.run().await.unwrap(), 1);
    let receipt = manager.active_package(OLD).unwrap().activation_id;
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch(trigger).unwrap();
    let error = manager
        .replace_dynamic(OLD, request(NEW))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ManagerError::PublicationUnrecorded { .. }),
        "{error:?}"
    );
    assert!(error.requires_restart());
    assert!(error.to_string().contains("forced_settlement_failure"));
    assert_eq!(
        reference.run().await.unwrap(),
        11,
        "Tembra adopted the candidate before SQLite settlement failed"
    );
    assert_pair_uncertain(&manager);
    let inventory = manager.inventory().unwrap();
    assert!(
        inventory
            .installed_facets
            .iter()
            .any(|facet| facet.facet_id == NEW_FACET)
    );
    assert!(
        !inventory
            .installed_facets
            .iter()
            .any(|facet| facet.facet_id == OLD)
    );
    let (desired, retained_receipt): (String, String) = connection
        .query_row(
            "SELECT desired, activation_id FROM installed_plugins WHERE plugin_id=?1",
            [OLD],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        desired,
        serde_json::to_string(&DesiredState::Active).unwrap()
    );
    assert_eq!(retained_receipt, receipt);
    let new_count: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM installed_plugins WHERE plugin_id=?1",
            [NEW],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        new_count, 0,
        "no half of a failed acknowledgement transaction may persist"
    );
    let running = inventory
        .operations
        .iter()
        .filter(|operation| operation.status == OperationStatus::Running)
        .count();
    assert_eq!(running, 2);
    for id in [OLD, NEW] {
        assert!(manager.active_package(id).is_err());
    }
    host.close().await.unwrap();
}

#[test]
fn package_acknowledgement_failure_after_adoption_preserves_pair_uncertainty() {
    run(|driver| {
        failed_settlement(
            driver,
            "CREATE TRIGGER reject_candidate BEFORE INSERT ON installed_plugins WHEN NEW.plugin_id='counter-provider-gated' BEGIN SELECT RAISE(FAIL, 'forced_settlement_failure'); END;",
        )
    });
}

#[test]
fn operation_acknowledgement_failure_rolls_back_both_package_selections() {
    run(|driver| {
        failed_settlement(
            driver,
            "CREATE TRIGGER reject_operation BEFORE UPDATE OF status ON operations WHEN NEW.plugin_id='counter-provider-gated' AND NEW.status='\"succeeded\"' BEGIN SELECT RAISE(FAIL, 'forced_settlement_failure'); END;",
        )
    });
}

#[test]
fn incompatible_cross_package_candidate_with_same_facet_keeps_old_selection_and_reference() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let sources = directory.path().join("source");
        incompatible_sources(&sources, "incompatible-provider");
        let (host, manager, _) =
            node(driver, &directory.path().join("admin.sqlite"), &sources).await;
        let reference = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(reference.run().await.unwrap(), 1);
        let before = manager.active_package(OLD).unwrap();
        let failure = manager
            .replace_dynamic(OLD, request("incompatible-provider"))
            .await
            .unwrap_err();
        assert!(!failure.requires_restart(), "{failure}");
        assert_eq!(
            manager.active_package(OLD).unwrap().activation_id,
            before.activation_id
        );
        assert_eq!(reference.run().await.unwrap(), 2);
        assert_eq!(reference.invocations().value().unwrap(), 2);
        let inventory = manager.inventory().unwrap();
        assert_eq!(
            inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == OLD)
                .unwrap()
                .observed,
            ObservedState::Active
        );
        assert!(
            !inventory
                .plugins
                .iter()
                .any(|plugin| plugin.plugin_id == "incompatible-provider")
        );
        assert!(
            inventory
                .installed_facets
                .iter()
                .any(|facet| facet.facet_id == OLD)
        );
        assert!(!manager.requires_recovery());
        host.close().await.unwrap();
    });
}

#[test]
fn failed_refusal_acknowledgement_preserves_both_original_and_recording_causes() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let sources = directory.path().join("source");
        incompatible_sources(&sources, NEW);
        let database = directory.path().join("admin.sqlite");
        let (host, manager, _) = node(driver, &database, &sources).await;
        let reference = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(reference.run().await.unwrap(), 1);
        let connection = rusqlite::Connection::open(database).unwrap();
        connection.execute_batch("CREATE TRIGGER reject_failure_record BEFORE UPDATE OF status ON operations WHEN NEW.plugin_id='counter-provider-gated' AND NEW.status='\"failed\"' BEGIN SELECT RAISE(FAIL, 'forced_refusal_recording_failure'); END;").unwrap();
        let error = manager
            .replace_dynamic(OLD, request(NEW))
            .await
            .unwrap_err();
        let ManagerError::FailureUnrecorded { failure, recording } = &error else {
            panic!("{error:?}")
        };
        assert!(matches!(
            failure.as_ref(),
            ManagerError::Runtime(_) | ManagerError::ReloadFailed { .. }
        ));
        assert!(matches!(recording.as_ref(), ManagerError::Database(_)));
        assert!(
            recording
                .to_string()
                .contains("forced_refusal_recording_failure")
        );
        assert_pair_uncertain(&manager);
        assert_eq!(
            reference.run().await.unwrap(),
            2,
            "candidate refusal preserves the old runtime even if acknowledgement fails"
        );
        host.close().await.unwrap();
    });
}

#[test]
fn actual_foreign_removal_downgrades_inventory_without_rewriting_acknowledged_intent() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let (host, manager, _) =
            node(driver, &directory.path().join("admin.sqlite"), &fixtures()).await;
        let reference = host
            .get(DynamicConsumer, DynamicConsumerInput {})
            .await
            .unwrap();
        assert_eq!(reference.run().await.unwrap(), 1);
        manager
            .observe_reference(OLD, reference.core().clone())
            .unwrap();
        let receipt = manager.active_package(OLD).unwrap().activation_id;
        host.core()
            .reload(
                vec![],
                core::ReloadOptions {
                    remove: vec![OLD.into()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let inventory = manager.inventory().unwrap();
        let plugin = inventory
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == OLD)
            .unwrap();
        assert_eq!(plugin.desired, DesiredState::Active);
        assert_eq!(plugin.observed, ObservedState::Unavailable);
        assert!(plugin.errors.iter().any(|error| {
            error.contains("absent from Tembra's installed catalog") && error.contains(OLD)
        }));
        assert!(
            !inventory
                .installed_facets
                .iter()
                .any(|facet| facet.facet_id == OLD)
        );
        assert_eq!(reference.core().status(), core::RefStatus::Gone);
        assert!(reference.run().await.is_err());
        assert_eq!(
            manager.active_package(OLD).unwrap().activation_id,
            receipt,
            "the read-only diagnostic does not rewrite prior administrative acknowledgement"
        );
        host.close().await.unwrap();
    });
}
