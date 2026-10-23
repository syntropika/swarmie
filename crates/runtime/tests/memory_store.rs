//! Qualifies the real storage plugin and a Contract-compatible acceptance replacement.

use std::{future::Future, path::PathBuf, rc::Rc, time::Duration};

use serde_json::{Value, json};
use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_memory_store::{
    DeleteRequest, GetRequest, GetResult, ListRequest, MemoryStore, MemoryStoreInput,
    MemoryStoreRef, PutRequest,
};
use swarmie_runtime::{ChangeKind, Configuration, TransitionOutcome, start};
use tembra::core;
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
                .expect("MemoryStore application acceptance did not settle");
        }));
}

fn get(namespace: &str, key: &str) -> GetRequest {
    GetRequest {
        namespace: namespace.into(),
        key: key.into(),
    }
}

fn put(namespace: &str, key: &str, value: Value) -> PutRequest {
    PutRequest {
        namespace: namespace.into(),
        key: key.into(),
        value,
    }
}

// The caller uses only the owning plugin's canonical Contract, independent of provider kind.
async fn unchanged_consumer(memory: &MemoryStoreRef) {
    assert_eq!(
        memory.get(get("client", "absent")).await.unwrap(),
        GetResult::Missing
    );
    memory
        .put(put("client", "null", Value::Null))
        .await
        .unwrap();
    assert_eq!(
        memory.get(get("client", "null")).await.unwrap(),
        GetResult::Present { value: Value::Null }
    );
    let document = json!({"parts":[1, true, null], "title":"Application data"});
    memory
        .put(put("client", "document", document.clone()))
        .await
        .unwrap();
    assert_eq!(
        memory.get(get("client", "document")).await.unwrap(),
        GetResult::Present { value: document }
    );
    assert_eq!(
        memory
            .list(ListRequest {
                namespace: "client".into()
            })
            .await
            .unwrap(),
        ["document", "null"]
    );
    let deletion = DeleteRequest {
        namespace: "client".into(),
        key: "document".into(),
    };
    assert!(memory.delete(deletion.clone()).await.unwrap());
    assert!(!memory.delete(deletion).await.unwrap());
    assert!(
        memory
            .delete(DeleteRequest {
                namespace: "client".into(),
                key: "null".into()
            })
            .await
            .unwrap()
    );
}

#[test]
fn native_storage_and_dynamic_override_share_the_contract_with_honest_restart_and_data_boundaries()
{
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
        assert!(
            source
                .join("dynamic-memory-store/1.0.0/manifest.json")
                .is_file(),
            "build real plugin artifacts with just tembra-plugins"
        );
        let database = directory.path().join("application.sqlite");
        let configuration = |application| Configuration {
            database: database.clone(),
            source: source.clone(),
            application,
        };
        let mut native = start(driver.clone(), configuration(None)).await.unwrap();
        assert!(
            native.deployment_errors.is_empty(),
            "{:?}",
            native.deployment_errors
        );
        assert!(native.recovery_error.is_none());
        let native_application = native.desired_application().clone();
        assert_eq!(
            native.applied[swarmie_memory_store::PLUGIN_ID]
                .selection
                .config,
            json!({}),
            "the default memory provider uses the node database owner"
        );
        let original = native
            .host
            .get(MemoryStore, MemoryStoreInput {})
            .await
            .unwrap();
        unchanged_consumer(&original).await;
        let durable = json!({"revision":3,"explicit_null":null});
        original
            .put(put("durable", "note", durable.clone()))
            .await
            .unwrap();

        let mut replacement = native_application.clone();
        replacement.composition.profile.overrides.push(Selection {
            id: swarmie_memory_store::PLUGIN_ID.into(),
            plugin: "dynamic-memory-store".into(),
            version: "1.0.0".into(),
            provider: Some("local".into()),
            config: json!({"initial":{"source":"dynamic"}}),
        });
        replacement.releases.push(Release {
            plugin: "dynamic-memory-store".into(),
            version: "1.0.0".into(),
            kind: PluginKind::Dynamic,
            facets: vec![FacetIdentity {
                id: "dynamic-memory-store".into(),
                target: FacetTarget::Server,
            }],
        });
        let desired_before = native.manager.composition().unwrap();
        let plan = native.inspect_application(&replacement).unwrap();
        assert_eq!(
            plan.changes
                .iter()
                .find(|change| change.instance_id == swarmie_memory_store::PLUGIN_ID)
                .unwrap()
                .kind,
            ChangeKind::Replaced
        );
        assert_eq!(native.manager.composition().unwrap(), desired_before);
        let report = native.apply_application(replacement).await.unwrap();
        assert!(report.desired_persisted);
        assert!(!report.recovery_required);
        assert_eq!(
            report.results[swarmie_memory_store::PLUGIN_ID].outcome,
            TransitionOutcome::RestartRequired
        );
        assert_eq!(
            native.applied[swarmie_memory_store::PLUGIN_ID]
                .selection
                .plugin,
            swarmie_memory_store::PLUGIN_ID
        );
        assert_eq!(original.core().status(), core::RefStatus::Live);
        assert_eq!(
            original.get(get("durable", "note")).await.unwrap(),
            GetResult::Present {
                value: durable.clone()
            }
        );
        native.host.close().await.unwrap();
        assert!(original.get(get("durable", "note")).await.is_err());
        drop(native);

        let mut dynamic = start(driver.clone(), configuration(None)).await.unwrap();
        assert!(
            dynamic.deployment_errors.is_empty(),
            "{:?}",
            dynamic.deployment_errors
        );
        assert!(dynamic.recovery_error.is_none());
        assert_eq!(
            dynamic.applied[swarmie_memory_store::PLUGIN_ID]
                .selection
                .plugin,
            "dynamic-memory-store"
        );
        let replaced = dynamic
            .host
            .get(MemoryStore, MemoryStoreInput {})
            .await
            .unwrap();
        unchanged_consumer(&replaced).await;
        // Provider substitution does not implicitly migrate the native SQLite database.
        assert_eq!(
            replaced.get(get("durable", "note")).await.unwrap(),
            GetResult::Missing
        );
        replaced
            .put(put("ephemeral", "dynamic", json!(true)))
            .await
            .unwrap();
        assert_eq!(
            replaced.get(get("configuration", "initial")).await.unwrap(),
            GetResult::Present {
                value: json!({"source":"dynamic"})
            }
        );
        let report = dynamic.apply_application(native_application).await.unwrap();
        assert!(report.desired_persisted);
        assert_eq!(
            report.results[swarmie_memory_store::PLUGIN_ID].outcome,
            TransitionOutcome::RestartRequired
        );
        assert_eq!(
            replaced.get(get("ephemeral", "dynamic")).await.unwrap(),
            GetResult::Present { value: json!(true) }
        );
        dynamic.host.close().await.unwrap();
        assert!(replaced.get(get("ephemeral", "dynamic")).await.is_err());
        drop(dynamic);

        let mut restored = start(driver.clone(), configuration(None)).await.unwrap();
        assert!(
            restored.deployment_errors.is_empty(),
            "{:?}",
            restored.deployment_errors
        );
        assert!(restored.recovery_error.is_none());
        let memory = restored
            .host
            .get(MemoryStore, MemoryStoreInput {})
            .await
            .unwrap();
        assert_eq!(
            memory.get(get("durable", "note")).await.unwrap(),
            GetResult::Present {
                value: durable.clone()
            }
        );
        assert_eq!(
            memory.get(get("ephemeral", "dynamic")).await.unwrap(),
            GetResult::Missing
        );
        unchanged_consumer(&memory).await;
        let mut disabled_profile = restored.desired_application().clone();
        disabled_profile
            .composition
            .profile
            .disable
            .push(swarmie_memory_store::PLUGIN_ID.into());
        let report = restored.apply_application(disabled_profile).await.unwrap();
        assert_eq!(
            report.results[swarmie_memory_store::PLUGIN_ID].outcome,
            TransitionOutcome::RestartRequired
        );
        restored.host.close().await.unwrap();
        assert!(memory.get(get("durable", "note")).await.is_err());
        drop(restored);
        let mut disabled = start(driver.clone(), configuration(None)).await.unwrap();
        assert!(
            disabled
                .host
                .get(MemoryStore, MemoryStoreInput {})
                .await
                .is_err()
        );
        assert!(database.is_file(), "deactivation never deletes data");
        let mut enabled_profile = disabled.desired_application().clone();
        enabled_profile
            .composition
            .profile
            .disable
            .retain(|id| id != swarmie_memory_store::PLUGIN_ID);
        let report = disabled.apply_application(enabled_profile).await.unwrap();
        assert_eq!(
            report.results[swarmie_memory_store::PLUGIN_ID].outcome,
            TransitionOutcome::RestartRequired
        );
        disabled.host.close().await.unwrap();
        drop(disabled);
        let enabled = start(driver, configuration(None)).await.unwrap();
        let memory = enabled
            .host
            .get(MemoryStore, MemoryStoreInput {})
            .await
            .unwrap();
        assert_eq!(
            memory.get(get("durable", "note")).await.unwrap(),
            GetResult::Present { value: durable }
        );
        enabled.host.close().await.unwrap();
    });
}
