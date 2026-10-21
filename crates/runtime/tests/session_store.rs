//! Selection through ordinary application composition, without default-enabling new storage.
use serde_json::json;
use swarmie_composition::Selection;
use swarmie_runtime::{Application, Configuration, start};
use swarmie_session_contracts::{CreateSession, SessionStore, SessionStoreInput};
use tembra_native::TokioDriver;

#[test]
fn profile_selects_compiled_session_provider_and_recovers_persistent_session() {
    let directory = tempfile::tempdir().unwrap();
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let mut application = Application::default();
            application
                .composition
                .profile
                .disable
                .push(swarmie_memory_store::PLUGIN_ID.into());
            application.composition.profile.plugins.push(Selection {
                id: "sessions".into(),
                plugin: swarmie_session_store_sqlite::PLUGIN_ID.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                provider: None,
                config: json!({"database":directory.path().join("sessions.sqlite")}),
            });
            for _ in 0..2 {
                let runtime = start(
                    driver.clone(),
                    Configuration {
                        database: directory.path().join("admin.sqlite"),
                        source: directory.path().join("source"),
                        application: Some(application.clone()),
                    },
                )
                .await
                .unwrap();
                let sessions = runtime
                    .host
                    .get(SessionStore, SessionStoreInput {})
                    .await
                    .unwrap();
                let record = sessions
                    .create(CreateSession {
                        session_id: "profile".into(),
                        metadata: json!({"selected":true}),
                    })
                    .await
                    .unwrap();
                assert_eq!(record.head, 0);
                assert_eq!(record.metadata, json!({"selected":true}));
                runtime.host.close().await.unwrap();
            }
        }));
}
