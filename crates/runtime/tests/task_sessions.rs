//! Explicit selection and restart through the ordinary runtime catalog.

use serde_json::json;
use swarmie_composition::Selection;
use swarmie_runtime::{Application, Configuration, TransitionOutcome, start};
use swarmie_session_contracts::{CreateSession, EventQuery, SessionStore, SessionStoreInput};
use swarmie_task_contracts::{TaskOutboxCompaction, TaskSpec, TaskStorage, TaskStorageInput};
use swarmie_task_session_contracts::*;
use tembra_native::TokioDriver;

#[test]
fn profile_selects_bridge_and_recovers_pinned_admission_and_publisher() {
    let directory = tempfile::tempdir().unwrap();
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(driver.run_until(async {
        let mut app=Application::default();
        app.composition.profile.disable.push(swarmie_memory_store::PLUGIN_ID.into());
        for (plugin,config) in [
            (swarmie_task_storage_sqlite::PLUGIN_ID,json!({"database":directory.path().join("tasks.sqlite")})),
            (swarmie_session_store_sqlite::PLUGIN_ID,json!({"database":directory.path().join("sessions.sqlite")})),
            (swarmie_durable_tasks::PLUGIN_ID,json!({})),
        ] {
            app.composition.profile.plugins.push(Selection { id:plugin.into(),plugin:plugin.into(),version:env!("CARGO_PKG_VERSION").into(),provider:None,config });
        }
        let configuration=|app| Configuration { database:directory.path().join("admin.sqlite"),source:directory.path().join("source"),application:app };
        let mut runtime=start(driver.clone(),configuration(Some(app.clone()))).await.unwrap();
        assert!(runtime.deployment_errors.is_empty());
        assert!(!runtime.effective.instances.values().any(|i|i.release.as_ref().is_some_and(|r|r.plugin==swarmie_task_session_integration::PLUGIN_ID)));
        let storage=runtime.host.get(TaskStorage,TaskStorageInput {}).await.unwrap();
        let sessions=runtime.host.get(SessionStore,SessionStoreInput {}).await.unwrap();
        let source=storage.outbox_status().await.unwrap().store_id;
        let destination=sessions.create(CreateSession { session_id:"chat".into(),metadata:json!({}) }).await.unwrap().store_id;
        app.composition.profile.plugins.push(Selection { id:"history-bridge".into(),plugin:swarmie_task_session_integration::PLUGIN_ID.into(),version:env!("CARGO_PKG_VERSION").into(),provider:None,config:json!({"database":directory.path().join("bindings.sqlite"),"task_store_id":source,"session_store_id":destination,"consumer_id":"history-v1","auto_publish":false}) });
        let report=runtime.apply_application(app).await.unwrap();
        assert_eq!(report.results["history-bridge"].outcome,TransitionOutcome::RestartRequired);
        runtime.host.close().await.unwrap(); drop(runtime);
        let runtime=start(driver.clone(),configuration(None)).await.unwrap();
        assert!(runtime.deployment_errors.is_empty());
        let bindings=runtime.host.get(TaskSessionBindings,TaskSessionBindingsInput {}).await.unwrap();
        let request=PrepareBinding { task_store_id:source.clone(),task:TaskSpec { id:"task".into(),kind:"fixture.unselected".into(),definition_version:1,input:json!({}),checkpoint:json!({}),phase:"start".into() },destination:SessionDestination { session_store_id:destination,session_id:"chat".into(),conversation_id:"conversation".into(),turn_id:"turn".into() },routing_version:1 };
        let key=BindingKey { task_store_id:source,task_id:"task".into() };
        bindings.prepare(request).await.unwrap(); bindings.admit(key.clone()).await.unwrap();
        runtime.host.close().await.unwrap(); drop(runtime);
        let runtime=start(driver.clone(),configuration(None)).await.unwrap();
        assert!(runtime.deployment_errors.is_empty());
        let bindings=runtime.host.get(TaskSessionBindings,TaskSessionBindingsInput {}).await.unwrap();
        assert_eq!(bindings.require_confirmed_binding(key).await.unwrap().admission,BindingAdmission::Active);
        let publisher=runtime.host.get(TaskSessionPublisher,TaskSessionPublisherInput {}).await.unwrap();
        while publisher.publish(PublishRequest { limit:1 }).await.unwrap().published>0 {}
        let sessions=runtime.host.get(SessionStore,SessionStoreInput {}).await.unwrap();
        assert!(!sessions.events(EventQuery::new("chat")).await.unwrap().events.is_empty());
        assert!(publisher.compact(TaskOutboxCompaction { limit:4096 }).await.unwrap().removed>0);
        runtime.host.close().await.unwrap();
    }));
}
