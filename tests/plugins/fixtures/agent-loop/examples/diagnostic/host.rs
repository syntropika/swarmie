//! Diagnostic composition uses real plugins and the existing synthetic Registry contributors.

use std::{collections::BTreeMap, path::Path, rc::Rc};

use serde_json::json;
use swarmie_agent_contracts::{
    AgentLoop, AgentLoopInput, AgentLoopRef, StartTurn, TurnLimits, TurnLookup,
};
use swarmie_agent_loop_fixture::{
    model::{self, ModelProbe, Reply},
    policy::{self, PolicyProbe},
    tool::{self, ToolProbe},
};
use swarmie_model_contracts::ModelIdentity;
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, MESSAGE, NewEvent, SessionStore, SessionStoreInput,
    SessionStoreRef,
};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, DurableTasksRef, TaskStorage, TaskStorageInput, TaskStorageRef,
};
use swarmie_task_session_contracts::{
    TaskSessionBindings, TaskSessionBindingsInput, TaskSessionPublisher, TaskSessionPublisherInput,
    TaskSessionPublisherRef,
};
use tembra::{core, prelude::*};

use super::metrics::Driver;

pub struct Harness {
    pub host: Host,
    pub agent: AgentLoopRef,
    pub tasks: DurableTasksRef,
    pub sessions: SessionStoreRef,
    pub storage: TaskStorageRef,
    pub publisher: TaskSessionPublisherRef,
    pub model: ModelProbe,
    pub tool: ToolProbe,
    pub policy: PolicyProbe,
    pub task_store_id: String,
    pub request: StartTurn,
}

impl Harness {
    pub fn lookup(&self) -> TurnLookup {
        TurnLookup {
            session_id: self.request.session_id.clone(),
            conversation_id: self.request.conversation_id.clone(),
            turn_id: self.request.turn_id.clone(),
        }
    }
}

pub async fn open(driver: Rc<Driver>, root: &Path, replies: Vec<Reply>) -> Result<Harness> {
    open_scripts(driver, root, Some(replies), BTreeMap::new(), 16).await
}

pub async fn open_routed(
    driver: Rc<Driver>,
    root: &Path,
    scripts: BTreeMap<String, Vec<Reply>>,
    max_parallel: u32,
) -> Result<Harness> {
    open_scripts(driver, root, None, scripts, max_parallel).await
}

async fn open_scripts(
    driver: Rc<Driver>,
    root: &Path,
    replies: Option<Vec<Reply>>,
    scripts: BTreeMap<String, Vec<Reply>>,
    max_parallel: u32,
) -> Result<Harness> {
    let prepare_single = replies.is_some();
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
        swarmie_session_store_sqlite::facet(
            &swarmie_session_store_sqlite::Configuration {
                database: root.join("sessions.sqlite"),
            },
            driver.clone(),
        )?,
        match replies {
            Some(replies) => model::facet(replies, model.clone())?,
            None => model::routed_facet(scripts, model.clone())?,
        },
        tool::facet(Default::default(), tool.clone())?,
        policy::facet(true, policy.clone())?,
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                poll_interval_ms: 2,
                max_parallel,
                ..Default::default()
            },
            driver.clone(),
        )?,
    ] {
        host.install(facet, core::LoadOptions::default())?.await?;
    }
    let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
    let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
    let configuration = swarmie_task_session_integration::Configuration {
        database: root.join("bindings.sqlite"),
        task_store_id: storage.outbox_status().await?.store_id,
        session_store_id: sessions.publication_status().await?.durable.store_id,
        consumer_id: "agent-session-history-v1".into(),
        auto_publish: false,
        poll_interval_ms: 2,
    };
    host.install(
        swarmie_task_session_integration::facet(&configuration, driver.clone())?,
        core::LoadOptions::default(),
    )?
    .await?;
    let _bindings = host
        .get(TaskSessionBindings, TaskSessionBindingsInput {})
        .await?;
    host.install(
        swarmie_agent_loop_default::facet(driver)?,
        core::LoadOptions::default(),
    )?
    .await?;
    let agent = host.get(AgentLoop, AgentLoopInput {}).await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    let publisher = host
        .get(TaskSessionPublisher, TaskSessionPublisherInput {})
        .await?;
    let request = StartTurn {
        turn_id: "published".into(),
        session_id: "session-published".into(),
        conversation_id: "conversation".into(),
        input_event_id: "input-published".into(),
        model: ModelIdentity {
            connection: "fixture".into(),
            model: "scripted".into(),
        },
        limits: TurnLimits::default(),
    };
    let harness = Harness {
        host,
        agent,
        tasks,
        sessions,
        storage,
        publisher,
        model,
        tool,
        policy,
        task_store_id: configuration.task_store_id,
        request,
    };
    if prepare_single {
        prepare(&harness, &harness.request).await?;
    }
    Ok(harness)
}

/// Session preparation is outside the admission-to-terminal observation window.
pub async fn prepare(h: &Harness, request: &StartTurn) -> Result<()> {
    h.sessions
        .create(CreateSession {
            session_id: request.session_id.clone(),
            metadata: json!({}),
        })
        .await?;
    h.sessions.append(AppendEvents {
        session_id: request.session_id.clone(),
        batch_id: request.input_event_id.clone(), expected_head: None,
        events: vec![NewEvent {
            event_id: request.input_event_id.clone(), kind: MESSAGE.into(), version: 1,
            correlation: Correlation { conversation_id: Some(request.conversation_id.clone()), ..Default::default() },
            occurred_at_ms: None,
            payload: json!({"role":"user","content":[{"type":"text","text":"Compute the result."}],"reply_to":null,"tool_call_id":null}),
        }],
    }).await?;
    Ok(())
}
