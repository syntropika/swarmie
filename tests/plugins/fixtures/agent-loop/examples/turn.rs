//! Executable examples with scripted model replies and real Tembra/SQLite execution.

use std::time::Duration;

use serde_json::json;
use swarmie_agent_contracts::{AgentLoop, AgentLoopInput, StartTurn, TurnLimits};
use swarmie_agent_loop_fixture::{
    model::{self, ModelProbe, Reply},
    policy::{self, PolicyProbe},
    tool::{self, ToolProbe},
};
use swarmie_model_contracts::ModelIdentity;
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, MESSAGE, NewEvent, SessionStore, SessionStoreInput,
};
use swarmie_task_contracts::{DurableTasks, DurableTasksInput, TaskLookup};
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

fn main() -> Result<()> {
    let with_tool = std::env::args()
        .nth(1)
        .is_some_and(|argument| argument == "tool");
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("example runtime")
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(20),async {
                let directory=tempfile::tempdir().expect("example storage");
                let host=Host::with_driver("agent-example","application",driver.clone())?;
                let replies=if with_tool {vec![Reply::double(21),Reply::text("The answer is 42.")]} else {vec![Reply::text("Hello from the extensible agent loop.")]};
                for facet in [
                    swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {database:directory.path().join("tasks.sqlite")})?,
                    swarmie_session_store_sqlite::facet(&swarmie_session_store_sqlite::Configuration {database:directory.path().join("sessions.sqlite")})?,
                    model::facet(replies,ModelProbe::default())?,
                    tool::facet(Default::default(),ToolProbe::default())?,
                    policy::facet(true,PolicyProbe::default())?,
                    swarmie_durable_tasks::facet(&swarmie_durable_tasks::Configuration {poll_interval_ms:2,..Default::default()},driver.clone())?,
                    swarmie_agent_loop_default::facet(driver.clone())?,
                ] {host.install(facet,core::LoadOptions::default())?.await?;}
                let sessions=host.get(SessionStore,SessionStoreInput {}).await?;
                sessions.create(CreateSession {session_id:"demo".into(),metadata:json!({})}).await?;
                sessions.append(AppendEvents {session_id:"demo".into(),batch_id:"input".into(),expected_head:None,events:vec![NewEvent {
                    event_id:"input".into(),kind:MESSAGE.into(),version:1,correlation:Correlation::default(),occurred_at_ms:None,
                    payload:json!({"role":"user","content":"Compute 21 doubled.","reply_to":null,"tool_call_id":null}),
                }]}).await?;
                let agent=host.get(AgentLoop,AgentLoopInput {}).await?;
                let turn=agent.start(StartTurn {turn_id:"demo-turn".into(),session_id:"demo".into(),input_event_id:"input".into(),model:ModelIdentity {connection:"fixture".into(),model:"scripted".into()},limits:TurnLimits::default()}).await?;
                let tasks=host.get(DurableTasks,DurableTasksInput {}).await?;
                let outcome=tasks.wait(TaskLookup {id:turn.task.id}).await?;
                println!("{}",serde_json::to_string_pretty(&outcome).expect("serializable outcome"));
                let page=sessions.events(swarmie_session_contracts::EventQuery::new("demo")).await?;
                for entry in page.events.into_iter().filter(|entry| entry.event.kind==MESSAGE) {
                    println!("{}",entry.event.payload);
                }
                host.close().await
            }).await.map_err(|_| RuntimeError::unavailable("example_deadline","example exceeded its deadline"))?
        }))
}
