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
    database: swarmie_sqlite_local::Database,
}

impl Harness {
    /// Counts origin-projected facts without asking the publisher to deliver or ACK them.
    pub fn projected_facts(&self) -> Result<u64> {
        self.database.with_connection(|connection| {
            connection.query_row("SELECT COUNT(*) FROM session_events WHERE json_extract(event, '$.event.kind') = 'swarmie.task-transition'", [], |r| r.get(0))
                .map_err(swarmie_sqlite_local::Error::from)
        }).map_err(Into::into)
    }

    /// Reads the actual selected owner outside the execution measurement window.
    pub fn persistence(&self) -> Result<serde_json::Value> {
        self.database.with_connection(|connection| {
            let journal_mode: String = connection.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
            let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |r| r.get(0))?;
            let joint_commits: u64 = connection.query_row("SELECT COUNT(*) FROM task_session_commits", [], |r| r.get(0))?;
            let logical_batches: u64 = connection.query_row("SELECT COUNT(*) FROM session_batches", [], |r| r.get(0))?;
            let history_events: u64 = connection.query_row("SELECT COUNT(*) FROM session_events", [], |r| r.get(0))?;
            let mut statement = connection.prepare("SELECT receipt FROM task_session_commits ORDER BY json_extract(receipt, '$.task_sequence')")?;
            let receipts = statement.query_map([], |r| r.get::<_, String>(0))?;
            let mut joint_intents = Vec::new();
            let mut joint_mutations = Vec::new();
            for receipt in receipts {
                let receipt: swarmie_task_session_contracts::TaskSessionCommitReceipt = serde_json::from_str(&receipt?)
                    .map_err(|error| swarmie_sqlite_local::Error::Domain(super::io_error(error)))?;
                for record in &receipt.records {
                    joint_mutations.push(json!({"task_id":record.id,"revision":record.revision,
                        "phase":record.phase,"explicit_appends":receipt.appends.len(),"head":receipt.head}));
                }
                for (record, append) in receipt.records.iter().zip(&receipt.appends) {
                    let reference = if record.phase == "model" { "request_event" } else { "tool_event" };
                    joint_intents.push(json!({"task_id":record.id,"phase":record.phase,
                        "revision":record.revision,"checkpoint_reference":record.checkpoint[reference],
                        "batch_id":append.batch_id,"session_id":append.session_id,
                        "first_sequence":append.first_sequence,"last_sequence":append.last_sequence,
                        "append_commit_sequence":append.commit_sequence,"head":receipt.head}));
                }
            }
            Ok::<_, swarmie_sqlite_local::Error>(json!({
                "composition":"unified_same_owner", "database_files":1,
                "database_file":"runtime.sqlite", "journal_mode":journal_mode,
                "synchronous":synchronous, "history_commit":"joint", "scheduler_commit":"task_session",
                "joint_commit_records":joint_commits, "logical_session_batches":logical_batches,
                "history_events":history_events, "joint_explicit_intent_records":joint_intents.len(),
                "joint_intents":joint_intents, "joint_mutations":joint_mutations
            }))
        }).map_err(Into::into)
    }

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
    let (owner, database_handle) = swarmie_sqlite_local::facet(
        &swarmie_sqlite_local::Configuration {
            database: root.join("runtime.sqlite"),
        },
        vec![
            swarmie_task_storage_sqlite::sqlite_schema(),
            swarmie_session_store_sqlite::sqlite_schema(),
            swarmie_task_session_sqlite::sqlite_schema(),
            swarmie_task_session_integration::sqlite_schema(),
        ],
    )?;
    host.install(owner, core::LoadOptions::default())?.await?;
    let database = database_handle.database()?;
    let mut facets = swarmie_task_session_sqlite::facets_with_database(
        &database_handle,
        &swarmie_task_session_sqlite::Configuration {
            outbox: Default::default(),
        },
        driver.clone(),
    )?;
    facets.extend([
        match replies {
            Some(replies) => model::facet(replies, model.clone())?,
            None => model::routed_facet(scripts, model.clone())?,
        },
        tool::facet(Default::default(), tool.clone())?,
        policy::facet(true, policy.clone())?,
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                commit: swarmie_durable_tasks::CommitStrategy::TaskSession,
                poll_interval_ms: 2,
                max_parallel,
                ..Default::default()
            },
            driver.clone(),
        )?,
    ]);
    let readiness = facets
        .into_iter()
        .map(|facet| host.install(facet, core::LoadOptions::default()))
        .collect::<Result<Vec<_>>>()?;
    for ready in readiness {
        ready.await?;
    }
    let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
    let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
    let configuration = swarmie_task_session_integration::SharedConfiguration {
        task_store_id: storage.outbox_status().await?.store_id,
        session_store_id: sessions.publication_status().await?.durable.store_id,
        consumer_id: "agent-session-history-v1".into(),
        auto_publish: false,
        poll_interval_ms: 2,
    };
    host.install(
        swarmie_task_session_integration::facet_with_database(
            &database_handle,
            &configuration,
            driver.clone(),
        )?,
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
        database,
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
