//! Deterministic native turn workloads adapted from the retained October 26 measurements.
use futures::{FutureExt, executor::LocalPool, future::LocalBoxFuture};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};
use swarmie_agent_loop::{conversation as history, durable, *};
use swarmie_conversations as conversations;
use swarmie_durable_tasks as tasks;
use swarmie_providers::mock::{self, MockModel, Step};
use swarmie_storage::sqlite::SqliteOwner;
use tembra::{Context, Host, Orb, Reference, Service};

#[derive(Default)]
struct Counter {
    effects: Cell<u32>,
    with_receipts: bool,
    receipts: RefCell<BTreeMap<String, ToolResult>>,
}
impl Tools for Counter {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        _: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            if self.with_receipts {
                context.check()?;
                assert_eq!(context.principal()?, "alice");
            }
            assert_eq!(call.arguments, json!({"amount": 1}));
            Ok(ToolDecision::Allow)
        }
        .boxed_local()
    }
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            let result = ToolResult::Success(json!(1));
            if self.with_receipts {
                context.check()?;
                assert!(
                    self.receipts
                        .borrow_mut()
                        .insert(execution.operation.to_string(), result.clone())
                        .is_none(),
                    "duplicate tool dispatch"
                );
            }
            self.effects.set(self.effects.get() + 1);
            Ok(result)
        }
        .boxed_local()
    }
}

fn response(tool: bool) -> ModelResponse {
    ModelResponse {
        protection: swarmie_providers::SubmissionProtection::Unprotected,
        message: AssistantMessage {
            provider: None,
            content: if tool { String::new() } else { "Done".into() },
            tool_calls: if tool {
                vec![ToolCall {
                    id: ToolCallId::new("call-1").unwrap(),
                    name: ToolName::new("counter.add").unwrap(),
                    arguments: json!({"amount": 1}),
                }]
            } else {
                vec![]
            },
        },
        usage: Usage {
            input_tokens: 2,
            output_tokens: 3,
        },
    }
}

fn request(prior: usize, tool: bool) -> Request {
    Request {
        context: AgentContext {
            sources: None,
            instructions: "Return a deterministic answer.".into(),
            messages: (0..prior)
                .map(|_| Message::User {
                    content: "x".repeat(256),
                })
                .chain([Message::User {
                    content: "Add one".into(),
                }])
                .collect(),
        },
        tools: if tool {
            vec![ToolSpec {
                name: ToolName::new("counter.add").unwrap(),
                description: "Increment a counter".into(),
                input_schema: json!({"type": "object"}),
            }]
        } else {
            vec![]
        },
        max_turns: NonZeroU32::new(3).unwrap(),
    }
}

fn providers(
    host: &Host,
    model: Rc<MockModel>,
    counter: Rc<Counter>,
) -> impl Fn(tembra::Declaration) -> tembra::Result<Agent> + 'static {
    host.install(vec![
        mock::orb("measure.model", model),
        Orb::new("measure.tools", 1).service(
            Service::new("measure.tools", move |_| Ok(counter.clone()))
                .method(
                    ToolMethod::Validate.as_str(),
                    |env, context, (spec, call): (ToolSpec, ToolCall)| async move {
                        env.validate(&context, &spec, &call).await
                    },
                )
                .method(
                    ToolMethod::Execute.as_str(),
                    |env, context, (id, call): (OperationId, ToolCall)| async move {
                        env.execute(&context, ToolExecution::new(id, &call)).await
                    },
                )
                .method(
                    ToolMethod::Recover.as_str(),
                    |env, context, (id, call): (OperationId, ToolCall)| async move {
                        env.recover(&context, ToolExecution::new(id, &call)).await
                    },
                ),
        ),
    ])
    .unwrap();
    let host = host.clone();
    move |declaration| {
        Ok(Agent::new(
            Rc::new(ModelService::bind(
                &declaration,
                host.clone(),
                "measure.model",
                vec!["main".into()],
            )?),
            Rc::new(ToolService::bind(
                &declaration,
                host.clone(),
                "measure.tools",
                vec!["main".into()],
            )?),
        ))
    }
}

fn call<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: &str,
    args: A,
) -> R {
    let reference = reference.clone();
    let method = method.to_owned();
    pool.run_until(host.root(principal, move |context| {
        async move { reference.call(&context, &method, args).await }.boxed_local()
    }))
    .unwrap()
}

fn turn(mode: &str, prior: usize, tool: bool, root: &Path) -> u64 {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new(if tool {
        vec![Step::Reply(response(true)), Step::Reply(response(false))]
    } else {
        vec![Step::Reply(response(false))]
    }));
    let counter = Rc::new(Counter {
        with_receipts: mode == "conversation",
        ..Counter::default()
    });
    let request = request(prior, tool);
    let nanos = if mode == "direct" {
        let agent = Agent::new(model.clone(), counter.clone());
        let started = Instant::now();
        let output = pool
            .run_until(host.root("alice", move |context| {
                async move { Ok(agent.execute(&context, request).await) }.boxed_local()
            }))
            .unwrap()
            .unwrap();
        let elapsed = started.elapsed().as_nanos() as u64;
        assert_eq!(output.reason, FinishReason::Complete);
        elapsed
    } else if mode == "managed" {
        let factory = providers(&host, model.clone(), counter.clone());
        host.install(vec![
            Orb::new("measure.loop", 1).service(
                Service::new("measure.loop", factory)
                    .method("warm", |_, _, (): ()| async { Ok(()) })
                    .method("execute", |agent, context, request: Request| async move {
                        Ok(agent.execute(&context, request).await)
                    }),
            ),
        ])
        .unwrap();
        let reference = Rc::new(
            host.get("measure.loop", vec!["main".into()], "alice")
                .unwrap(),
        );
        call::<_, ()>(&mut pool, &host, &reference, "alice", "warm", ());
        let started = Instant::now();
        let output: ExecutionResult =
            call(&mut pool, &host, &reference, "alice", "execute", request);
        let elapsed = started.elapsed().as_nanos() as u64;
        assert_eq!(output.unwrap().reason, FinishReason::Complete);
        elapsed
    } else {
        let directory = tempfile::Builder::new()
            .prefix("turn-")
            .tempdir_in(root)
            .unwrap();
        let owner = SqliteOwner::new(directory.path().join("state.sqlite"));
        let factory = providers(&host, model.clone(), counter.clone());
        let (definition, loop_orb, history_reference) = if mode == "conversation" {
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
            let binding = host.clone();
            (
                history::durable::definition(),
                history::durable::orb(move |declaration| {
                    let history = history::History::bind(
                        &declaration,
                        binding.clone(),
                        conversations::SERVICE,
                        vec![conversations::INSTANCE_KEY.into()],
                    )?;
                    Ok((factory(declaration)?, history))
                }),
                Some(reference),
            )
        } else {
            assert_eq!(mode, "durable");
            (durable::definition(), durable::orb(factory), None)
        };
        host.install(vec![
            tasks::orb(
                host.clone(),
                owner.task_factory(),
                tasks::Definitions::new([definition.clone()]).unwrap(),
                tasks::Options::new(
                    pool.spawner(),
                    NonZeroU64::new(60_000).unwrap(),
                    Rc::new(|duration| {
                        async move { futures_timer::Delay::new(duration).await }.boxed_local()
                    }),
                ),
            ),
            loop_orb,
        ])
        .unwrap();
        let scheduler = Rc::new(
            host.get(
                tasks::SERVICE,
                vec![tasks::INSTANCE_KEY.into()],
                tasks::HOST_PRINCIPAL,
            )
            .unwrap(),
        );
        call::<_, ()>(
            &mut pool,
            &host,
            &scheduler,
            "host",
            tasks::Method::Resume.as_str(),
            (),
        );
        pool.run_until_stalled();
        let (input, history_id) = if let Some(reference) = &history_reference {
            let id = conversations::ConversationId::new();
            let created: conversations::WriteReceipt = call(
                &mut pool,
                &host,
                reference,
                "alice",
                conversations::Method::Create.as_str(),
                (id,),
            );
            let entries = request
                .context
                .messages
                .iter()
                .cloned()
                .map(|message| {
                    conversations::EntryDraft::encode(history::Record::Message(message)).unwrap()
                })
                .collect::<Vec<_>>();
            let appended: conversations::WriteReceipt = call(
                &mut pool,
                &host,
                reference,
                "alice",
                conversations::Method::Append.as_str(),
                (created.head, entries),
            );
            (
                serde_json::to_value(history::Request::new(
                    appended.head,
                    request.context.instructions,
                    request.tools,
                    request.max_turns,
                ))
                .unwrap(),
                Some(id),
            )
        } else {
            (serde_json::to_value(request).unwrap(), None)
        };
        let spec = tasks::TaskSpec {
            id: tasks::TaskId::new(),
            definition,
            input,
        };
        let id = spec.id;
        let started = Instant::now();
        let created: tasks::TaskId = call(
            &mut pool,
            &host,
            &scheduler,
            "alice",
            tasks::Method::Create.as_str(),
            (spec,),
        );
        pool.run_until_stalled();
        let elapsed = started.elapsed().as_nanos() as u64;
        assert_eq!(created, id);
        let record: Option<tasks::TaskRecord> = call(
            &mut pool,
            &host,
            &scheduler,
            "alice",
            tasks::Method::Get.as_str(),
            (id,),
        );
        let record = record.unwrap();
        assert!(record.assignment.is_none());
        let tasks::TaskStatus::Completed(value) = record.status else {
            panic!("turn did not complete")
        };
        let output: Output = serde_json::from_value(value).unwrap();
        assert_eq!(output.reason, FinishReason::Complete);
        assert_eq!(output.messages.len(), if tool { 3 } else { 1 });
        if let Some(reference) = history_reference {
            let conversation: Option<conversations::Conversation> = call(
                &mut pool,
                &host,
                &reference,
                "alice",
                conversations::Method::Get.as_str(),
                (history_id.unwrap(),),
            );
            let mut read = conversations::Read::new(conversation.unwrap().head);
            let mut messages = 0;
            loop {
                let page: conversations::Page = call(
                    &mut pool,
                    &host,
                    &reference,
                    "alice",
                    conversations::Method::Read.as_str(),
                    read.clone(),
                );
                messages += page
                    .entries
                    .iter()
                    .filter(|entry| {
                        matches!(
                            serde_json::from_value::<history::Record>(entry.data.clone()).unwrap(),
                            history::Record::Message(_)
                        )
                    })
                    .count();
                if let Some(next) = page.next {
                    read.after = Some(next);
                } else {
                    break;
                }
            }
            assert_eq!(messages, prior + 1 + if tool { 3 } else { 1 });
        }
        // Close before deleting the database, including on Windows.
        pool.run_until(host.close()).unwrap();
        elapsed
    };
    assert_eq!(counter.effects.get(), u32::from(tool));
    assert_eq!(model.remaining(), 0);
    let calls = model.calls();
    assert_eq!(calls.len(), if tool { 2 } else { 1 });
    assert_eq!(calls[0].request.context.messages.len(), prior + 1);
    assert!(calls.iter().all(|call| call.principal == "alice"));
    if matches!(mode, "direct" | "managed") {
        pool.run_until(host.close()).unwrap();
    }
    let diagnostics = host.kernel().diagnostics();
    assert_eq!(diagnostics.calls, 0);
    assert_eq!(diagnostics.resources, 0);
    assert_eq!(diagnostics.roots, 0);
    nanos
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("expected operation, total messages, fixture directory and timeout".into());
    }
    let parts = args[0].split('.').collect::<Vec<_>>();
    if parts.len() != 4
        || parts[0] != "agent"
        || !["direct", "managed", "durable", "conversation"].contains(&parts[1])
        || !["answer", "tool"].contains(&parts[2])
    {
        return Err("unsupported turn workload".into());
    }
    let messages: usize = args[1].parse()?;
    let repetitions: usize = parts[3].parse()?;
    let timeout: u64 = args[3].parse()?;
    if ![1, 101, 1001].contains(&messages)
        || ![20, 60].contains(&repetitions)
        || !(100..=600_000).contains(&timeout)
    {
        return Err("turn size or deadline is outside the supported range".into());
    }
    let root = Path::new(&args[2]);
    let tool = parts[2] == "tool";
    let started = Instant::now();
    let mut measured = Vec::with_capacity(repetitions);
    for index in 0..repetitions + 5 {
        if started.elapsed() > Duration::from_millis(timeout) {
            return Err("turn worker exceeded its cooperative deadline".into());
        }
        let nanos = turn(parts[1], messages - 1, tool, root);
        if index >= 5 {
            measured.push(nanos);
        }
    }
    let mut ordered = measured.clone();
    ordered.sort_unstable();
    let median = (ordered[ordered.len() / 2 - 1] as f64 + ordered[ordered.len() / 2] as f64)
        / 2.0
        / 1_000_000.0;
    let p95 = ordered[(ordered.len() * 95).div_ceil(100) - 1] as f64 / 1_000_000.0;
    println!(
        "{}",
        json!({"elapsed_ms": median, "turn_p95_ms": p95, "timed_turns": repetitions, "model_calls_per_turn": if tool { 2 } else { 1 }, "tool_effects_per_turn": u32::from(tool), "verified_items": {"turn_samples_ns": measured, "prior_messages": messages - 1, "message_bytes": 256, "warmups": 5, "completed": repetitions, "no_live_calls_or_resources": true }})
    );
    Ok(())
}
