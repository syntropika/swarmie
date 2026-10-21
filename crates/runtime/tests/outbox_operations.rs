//! Application configuration and provider monitoring without a scheduler or publisher.

use std::{cell::RefCell, path::Path, rc::Rc};

use futures::FutureExt;

use serde_json::json;
use swarmie_composition::Selection;
use swarmie_runtime::{Application, Configuration, Runtime};
use swarmie_task_contracts::*;
use tembra::{Context, Host, NativeFacet, Result, core, native::TokioDriver};

/// Publisher operations enter through their installed Facet's live managed root.
struct Publisher {
    storage: TaskStorageRef,
    scope: core::FacetScope,
}

impl Publisher {
    async fn install(host: &Host, storage: TaskStorageRef) -> Self {
        let selected = Rc::new(RefCell::new(None));
        let selected_scope = selected.clone();
        host.install(
            NativeFacet::new("fixture.operational-publisher", vec![], move |scope, _| {
                *selected_scope.borrow_mut() = Some(scope.clone());
                Ok(())
            })
            .unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
        let scope = selected.borrow().clone().unwrap();
        Self { storage, scope }
    }

    async fn register(&self) -> Result<TaskOutboxConsumerState> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .register_outbox_consumer(TaskOutboxConsumer {
                            id: "publisher".into(),
                        })
                        .await
                }
                .boxed_local()
            })
            .await
    }

    async fn read(&self) -> Result<TaskOutboxPage> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .read_outbox(TaskOutboxQuery {
                            consumer_id: "publisher".into(),
                            after: None,
                            limit: 1,
                        })
                        .await
                }
                .boxed_local()
            })
            .await
    }

    async fn acknowledge(&self, through: TaskOutboxEventId) -> Result<()> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .acknowledge_outbox(TaskOutboxAcknowledgement {
                            consumer_id: "publisher".into(),
                            through,
                        })
                        .await
                }
                .boxed_local()
            })
            .await
    }
}

async fn start(driver: Rc<TokioDriver>, root: &Path, capacity: u64) -> Runtime {
    let mut application = Application::default();
    application.composition.profile.disable = vec![
        "swarmie.memory-store".into(),
        "swarmie.local-plugin-source".into(),
    ];
    application.composition.profile.plugins.push(Selection {
        id: "storage".into(),
        plugin: swarmie_task_storage_sqlite::PLUGIN_ID.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        provider: None,
        config: json!({
            "database": root.join("tasks.sqlite"),
            "outbox": { "max_events": capacity }
        }),
    });
    let runtime = swarmie_runtime::start(
        driver,
        Configuration {
            database: root.join("administration.sqlite"),
            source: root.join("source"),
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
    assert!(runtime.recovery_error.is_none());
    runtime
}

fn transaction(id: &str) -> TaskTransaction {
    TaskTransaction {
        owner: "operator".into(),
        writes: vec![TaskWrite {
            provenance: None,
            expected_revision: None,
            record: TaskRecord::new(
                TaskSpec {
                    id: id.into(),
                    kind: "fixture.operational-capacity".into(),
                    definition_version: 1,
                    phase: "start".into(),
                    input: json!(null),
                    checkpoint: json!(null),
                },
                None,
            ),
        }],
    }
}

#[test]
fn json_capacity_and_monitoring_allow_supervised_recovery_without_recompilation() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let runtime = start(driver.clone(), directory.path(), 1).await;
            let storage = runtime
                .host
                .get(TaskStorage, TaskStorageInput {})
                .await
                .unwrap();
            storage
                .claim_owner(StorageOwner {
                    id: "operator".into(),
                })
                .await
                .unwrap();
            storage.commit(transaction("first")).await.unwrap();
            let error = storage.commit(transaction("refused")).await.unwrap_err();
            assert_eq!(error.reason, "task_storage_outbox_full");
            // Monitoring remains available at capacity, including with no registered consumer.
            let before = storage.outbox_status().await.unwrap();
            assert_eq!(before.limits.max_events, 1);
            assert_eq!(
                before.limits.max_bytes,
                TaskOutboxLimits::default().max_bytes
            );
            assert_eq!(before.retained_events, 1);
            assert_eq!(before.pending_events, 1);
            assert_eq!(before.compactable_events, 0);
            assert!(before.consumers.is_empty());
            runtime.host.close().await.unwrap();
            drop(runtime);

            // A fresh Host applies larger ordinary JSON settings to the same retained database.
            let runtime = start(driver.clone(), directory.path(), 3).await;
            let storage = runtime
                .host
                .get(TaskStorage, TaskStorageInput {})
                .await
                .unwrap();
            let reopened = storage.outbox_status().await.unwrap();
            assert_eq!(reopened.store_id, before.store_id);
            assert_eq!(reopened.retained_events, 1);
            assert_eq!(reopened.retained_bytes, before.retained_bytes);
            assert_eq!(reopened.limits.max_events, 3);
            storage
                .claim_owner(StorageOwner {
                    id: "operator".into(),
                })
                .await
                .unwrap();
            storage.commit(transaction("second")).await.unwrap();
            let publisher = Publisher::install(&runtime.host, storage.clone()).await;
            publisher.register().await.unwrap();
            let page = publisher.read().await.unwrap();
            assert_eq!(page.events[0].task_id, "first");
            let first = page.events[0].id.clone();
            let delivered = storage.outbox_status().await.unwrap();
            assert_eq!(delivered.retained_events, 2);
            assert_eq!(delivered.pending_events, 2);
            assert_eq!(delivered.compactable_events, 0);
            assert_eq!(delivered.consumers.len(), 1);
            assert_eq!(delivered.consumers[0].acknowledged, None);
            assert_eq!(delivered.consumers[0].delivered.as_ref(), Some(&first));
            assert_eq!(
                storage.outbox_status().await.unwrap(),
                delivered,
                "monitoring must not acknowledge or advance delivery"
            );
            publisher.acknowledge(first.clone()).await.unwrap();
            let acknowledged = storage.outbox_status().await.unwrap();
            assert_eq!(acknowledged.compactable_events, 1);
            assert_eq!(acknowledged.pending_events, 1);
            // Acknowledgement alone does not silently delete retained rows.
            assert_eq!(acknowledged.retained_events, 2);
            let removed = storage
                .compact_outbox(TaskOutboxCompaction { limit: 256 })
                .await
                .unwrap();
            assert_eq!(removed.removed, 1);
            let compacted = storage.outbox_status().await.unwrap();
            assert_eq!(compacted.retained_events, 1);
            assert_eq!(compacted.pending_events, 1);
            assert_eq!(compacted.retained_after, Some(first));
            assert_eq!(
                storage.scan(TaskQuery::default()).await.unwrap().sequence,
                2
            );
            runtime.host.close().await.unwrap();
        }));
}
