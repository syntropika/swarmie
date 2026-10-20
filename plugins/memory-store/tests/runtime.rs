use std::path::Path;

use serde_json::{Value, json};
use swarmie_memory_store::{
    Configuration, ConfigurationError, DeleteRequest, GetRequest, GetResult, ListRequest,
    MemoryStore, MemoryStoreInput, MemoryStoreRef, PutRequest,
};
use tembra::{core, prelude::*};

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

async fn open(host: &Host, database: &Path) -> Result<MemoryStoreRef> {
    host.install(
        swarmie_memory_store::facet(&Configuration {
            database: database.into(),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.get(MemoryStore, MemoryStoreInput {}).await
}

#[test]
fn json_values_namespaces_sorted_keys_and_delete_use_real_references() -> Result<()> {
    let directory = tempfile::tempdir().expect("test directory");
    let database = directory.path().join("memory.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("memory-test", "application", driver.clone())?;
    driver.run_until(async move {
        let memory = open(&host, &database).await?;
        assert_eq!(
            memory.get(get("notes", "missing")).await?,
            GetResult::Missing
        );
        memory.put(put("notes", "null", Value::Null)).await?;
        assert_eq!(
            memory.get(get("notes", "null")).await?,
            GetResult::Present { value: Value::Null }
        );
        let document = json!({"title":"Draft", "parts":[1, true, null, {"unicode":"á"}]});
        memory
            .put(put("notes", "document", document.clone()))
            .await?;
        assert_eq!(
            memory.get(get("notes", "document")).await?,
            GetResult::Present { value: document }
        );
        memory.put(put("private", "document", json!(false))).await?;
        memory.put(put("notes", "document", json!(42))).await?;
        assert_eq!(
            memory.get(get("notes", "document")).await?,
            GetResult::Present { value: json!(42) }
        );
        assert_eq!(
            memory.get(get("private", "document")).await?,
            GetResult::Present {
                value: json!(false)
            }
        );
        for key in ["z", "Z", "é", "a", "Ω"] {
            memory.put(put("ordered", key, json!(key))).await?;
        }
        assert_eq!(
            memory
                .list(ListRequest {
                    namespace: "ordered".into()
                })
                .await?,
            ["Z", "a", "z", "é", "Ω"]
        );
        assert!(
            memory
                .list(ListRequest {
                    namespace: "absent".into()
                })
                .await?
                .is_empty()
        );
        let deletion = DeleteRequest {
            namespace: "notes".into(),
            key: "null".into(),
        };
        assert!(memory.delete(deletion.clone()).await?);
        assert!(!memory.delete(deletion).await?);
        assert_eq!(memory.get(get("notes", "null")).await?, GetResult::Missing);
        host.close().await
    })
}

#[test]
fn invalid_identifiers_refuse_without_mutation_and_boundaries_are_bytes() -> Result<()> {
    let directory = tempfile::tempdir().expect("test directory");
    let database = directory.path().join("memory.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("memory-test", "application", driver.clone())?;
    driver.run_until(async move {
        let memory = open(&host, &database).await?;
        memory.put(put("notes", "draft", json!("before"))).await?;
        for invalid in [
            "".into(),
            " ".into(),
            "bad\nname".into(),
            " name".into(),
            "name\u{2003}".into(),
            "a".repeat(129),
            "é".repeat(65),
        ] {
            let error = memory
                .put(put(&invalid, "draft", json!("after")))
                .await
                .unwrap_err();
            assert_eq!(error.reason, "memory_store_invalid_namespace");
            assert_eq!(
                memory.get(get("notes", "draft")).await?,
                GetResult::Present {
                    value: json!("before")
                }
            );
            assert!(
                memory
                    .list(ListRequest { namespace: invalid })
                    .await
                    .is_err()
            );
        }
        for invalid in [
            "".into(),
            "\0".into(),
            " draft".into(),
            "draft ".into(),
            "a".repeat(513),
            "é".repeat(257),
        ] {
            assert_eq!(
                memory
                    .put(put("notes", &invalid, Value::Null))
                    .await
                    .unwrap_err()
                    .reason,
                "memory_store_invalid_key"
            );
            assert_eq!(
                memory.get(get("notes", &invalid)).await.unwrap_err().reason,
                "memory_store_invalid_key"
            );
            assert!(
                memory
                    .delete(DeleteRequest {
                        namespace: "notes".into(),
                        key: invalid
                    })
                    .await
                    .is_err()
            );
        }
        let namespace = "é".repeat(64);
        let key = "é".repeat(256);
        memory.put(put(&namespace, &key, Value::Null)).await?;
        assert_eq!(
            memory.get(get(&namespace, &key)).await?,
            GetResult::Present { value: Value::Null }
        );
        assert_eq!(
            memory
                .list(ListRequest {
                    namespace: "notes".into()
                })
                .await?,
            ["draft"]
        );
        host.close().await
    })
}

#[test]
fn shutdown_revokes_stale_reference_and_reactivation_preserves_data() -> Result<()> {
    let directory = tempfile::tempdir().expect("test directory");
    let database = directory.path().join("memory.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("first-host", "application", driver.clone())?;
    driver.run_until(async {
        let memory = open(&host, &database).await?;
        memory
            .put(put("notes", "durable", json!({"revision":1})))
            .await?;
        host.close().await?;
        assert!(memory.get(get("notes", "durable")).await.is_err());
        assert!(
            memory
                .put(put("notes", "durable", json!("stale")))
                .await
                .is_err()
        );
        let next = Host::with_driver("second-host", "application", driver.clone())?;
        let reopened = open(&next, &database).await?;
        assert_eq!(
            reopened.get(get("notes", "durable")).await?,
            GetResult::Present {
                value: json!({"revision":1})
            }
        );
        next.close().await
    })
}

#[test]
fn lock_and_corrupt_storage_fail_explicitly_instead_of_appearing_absent() -> Result<()> {
    let directory = tempfile::tempdir().expect("test directory");
    let database = directory.path().join("memory.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("memory-test", "application", driver.clone())?;
    driver.run_until(async move {
        let memory = open(&host, &database).await?;
        memory.put(put("notes", "locked", json!("before"))).await?;
        let competing = rusqlite::Connection::open(&database).expect("competing connection");
        competing.execute_batch("BEGIN IMMEDIATE").expect("write lock");
        assert_eq!(memory.put(put("notes", "locked", json!("after"))).await.unwrap_err().reason, "memory_store_storage");
        competing.execute_batch("ROLLBACK").expect("release lock");
        assert_eq!(memory.get(get("notes", "locked")).await?, GetResult::Present {value:json!("before")});
        competing.execute("UPDATE swarmie_memory_entries SET value = 'not-json' WHERE namespace = 'notes' AND key = 'locked'", []).expect("corrupt test fixture");
        assert_eq!(memory.get(get("notes", "locked")).await.unwrap_err().reason, "memory_store_corrupt");
        assert_eq!(memory.get(get("notes", "absent")).await?, GetResult::Missing);
        host.close().await
    })
}

#[test]
fn configuration_and_open_failures_are_explicit_without_fallback() -> Result<()> {
    assert_eq!(
        Configuration {
            database: "".into()
        }
        .validate(),
        Err(ConfigurationError::EmptyDatabase)
    );
    for path in [":memory:", "file::memory:", "file:memory?mode=memory"] {
        let configuration = Configuration {
            database: path.into(),
        };
        assert_eq!(
            configuration.validate(),
            Err(ConfigurationError::NonDurableDatabase)
        );
        assert_eq!(
            swarmie_memory_store::facet(&configuration)
                .err()
                .unwrap()
                .reason,
            "memory_store_configuration"
        );
    }
    assert!(
        serde_json::from_value::<Configuration>(json!({"database":"memory.sqlite","unknown":true}))
            .is_err()
    );
    let directory = tempfile::tempdir().expect("test directory");
    let database = directory
        .path()
        .join("missing-parent")
        .join("memory.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("memory-test", "application", driver.clone())?;
    driver.run_until(async move {
        assert_eq!(
            open(&host, &database).await.err().unwrap().reason,
            "memory_store_storage"
        );
        assert!(!database.exists());
        host.close().await
    })
}

#[test]
fn canonical_descriptor_and_wire_values_preserve_absence_vs_null() -> Result<()> {
    let contract = MemoryStore.descriptor()?;
    assert_eq!(contract.descriptor().name, "swarmie.memory-store");
    assert_eq!(
        serde_json::to_value(GetResult::Missing).unwrap(),
        Value::Null
    );
    assert_eq!(
        serde_json::to_value(GetResult::Present { value: Value::Null }).unwrap(),
        json!({"value":null})
    );
    assert_eq!(
        serde_json::from_value::<GetResult>(Value::Null).unwrap(),
        GetResult::Missing
    );
    assert_eq!(
        serde_json::from_value::<GetResult>(json!({"value":null})).unwrap(),
        GetResult::Present { value: Value::Null }
    );
    assert!(serde_json::from_value::<GetResult>(json!({"value":null,"unexpected":true})).is_err());
    Ok(())
}
