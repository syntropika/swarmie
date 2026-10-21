//! Joint Profile selection and fresh-Host recovery of independently composed capabilities.

use serde_json::json;
use swarmie_agent_contracts::{AgentLoop, AgentLoopInput, TurnLookup};
use swarmie_composition::Selection;
use swarmie_model_contracts::{
    ModelEndpoint, ModelIdentity, ModelProviderSchemaInput, ProviderRegistration, Support,
    model_providers,
};
use swarmie_plugin_contracts::ObservedState;
use swarmie_runtime::{Application, Configuration, start};
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, NewEvent, SessionStore, SessionStoreInput,
};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, TaskLookup, TaskSpec, TaskStorage, TaskStorageInput,
};
use swarmie_task_session_contracts::*;
use tembra_native::TokioDriver;

fn selection(id: &str, plugin: &str, config: serde_json::Value) -> Selection {
    Selection {
        id: id.into(),
        plugin: plugin.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        provider: None,
        config,
    }
}

#[test]
fn all_selected_core_capabilities_recover_binding_publication_and_the_profile() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let mut application = Application::default();
            application.composition.profile.disable = vec![
                swarmie_memory_store::PLUGIN_ID.into(),
                "swarmie.local-plugin-source".into(),
            ];
            application.composition.profile.plugins.extend([
                selection(
                    "sessions",
                    swarmie_session_store_sqlite::PLUGIN_ID,
                    json!({"database": directory.path().join("sessions.sqlite")}),
                ),
                selection(
                    "storage",
                    swarmie_task_storage_sqlite::PLUGIN_ID,
                    json!({"database": directory.path().join("tasks.sqlite")}),
                ),
                selection("tasks", swarmie_durable_tasks::PLUGIN_ID, json!({})),
                selection(
                    "models",
                    swarmie_model_provider_genai::PLUGIN_ID,
                    json!({"connections": [{
                        "id": "local", "provider": "ollama", "models": ["configured-model"]
                    }]}),
                ),
            ]);
            let initial = start(
                driver.clone(),
                Configuration {
                    database: directory.path().join("administration.sqlite"),
                    source: directory.path().join("source"),
                    application: Some(application.clone()),
                },
            )
            .await
            .unwrap();
            assert!(initial.deployment_errors.is_empty());
            let task_store_id = initial
                .host
                .get(TaskStorage, TaskStorageInput {})
                .await
                .unwrap()
                .outbox_status()
                .await
                .unwrap()
                .store_id;
            let session_store_id = initial
                .host
                .get(SessionStore, SessionStoreInput {})
                .await
                .unwrap()
                .publication_status()
                .await
                .unwrap()
                .durable
                .store_id;
            initial.host.close().await.unwrap();
            drop(initial);
            application.composition.profile.plugins.extend([
                selection(
                    "history-bridge",
                    swarmie_task_session_integration::PLUGIN_ID,
                    json!({
                        "database":directory.path().join("bindings.sqlite"),
                        "task_store_id":task_store_id, "session_store_id":session_store_id,
                        "consumer_id":"core-history", "auto_publish":false,
                    }),
                ),
                selection("agent", swarmie_agent_loop_default::PLUGIN_ID, json!({})),
            ]);
            let append = AppendEvents {
                session_id: "history".into(),
                batch_id: "stable-batch".into(),
                expected_head: None,
                events: vec![NewEvent {
                    event_id: "stable-event".into(),
                    kind: "fixture.composition".into(),
                    version: 1,
                    correlation: Correlation::default(),
                    occurred_at_ms: None,
                    payload: json!({"committed": true}),
                }],
            };
            let mut receipt = None;
            let mut task_store = None;
            for reopening in [false, true] {
                let runtime = start(
                    driver.clone(),
                    Configuration {
                        database: directory.path().join("administration.sqlite"),
                        source: directory.path().join("source"),
                        application: (!reopening).then(|| application.clone()),
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
                let inventory = runtime.manager.inventory().unwrap();
                for plugin in [
                    swarmie_session_store_sqlite::PLUGIN_ID,
                    swarmie_task_storage_sqlite::PLUGIN_ID,
                    swarmie_durable_tasks::PLUGIN_ID,
                    swarmie_model_provider_genai::PLUGIN_ID,
                    swarmie_task_session_integration::PLUGIN_ID,
                    swarmie_agent_loop_default::PLUGIN_ID,
                ] {
                    assert!(
                        inventory.plugins.iter().any(|entry| {
                            entry.plugin_id == plugin && entry.observed == ObservedState::Active
                        }),
                        "{plugin} was not active"
                    );
                }
                let sessions = runtime
                    .host
                    .get(SessionStore, SessionStoreInput {})
                    .await
                    .unwrap();
                let session = sessions
                    .create(CreateSession {
                        session_id: "history".into(),
                        metadata: json!({"selected": true}),
                    })
                    .await
                    .unwrap();
                if reopening {
                    assert!(session.head >= 1);
                } else {
                    assert_eq!(session.head, 0);
                }
                let committed = sessions.append(append.clone()).await.unwrap();
                assert_eq!(committed.store_id, session.store_id);
                assert_eq!(committed.last_sequence, 1);
                if let Some(previous) = &receipt {
                    assert_eq!(&committed, previous);
                }
                receipt = Some(committed);

                let tasks = runtime
                    .host
                    .get(DurableTasks, DurableTasksInput {})
                    .await
                    .unwrap();
                let bindings = runtime
                    .host
                    .get(TaskSessionBindings, TaskSessionBindingsInput {})
                    .await
                    .unwrap();
                let key = BindingKey {
                    task_store_id: task_store_id.clone(),
                    task_id: "retained-task".into(),
                };
                if !reopening {
                    bindings
                        .prepare(PrepareBinding {
                            task_store_id: task_store_id.clone(),
                            task: TaskSpec {
                                id: "retained-task".into(),
                                kind: "fixture.unavailable".into(),
                                definition_version: 1,
                                input: json!({"retained":true}),
                                checkpoint: json!({}),
                                phase: "start".into(),
                            },
                            destination: SessionDestination {
                                session_store_id: session_store_id.clone(),
                                session_id: "history".into(),
                                conversation_id: "conversation".into(),
                                turn_id: "core-turn".into(),
                            },
                            routing_version: 1,
                        })
                        .await
                        .unwrap();
                    bindings.admit(key.clone()).await.unwrap();
                }
                assert_eq!(
                    bindings
                        .require_confirmed_binding(key)
                        .await
                        .unwrap()
                        .admission,
                    BindingAdmission::Active
                );
                let publisher = runtime
                    .host
                    .get(TaskSessionPublisher, TaskSessionPublisherInput {})
                    .await
                    .unwrap();
                let published = publisher
                    .publish(PublishRequest { limit: 128 })
                    .await
                    .unwrap();
                if !reopening {
                    assert!(published.published > 0);
                }
                let agent = runtime
                    .host
                    .get(AgentLoop, AgentLoopInput {})
                    .await
                    .unwrap();
                assert!(
                    agent
                        .inspect(TurnLookup {
                            session_id: "history".into(),
                            conversation_id: "conversation".into(),
                            turn_id: "absent-turn".into()
                        })
                        .await
                        .unwrap()
                        .is_none()
                );
                let retained = tasks
                    .get(TaskLookup {
                        id: "retained-task".into(),
                    })
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(retained.input, json!({"retained": true}));
                let storage = runtime
                    .host
                    .get(TaskStorage, TaskStorageInput {})
                    .await
                    .unwrap();
                let status = storage.outbox_status().await.unwrap();
                assert!(status.retained_events > 0);
                if let Some(previous) = &task_store {
                    assert_eq!(&status.store_id, previous);
                }
                task_store = Some(status.store_id);

                let registry = runtime
                    .host
                    .core()
                    .registry(&model_providers().unwrap())
                    .unwrap();
                let table = registry.table(None).unwrap();
                let registration: ProviderRegistration = serde_json::from_value(
                    table
                        .get(swarmie_model_provider_genai::PLUGIN_ID)
                        .unwrap()
                        .data()
                        .clone(),
                )
                .unwrap();
                let provider = runtime
                    .host
                    .get(
                        ModelEndpoint::new(&registration.provider_contract).unwrap(),
                        ModelProviderSchemaInput {},
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    provider
                        .capabilities(ModelIdentity {
                            connection: "local".into(),
                            model: "configured-model".into(),
                        })
                        .await
                        .unwrap()
                        .streaming,
                    Support::Supported
                );
                runtime.host.close().await.unwrap();
                assert!(registry.table(None).is_err());
            }
        }));
}
