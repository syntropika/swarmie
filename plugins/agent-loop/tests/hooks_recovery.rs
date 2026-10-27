#![cfg(feature = "durable-tasks")]

#[path = "support/request.rs"]
mod input;
#[path = "support/scenario.rs"]
mod scenario;

use futures::{FutureExt, executor::LocalPool};
use serde_json::json;
use std::{cell::Cell, rc::Rc};
use swarmie_agent_loop::{hooks::*, *};
use swarmie_models::mock::{MockModel, Step};
use tembra::Host;
#[cfg(feature = "conversations")]
use tembra::{Orb, Service};

#[path = "support/hook_scenario.rs"]
mod hook_scenario;
use hook_scenario::*;

#[cfg(feature = "durable-tasks")]
#[test]
fn durable_restart_reconciles_the_prepared_call_after_result_hook_failure() {
    use std::num::NonZeroU64;
    use swarmie_durable_tasks::{self as tasks, TaskRecord, TaskStatus};
    use swarmie_storage_sqlite::SqliteOwner;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite");
    let mut pool = LocalPool::new();
    let model = Rc::new(MockModel::new([
        Step::Reply(response(true)),
        Step::Reply(response(false)),
    ]));
    let tools = Rc::new(scenario::Scenario::default());
    let prepared = Rc::new(Cell::new(0));
    let mut id = tasks::TaskId::new();
    let mut operation = None;
    for restart in [false, true] {
        let host = Host::new(pool.spawner());
        let owner = SqliteOwner::new(&path);
        let m = model.clone();
        let t = tools.clone();
        let counter = prepared.clone();
        let hooks = Hooks::default();
        host.install(vec![
            tasks::orb(
                host.clone(),
                owner.task_factory(),
                tasks::Definitions::new([durable::definition()]).unwrap(),
                tasks::Options::new(
                    pool.spawner(),
                    NonZeroU64::new(60_000).unwrap(),
                    Rc::new(|duration| {
                        async move { futures_timer::Delay::new(duration).await }.boxed_local()
                    }),
                ),
            ),
            durable::orb(move |_| Ok(Agent::new(m.clone(), t.clone()))),
            contributor(durable::SERVICE)
                .contribute(
                    durable::SERVICE,
                    &hooks.before_tool_call,
                    0,
                    move |_, mut v| {
                        counter.set(counter.get() + 1);
                        v.call.arguments = json!({"amount":1});
                        async { Ok(v) }
                    },
                )
                .contribute(
                    durable::SERVICE,
                    &hooks.after_tool_call,
                    0,
                    move |_, v| async move {
                        if restart {
                            Ok(v)
                        } else {
                            Err(tembra::Error::invalid(
                                "fixture.result-transform",
                                "failed after effect",
                            ))
                        }
                    },
                ),
        ])
        .unwrap();
        let reference = Rc::new(
            host.get(tasks::SERVICE, vec![tasks::INSTANCE_KEY.into()], "host")
                .unwrap(),
        );
        if !restart {
            id = invoke(
                &mut pool,
                &host,
                &reference,
                tasks::Method::Create.as_str(),
                (tasks::TaskSpec {
                    id,
                    definition: durable::definition(),
                    input: serde_json::to_value(input::request()).unwrap(),
                },),
            );
        }
        let scheduler = reference.clone();
        pool.run_until(host.root("host", move |ctx| {
            async move {
                scheduler
                    .call::<_, ()>(&ctx, tasks::Method::Resume.as_str(), ())
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
        pool.run_until_stalled();
        let record: Option<TaskRecord> = invoke(
            &mut pool,
            &host,
            &reference,
            tasks::Method::Get.as_str(),
            (id,),
        );
        let record = record.unwrap();
        if restart {
            assert!(matches!(record.status, TaskStatus::Completed(_)));
            assert_eq!(
                tools.reconciled_tools.borrow().as_slice(),
                &[operation.unwrap()]
            );
            assert!(record.assignment.is_none());
        } else {
            assert!(!matches!(
                record.status,
                TaskStatus::Failed(_) | TaskStatus::Completed(_)
            ));
            let state: State = serde_json::from_value(record.checkpoint.unwrap()).unwrap();
            let Phase::ToolPending {
                operation: pending,
                call,
                ..
            } = state.phase()
            else {
                panic!("lost pending tool")
            };
            assert_eq!(call.arguments, json!({"amount":1}));
            operation = Some(*pending);
        }
        close(&mut pool, &host);
    }
    assert_eq!(prepared.get(), 1);
    assert_eq!(tools.effects.borrow().len(), 1);
    assert_eq!(model.calls().len(), 2);
    assert_eq!(tools.tool_inputs.borrow()[0].arguments, json!({"amount":1}));
    assert_eq!(
        *tools.tool_inputs.borrow(),
        *tools.recovered_inputs.borrow()
    );
}

#[cfg(feature = "conversations")]
#[test]
fn history_restart_restores_prepared_tool_evidence_without_rewriting_assistant_arguments() {
    use swarmie_agent_loop::conversation::{self as history, History, Record};
    use swarmie_conversations::{self as conversations, EntryDraft, Read, WriteReceipt};
    use swarmie_storage_sqlite::SqliteOwner;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let model = Rc::new(MockModel::new([
        Step::Reply(response(true)),
        Step::Reply(response(false)),
    ]));
    let tools = Rc::new(scenario::Scenario::default());
    let prepared = Rc::new(Cell::new(0));
    let mut checkpoint = None;
    let mut head = None;
    for restart in [false, true] {
        let host = Host::new(pool.spawner());
        let owner = SqliteOwner::new(&path);
        host.install(vec![conversations::orb(owner.conversation_factory())])
            .unwrap();
        let reference = Rc::new(
            host.get(
                conversations::SERVICE,
                vec![conversations::INSTANCE_KEY.into()],
                "host",
            )
            .unwrap(),
        );
        if !restart {
            let created: WriteReceipt = invoke(
                &mut pool,
                &host,
                &reference,
                conversations::Method::Create.as_str(),
                (conversations::ConversationId::new(),),
            );
            let written: WriteReceipt = invoke(
                &mut pool,
                &host,
                &reference,
                conversations::Method::Append.as_str(),
                (
                    created.head,
                    vec![
                        EntryDraft::encode(Record::Message(
                            input::request().context.messages[0].clone(),
                        ))
                        .unwrap(),
                    ],
                ),
            );
            head = Some(written.head);
        }
        let bound_host = host.clone();
        let m = model.clone();
        let t = tools.clone();
        let hooks = Hooks::default();
        let counter = prepared.clone();
        let native = Hooks::default().declare(Orb::new(SERVICE, 1)).service(
            Service::new(SERVICE, move |declaration| {
                Ok((
                    Agent::new(m.clone(), t.clone()).bind_hooks(&declaration)?,
                    History::bind(
                        &declaration,
                        bound_host.clone(),
                        conversations::SERVICE,
                        vec![conversations::INSTANCE_KEY.into()],
                    )?,
                ))
            })
            .method(
                Method::Execute.as_str(),
                |env, ctx, request: history::Request| async move {
                    Ok(env.1.execute(&ctx, &env.0, request).await)
                },
            )
            .method(
                Method::Resume.as_str(),
                |env, ctx, checkpoint: history::Checkpoint| async move {
                    Ok(env.1.resume(&ctx, &env.0, checkpoint).await)
                },
            ),
        );
        host.install(vec![
            native,
            contributor(SERVICE)
                .contribute(SERVICE, &hooks.before_tool_call, 0, move |_, mut v| {
                    counter.set(counter.get() + 1);
                    v.call.arguments = json!({"amount":1});
                    async { Ok(v) }
                })
                .contribute(SERVICE, &hooks.after_tool_call, 0, move |_, v| async move {
                    if restart {
                        Ok(v)
                    } else {
                        Err(tembra::Error::invalid("fixture.result-transform", "failed"))
                    }
                }),
        ])
        .unwrap();
        let agent = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
        if restart {
            let saved = serde_json::to_value(checkpoint.as_ref().unwrap()).unwrap();
            for replacement in [
                serde_json::Value::Null,
                json!(conversations::EntryId::new()),
                saved["phase"]["assistant"].clone(),
            ] {
                let mut forged = saved.clone();
                forged["phase"]["prepared"] = replacement;
                let forged: history::Checkpoint = serde_json::from_value(forged).unwrap();
                let rejected: history::ExecutionResult =
                    invoke(&mut pool, &host, &agent, Method::Resume.as_str(), forged);
                assert_eq!(rejected.unwrap_err().error.code, tembra::Code::Invalid);
                assert!(tools.reconciled_tools.borrow().is_empty());
            }
            let result: history::ExecutionResult = invoke(
                &mut pool,
                &host,
                &agent,
                Method::Resume.as_str(),
                checkpoint.take().unwrap(),
            );
            let result = result.unwrap();
            let page: conversations::Page = invoke(
                &mut pool,
                &host,
                &reference,
                conversations::Method::Read.as_str(),
                Read::new(result.checkpoint.head()),
            );
            let records = page
                .entries
                .iter()
                .map(|entry| entry.decode::<Record>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(records.iter().filter(|r| matches!(r,Record::PreparedTool { call, .. } if call.arguments==json!({"amount":1}))).count(),1);
            assert!(records.iter().any(|r| matches!(r,Record::Message(Message::Assistant(v)) if v.tool_calls.first().is_some_and(|c| c.arguments==json!({"amount":99})))));
            assert_eq!(result.output.reason, FinishReason::Complete);
        } else {
            let base = input::request();
            let result: history::ExecutionResult = invoke(
                &mut pool,
                &host,
                &agent,
                Method::Execute.as_str(),
                history::Request {
                    at: head.unwrap(),
                    instructions: base.context.instructions,
                    tools: base.tools,
                    max_turns: base.max_turns,
                },
            );
            checkpoint = Some(*result.unwrap_err().checkpoint.unwrap());
        }
        close(&mut pool, &host);
    }
    assert_eq!(prepared.get(), 1);
    assert_eq!(tools.effects.borrow().len(), 1);
    assert_eq!(tools.reconciled_tools.borrow().len(), 1);
    assert_eq!(tools.tool_inputs.borrow()[0].arguments, json!({"amount":1}));
    assert_eq!(
        *tools.tool_inputs.borrow(),
        *tools.recovered_inputs.borrow()
    );
}
