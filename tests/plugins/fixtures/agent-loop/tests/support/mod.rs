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
use swarmie_task_contracts::{DurableTasks, DurableTasksInput, DurableTasksRef, TaskLookup};
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
            tokio::time::timeout(Duration::from_secs(20), test(driver.clone()))
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
        false,
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
        true,
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
    replacement: bool,
) -> Result<Harness> {
    let host = Host::with_driver("agent-acceptance", "application", driver.clone())?;
    let model = ModelProbe::default();
    let tool = ToolProbe::default();
    let policy = PolicyProbe::default();
    for facet in [
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            database: root.join("tasks.sqlite"),
        })?,
        swarmie_session_store_sqlite::facet(&swarmie_session_store_sqlite::Configuration {
            database: root.join("sessions.sqlite"),
        })?,
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
        model,
        tool,
        policy,
    })
}

impl Harness {
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
                    correlation: Correlation::default(),
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
        let record = self
            .agent
            .inspect(TurnLookup {
                turn_id: turn.into(),
            })
            .await?
            .unwrap();
        let waiting = tokio::time::timeout(
            Duration::from_secs(5),
            self.tasks.wait(TaskLookup { id: record.task.id }),
        )
        .await;
        if let Err(error) = waiting {
            let record = self
                .agent
                .inspect(TurnLookup {
                    turn_id: turn.into(),
                })
                .await?;
            panic!("turn did not settle: {error}; record={record:?}");
        }
        waiting.unwrap()?;
        Ok(self
            .agent
            .inspect(TurnLookup {
                turn_id: turn.into(),
            })
            .await?
            .unwrap())
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
