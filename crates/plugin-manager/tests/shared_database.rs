use serde_json::json;
use swarmie_memory_store::{GetRequest, GetResult, MemoryStore, MemoryStoreInput, PutRequest};
use swarmie_plugin_contracts::RUNTIME_HOST;
use swarmie_plugin_manager::PluginManager;
use swarmie_sqlite_local::{Configuration, Database};
use tembra::{Host, core};
use tembra_executor_js::{Executor, ExecutorOptions};

#[test]
fn administration_and_memory_share_one_owner_and_survive_reopen()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let configuration = Configuration {
        database: directory.path().join("application.sqlite"),
    };
    let driver = tembra::native::TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(driver.run_until(async {
            for reopened in [false, true] {
                let host = Host::with_driver(RUNTIME_HOST, "application", driver.clone())?;
                let (owner, handle) = swarmie_sqlite_local::facet(
                    &configuration,
                    vec![
                        swarmie_plugin_manager::sqlite_schema(),
                        swarmie_memory_store::sqlite_schema(),
                    ],
                )?;
                host.install(owner, core::LoadOptions::default())?.await?;
                let physical = handle.database()?;
                let manager = PluginManager::start_with_database(
                    host.clone(),
                    Executor::new(driver.clone(), ExecutorOptions::default())?,
                    handle.clone(),
                )
                .await?;
                host.install(
                    swarmie_memory_store::facet_with_database(&handle)?,
                    core::LoadOptions::default(),
                )?
                .await?;
                let memory = host.get(MemoryStore, MemoryStoreInput {}).await?;
                let lookup = GetRequest {
                    namespace: "notes".into(),
                    key: "shared".into(),
                };
                if reopened {
                    assert_eq!(
                        manager.composition()?,
                        Some(json!({"selection":"retained"}))
                    );
                    assert_eq!(
                        memory.get(lookup.clone()).await?,
                        GetResult::Present {
                            value: json!({"revision":1})
                        }
                    );
                } else {
                    let permit = manager.begin_composition_reconciliation()?;
                    manager.save_composition(&json!({"selection":"retained"}))?;
                    drop(permit);
                    memory
                        .put(PutRequest {
                            namespace: lookup.namespace.clone(),
                            key: lookup.key.clone(),
                            value: json!({"revision":1}),
                        })
                        .await?;
                }
                assert!(
                    matches!(
                        Database::open(&configuration, vec![]),
                        Err(swarmie_sqlite_local::Error::Owned)
                    ),
                    "logical domains do not acquire another physical connection"
                );
                physical.with_connection(|connection| {
                    assert_eq!(
                        connection
                            .query_row("SELECT COUNT(*) FROM composition", [], |row| row
                                .get::<_, u32>(0))?,
                        1
                    );
                    assert_eq!(
                        connection.query_row(
                            "SELECT COUNT(*) FROM swarmie_memory_entries",
                            [],
                            |row| row.get::<_, u32>(0)
                        )?,
                        1
                    );
                    Ok::<_, swarmie_sqlite_local::Error>(())
                })?;
                host.close().await?;
                assert!(manager.composition().is_err());
                assert!(memory.get(lookup).await.is_err());
                assert!(
                    physical
                        .with_connection(|_| Ok::<_, swarmie_sqlite_local::Error>(()))
                        .is_err(),
                    "owner retirement revokes every retained physical clone"
                );
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        }))
}
