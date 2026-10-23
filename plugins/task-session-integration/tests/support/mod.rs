use std::{future::Future, path::Path, rc::Rc, time::Duration};

use serde_json::json;
use swarmie_session_contracts::*;
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::{core, native::TokioDriver, prelude::*};

pub fn run<F, Fut>(test: F) -> Result<()>
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(30), test(driver.clone()))
                .await
                .expect("bridge acceptance timed out")
        }))
}

pub struct Harness {
    pub host: Host,
    pub storage: TaskStorageRef,
    pub sessions: SessionStoreRef,
    pub tasks: DurableTasksRef,
    pub bindings: TaskSessionBindingsRef,
    pub publisher: TaskSessionPublisherRef,
    pub configuration: swarmie_task_session_integration::Configuration,
}

impl Harness {
    pub async fn open(
        path: &Path,
        driver: Rc<TokioDriver>,
        limits: TaskOutboxLimits,
        auto_publish: bool,
    ) -> Result<Self> {
        Self::open_pinned(path, driver, limits, auto_publish, None).await
    }

    pub async fn open_pinned(
        path: &Path,
        driver: Rc<TokioDriver>,
        limits: TaskOutboxLimits,
        auto_publish: bool,
        pins: Option<swarmie_task_session_integration::Configuration>,
    ) -> Result<Self> {
        let host = Host::with_driver("bridge-acceptance", "application", driver.clone())?;
        host.install(
            swarmie_task_storage_sqlite::facet(
                &serde_json::from_value(
                    json!({"database":path.join("tasks.sqlite"),"outbox":limits}),
                )
                .unwrap(),
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            swarmie_session_store_sqlite::facet(
                &swarmie_session_store_sqlite::Configuration {
                    database: path.join("sessions.sqlite"),
                },
                driver.clone(),
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            swarmie_durable_tasks::facet(
                &swarmie_durable_tasks::Configuration::default(),
                driver.clone(),
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
        let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
        let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
        let source = storage.identity().await?.store_id;
        let destination = sessions.identity().await?.store_id;
        // The cheap identity probes and the diagnostics describe the same live stores.
        assert_eq!(source, storage.outbox_status().await?.store_id);
        assert_eq!(
            destination,
            sessions.publication_status().await?.durable.store_id
        );
        let configuration = pins.unwrap_or(swarmie_task_session_integration::Configuration {
            database: path.join("bindings.sqlite"),
            task_store_id: source,
            session_store_id: destination,
            consumer_id: "session-history-v1".into(),
            auto_publish,
            poll_interval_ms: 5,
        });
        if let Err(error) = host
            .install(
                swarmie_task_session_integration::facet(&configuration, driver)?,
                core::LoadOptions::default(),
            )?
            .await
        {
            host.close().await?;
            return Err(error);
        }
        let bindings = host
            .get(TaskSessionBindings, TaskSessionBindingsInput {})
            .await?;
        let publisher = host
            .get(TaskSessionPublisher, TaskSessionPublisherInput {})
            .await?;
        Ok(Self {
            host,
            storage,
            sessions,
            tasks,
            bindings,
            publisher,
            configuration,
        })
    }

    pub async fn request(
        &self,
        task_id: &str,
        session: &str,
        conversation: &str,
    ) -> Result<PrepareBinding> {
        self.sessions
            .create(CreateSession {
                session_id: session.into(),
                metadata: json!({}),
            })
            .await?;
        Ok(PrepareBinding {
            task_store_id: self.configuration.task_store_id.clone(),
            task: spec(task_id),
            destination: SessionDestination {
                session_store_id: self.configuration.session_store_id.clone(),
                session_id: session.into(),
                conversation_id: conversation.into(),
                turn_id: "turn".into(),
            },
            routing_version: 1,
        })
    }
}

pub fn spec(id: &str) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        kind: "fixture.bridge".into(),
        definition_version: 1,
        input: json!({"stable":"input"}),
        checkpoint: json!({}),
        phase: "start".into(),
    }
}

pub fn key(request: &PrepareBinding) -> BindingKey {
    BindingKey {
        task_store_id: request.task_store_id.clone(),
        task_id: request.task.id.clone(),
    }
}

pub async fn drain(publisher: &TaskSessionPublisherRef) -> Result<u32> {
    let mut total = 0;
    loop {
        let result = publisher.publish(PublishRequest { limit: 2 }).await?;
        let count = result.published + result.excluded;
        total += count;
        if count == 0 {
            return Ok(total);
        }
    }
}
