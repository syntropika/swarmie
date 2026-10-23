//! Production catalog ownership and exact joint-commit replay after a fresh Host.

use serde_json::json;
use swarmie_composition::Selection;
use swarmie_runtime::{Application, Configuration, start};
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, NewEvent, SessionLookup, SessionStore,
    SessionStoreInput,
};
use swarmie_sqlite_local::{SqliteLocal, SqliteLocalInput};
use swarmie_task_contracts::{
    CommitResult, StorageOwner, TaskLookup, TaskRecord, TaskSessionAppend, TaskSpec, TaskStorage,
    TaskStorageInput, TaskTransaction, TaskWrite,
};
use swarmie_task_session_contracts::{
    TaskSessionCommit, TaskSessionCommitInput, TaskSessionCommitOperation,
    TaskSessionCommitRequest, TaskSessionCommitResult,
};
use tembra_native::TokioDriver;

fn append(id: &str, head: u64) -> AppendEvents {
    AppendEvents {
        session_id: "session".into(),
        batch_id: id.into(),
        expected_head: Some(head),
        events: vec![NewEvent {
            event_id: id.into(),
            kind: "test.intent".into(),
            version: 1,
            occurred_at_ms: None,
            payload: json!({"next":"model"}),
            correlation: Correlation {
                task_id: Some("task".into()),
                ..Default::default()
            },
        }],
    }
}

#[test]
fn production_shared_owner_replays_original_commit_after_domains_advance_and_restart() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let database = directory.path().join("node.sqlite");
            let mut application = Application::default();
            application.composition.profile.plugins.push(Selection {
                id: "local-history".into(),
                plugin: swarmie_task_session_sqlite::PLUGIN_ID.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                provider: None,
                config: json!({}),
            });
            let runtime = start(
                driver.clone(),
                Configuration {
                    database: database.clone(),
                    source: directory.path().join("source"),
                    application: Some(application),
                },
            )
            .await
            .unwrap();
            assert!(
                runtime.deployment_errors.is_empty(),
                "{:?}",
                runtime.deployment_errors
            );
            let physical = runtime
                .host
                .get(SqliteLocal, SqliteLocalInput {})
                .await
                .unwrap();
            let physical_id = physical.identity().await.unwrap();
            let storage = runtime
                .host
                .get(TaskStorage, TaskStorageInput {})
                .await
                .unwrap();
            let sessions = runtime
                .host
                .get(SessionStore, SessionStoreInput {})
                .await
                .unwrap();
            let joint = runtime
                .host
                .get(TaskSessionCommit, TaskSessionCommitInput {})
                .await
                .unwrap();
            let identity = joint.identity().await.unwrap();
            assert_eq!(
                identity.task_store_id,
                storage.identity().await.unwrap().store_id
            );
            assert_eq!(
                identity.session_store_id,
                sessions.identity().await.unwrap().store_id
            );
            sessions
                .create(CreateSession {
                    session_id: "session".into(),
                    metadata: json!({}),
                })
                .await
                .unwrap();
            storage
                .claim_owner(StorageOwner {
                    id: "first-owner".into(),
                })
                .await
                .unwrap();
            let record = TaskRecord::new(
                TaskSpec {
                    id: "task".into(),
                    kind: "test.task".into(),
                    definition_version: 1,
                    input: json!({}),
                    checkpoint: json!({}),
                    phase: "model".into(),
                },
                None,
            );
            let operation = TaskSessionCommitOperation::new(
                identity.clone(),
                vec![TaskWrite {
                    provenance: None,
                    expected_revision: None,
                    record: record.clone(),
                }],
                vec![TaskSessionAppend {
                    session_store_id: identity.session_store_id.clone(),
                    append: append("first", 0),
                }],
            )
            .unwrap();
            let first = joint
                .commit(TaskSessionCommitRequest {
                    owner: "first-owner".into(),
                    operation: operation.clone(),
                })
                .await
                .unwrap();
            assert!(matches!(first, TaskSessionCommitResult::Committed { .. }));
            assert_eq!(
                storage.get(TaskLookup { id: "task".into() }).await.unwrap(),
                Some(record.clone())
            );
            assert_eq!(
                sessions
                    .open(SessionLookup {
                        session_id: "session".into()
                    })
                    .await
                    .unwrap()
                    .unwrap()
                    .head,
                1
            );
            let mut advanced = record;
            advanced.revision = 2;
            advanced.checkpoint = json!({"advanced":true});
            assert!(matches!(
                storage
                    .commit(TaskTransaction {
                        owner: "first-owner".into(),
                        writes: vec![TaskWrite {
                            provenance: None,
                            expected_revision: Some(1),
                            record: advanced.clone(),
                        }]
                    })
                    .await
                    .unwrap(),
                CommitResult::Committed { .. }
            ));
            sessions.append(append("second", 1)).await.unwrap();
            runtime.host.close().await.unwrap();
            assert!(joint.identity().await.is_err());
            drop(runtime);
            let reopened = start(
                driver.clone(),
                Configuration {
                    database,
                    source: directory.path().join("source"),
                    application: None,
                },
            )
            .await
            .unwrap();
            assert!(
                reopened.deployment_errors.is_empty(),
                "{:?}",
                reopened.deployment_errors
            );
            assert_eq!(
                reopened
                    .host
                    .get(SqliteLocal, SqliteLocalInput {})
                    .await
                    .unwrap()
                    .identity()
                    .await
                    .unwrap(),
                physical_id
            );
            let storage = reopened
                .host
                .get(TaskStorage, TaskStorageInput {})
                .await
                .unwrap();
            let sessions = reopened
                .host
                .get(SessionStore, SessionStoreInput {})
                .await
                .unwrap();
            let joint = reopened
                .host
                .get(TaskSessionCommit, TaskSessionCommitInput {})
                .await
                .unwrap();
            assert!(
                joint
                    .commit(TaskSessionCommitRequest {
                        owner: "first-owner".into(),
                        operation: operation.clone()
                    })
                    .await
                    .is_err()
            );
            storage
                .claim_owner(StorageOwner {
                    id: "replacement-owner".into(),
                })
                .await
                .unwrap();
            let replay = joint
                .commit(TaskSessionCommitRequest {
                    owner: "replacement-owner".into(),
                    operation,
                })
                .await
                .unwrap();
            assert_eq!(replay, first);
            assert_eq!(
                storage.get(TaskLookup { id: "task".into() }).await.unwrap(),
                Some(advanced)
            );
            assert_eq!(
                sessions
                    .open(SessionLookup {
                        session_id: "session".into()
                    })
                    .await
                    .unwrap()
                    .unwrap()
                    .head,
                2
            );
            let sqlite_files = std::fs::read_dir(directory.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "sqlite"))
                .collect::<Vec<_>>();
            assert_eq!(
                sqlite_files.len(),
                1,
                "all selected domains share the node database"
            );
            reopened.host.close().await.unwrap();
        }));
}
