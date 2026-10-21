//! Shared real-Host setup for the acceptance suites.

use serde_json::json;
use std::{future::Future, path::Path, rc::Rc, time::Duration};
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{
    model::{self, ModelProbe, Reply},
    policy::{self, PolicyProbe},
    tool::{self, ToolConfiguration, ToolProbe},
};
use swarmie_model_contracts::ModelIdentity;
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, EventQuery, MESSAGE, NewEvent, SessionEvent,
    SessionStore, SessionStoreInput, SessionStoreRef,
};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, DurableTasksRef, TaskLookup, TaskStorage, TaskStorageInput,
    TaskStorageRef,
};
use swarmie_task_session_contracts::{
    TaskSessionBindings, TaskSessionBindingsInput, TaskSessionBindingsRef, TaskSessionPublisher,
    TaskSessionPublisherInput, TaskSessionPublisherRef,
};
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

pub fn run<F, Fut>(test: F)
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
                .expect("agent acceptance timed out")
                .unwrap();
        }));
}

pub struct Harness {
    pub host: Host,
    pub agent: AgentLoopRef,
    pub tasks: DurableTasksRef,
    pub sessions: SessionStoreRef,
    pub storage: TaskStorageRef,
    pub bindings: TaskSessionBindingsRef,
    pub binding_configuration: swarmie_task_session_integration::Configuration,
    pub model: ModelProbe,
    pub tool: ToolProbe,
    pub policy: PolicyProbe,
}

pub async fn open(
    driver: Rc<TokioDriver>,
    root: &Path,
    replies: Vec<Reply>,
    tool_configuration: ToolConfiguration,
    allowed: bool,
) -> Result<Harness> {
    open_selected(
        driver,
        root,
        replies,
        tool_configuration,
        Some(allowed),
        true,
    )
    .await
}

pub async fn open_selected(
    driver: Rc<TokioDriver>,
    root: &Path,
    replies: Vec<Reply>,
    tool_configuration: ToolConfiguration,
    allowed: Option<bool>,
    include_tool: bool,
) -> Result<Harness> {
    open_with_loop(
        driver,
        root,
        replies,
        tool_configuration,
        allowed,
        include_tool,
        LoopSetup::default(),
    )
    .await
}

pub async fn open_with_contributions(
    driver: Rc<TokioDriver>,
    root: &Path,
    replies: Vec<Reply>,
    contributions: Vec<NativeFacet>,
) -> Result<Harness> {
    open_with_loop(
        driver,
        root,
        replies,
        Default::default(),
        Some(true),
        true,
        LoopSetup {
            contributions,
            ..Default::default()
        },
    )
    .await
}

pub async fn open_degraded_bindings(
    driver: Rc<TokioDriver>,
    root: &Path,
    configuration: swarmie_task_session_integration::Configuration,
) -> Result<Harness> {
    open_with_loop(
        driver,
        root,
        vec![Reply::text("must not execute")],
        Default::default(),
        Some(true),
        true,
        LoopSetup {
            binding_configuration: Some(configuration),
            tolerate_binding_startup_failure: true,
            ..Default::default()
        },
    )
    .await
}

pub async fn open_replacement(driver: Rc<TokioDriver>, root: &Path) -> Result<Harness> {
    open_with_loop(
        driver,
        root,
        vec![Reply::text("unused")],
        Default::default(),
        None,
        false,
        LoopSetup {
            replacement: true,
            ..Default::default()
        },
    )
    .await
}

#[derive(Default)]
struct LoopSetup {
    replacement: bool,
    contributions: Vec<NativeFacet>,
    binding_configuration: Option<swarmie_task_session_integration::Configuration>,
    tolerate_binding_startup_failure: bool,
    session_provider: Option<NativeFacet>,
}

pub async fn open_with_session_provider(
    driver: Rc<TokioDriver>,
    root: &Path,
    session_provider: NativeFacet,
) -> Result<Harness> {
    open_with_loop(
        driver,
        root,
        vec![Reply::text("verified destination")],
        Default::default(),
        Some(true),
        true,
        LoopSetup {
            session_provider: Some(session_provider),
            ..Default::default()
        },
    )
    .await
}

async fn open_with_loop(
    driver: Rc<TokioDriver>,
    root: &Path,
    replies: Vec<Reply>,
    tool_configuration: ToolConfiguration,
    allowed: Option<bool>,
    include_tool: bool,
    setup: LoopSetup,
) -> Result<Harness> {
    let LoopSetup {
        replacement,
        contributions,
        binding_configuration,
        tolerate_binding_startup_failure,
        session_provider,
    } = setup;
    let host = Host::with_driver("agent-acceptance", "application", driver.clone())?;
    let model = ModelProbe::default();
    let tool = ToolProbe::default();
    let policy = PolicyProbe::default();
    for facet in [
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            database: root.join("tasks.sqlite"),
            outbox: Default::default(),
            outbox_legacy_owners: Default::default(),
        })?,
        match session_provider {
            Some(provider) => provider,
            None => swarmie_session_store_sqlite::facet(
                &swarmie_session_store_sqlite::Configuration {
                    database: root.join("sessions.sqlite"),
                },
                driver.clone(),
            )?,
        },
        model::facet(replies, model.clone())?,
    ] {
        host.install(facet, core::LoadOptions::default())?.await?;
    }
    if include_tool {
        host.install(
            tool::facet(tool_configuration, tool.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
    }
    if let Some(allowed) = allowed {
        host.install(
            policy::facet(allowed, policy.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
    }
    for contribution in contributions {
        host.install(contribution, core::LoadOptions::default())?
            .await?;
    }
    host.install(
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                poll_interval_ms: 2,
                ..Default::default()
            },
            driver.clone(),
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
    let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
    let binding_configuration =
        binding_configuration.unwrap_or(swarmie_task_session_integration::Configuration {
            database: root.join("bindings.sqlite"),
            task_store_id: storage.outbox_status().await?.store_id,
            session_store_id: sessions.publication_status().await?.durable.store_id,
            consumer_id: "agent-session-history-v1".into(),
            auto_publish: false,
            poll_interval_ms: 2,
        });
    if let Err(error) = host
        .install(
            swarmie_task_session_integration::facet(&binding_configuration, driver.clone())?,
            core::LoadOptions::default(),
        )?
        .await
    {
        if !tolerate_binding_startup_failure {
            host.close().await?;
            return Err(error);
        }
        assert_eq!(error.reason, "binding_store_mismatch");
    }
    let bindings = host
        .get(TaskSessionBindings, TaskSessionBindingsInput {})
        .await?;
    host.install(
        if replacement {
            swarmie_agent_loop_fixture::replacement::facet()?
        } else {
            swarmie_agent_loop_default::facet(driver)?
        },
        core::LoadOptions::default(),
    )?
    .await?;
    let agent = host.get(AgentLoop, AgentLoopInput {}).await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
    Ok(Harness {
        host,
        agent,
        tasks,
        sessions,
        storage,
        bindings,
        binding_configuration,
        model,
        tool,
        policy,
    })
}

impl Harness {
    pub async fn publisher(&self) -> Result<TaskSessionPublisherRef> {
        self.host
            .get(TaskSessionPublisher, TaskSessionPublisherInput {})
            .await
    }

    pub fn lookup(turn: &str) -> TurnLookup {
        TurnLookup {
            session_id: format!("session-{turn}"),
            conversation_id: "conversation".into(),
            turn_id: turn.into(),
        }
    }

    pub async fn start(&self, turn: &str, limits: TurnLimits) -> Result<TurnRecord> {
        let session_id = format!("session-{turn}");
        let input_event_id = format!("input-{turn}");
        self.sessions
            .create(CreateSession {
                session_id: session_id.clone(),
                metadata: json!({}),
            })
            .await?;
        self.sessions
            .append(AppendEvents {
                session_id: session_id.clone(),
                batch_id: input_event_id.clone(),
                expected_head: None,
                events: vec![NewEvent {
                    event_id: input_event_id.clone(),
                    kind: MESSAGE.into(),
                    version: 1,
                    correlation: Correlation {
                        conversation_id: Some("conversation".into()),
                        ..Default::default()
                    },
                    occurred_at_ms: None,
                    payload: serde_json::to_value(swarmie_session_contracts::Message {
                        role: "user".into(),
                        content: json!([{"type":"text","text":"Compute the result."}]),
                        reply_to: None,
                        tool_call_id: None,
                    })
                    .unwrap(),
                }],
            })
            .await?;
        self.agent
            .start(StartTurn {
                turn_id: turn.into(),
                session_id,
                conversation_id: "conversation".into(),
                input_event_id,
                model: ModelIdentity {
                    connection: "fixture".into(),
                    model: "scripted".into(),
                },
                limits,
            })
            .await
    }

    pub async fn terminal(&self, turn: &str) -> Result<TurnRecord> {
        let record = self.agent.inspect(Harness::lookup(turn)).await?.unwrap();
        let waiting = tokio::time::timeout(
            Duration::from_secs(10),
            self.tasks.wait(TaskLookup { id: record.task.id }),
        )
        .await;
        if let Err(error) = waiting {
            let record = self.agent.inspect(Harness::lookup(turn)).await?;
            panic!("turn did not settle: {error}; record={record:?}");
        }
        waiting.unwrap()?;
        Ok(self.agent.inspect(Harness::lookup(turn)).await?.unwrap())
    }

    pub async fn events(&self, turn: &str) -> Result<Vec<SessionEvent>> {
        let mut query = EventQuery::new(format!("session-{turn}"));
        let mut result = Vec::new();
        loop {
            let page = self.sessions.events(query.clone()).await?;
            result.extend(page.events);
            let Some(cursor) = page.next else {
                break;
            };
            query.cursor = Some(cursor);
        }
        Ok(result)
    }
}
