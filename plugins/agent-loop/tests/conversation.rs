#![cfg(feature = "conversations")]

mod support;

use futures::{FutureExt, executor::LocalPool};
use serde_json::json;
use std::rc::Rc;
use swarmie_agent_loop::{
    conversation::{self as history, History, Record},
    *,
};
use swarmie_conversations::{
    self as conversations, ConversationId, EntryDraft, Head, HistoryPoint, Read, WriteReceipt,
};
use swarmie_providers::mock::{MockModel, Step};
use swarmie_storage::sqlite::SqliteOwner;
use tembra::{Host, Reference};

fn open(pool: &LocalPool, owner: &SqliteOwner) -> (Host, Rc<Reference>, History) {
    let host = Host::new(pool.spawner());
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
    let history = History::new(reference.clone());
    (host, reference, history)
}

fn invoke<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    method: conversations::Method,
    args: A,
) -> R {
    let reference = reference.clone();
    pool.run_until(host.root("alice", move |ctx| {
        async move { reference.call(&ctx, method.as_str(), args).await }.boxed_local()
    }))
    .unwrap()
}

fn seed(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    messages: Vec<Message>,
) -> Head {
    let created: WriteReceipt = invoke(
        pool,
        host,
        reference,
        conversations::Method::Create,
        (ConversationId::new(),),
    );
    let entries: Vec<_> = messages
        .into_iter()
        .map(|message| EntryDraft::encode(Record::Message(message)).unwrap())
        .collect();
    if entries.is_empty() {
        created.head
    } else {
        let appended: WriteReceipt = invoke(
            pool,
            host,
            reference,
            conversations::Method::Append,
            (created.head, entries),
        );
        appended.head
    }
}

fn request(at: Head) -> history::Request {
    let base = support::request();
    history::Request {
        at,
        instructions: base.context.instructions,
        tools: base.tools,
        max_turns: base.max_turns,
    }
}

fn execute(
    pool: &mut LocalPool,
    host: &Host,
    history: History,
    agent: Agent,
    request: history::Request,
) -> history::ExecutionResult {
    pool.run_until(host.root("alice", move |ctx| {
        async move { Ok(history.execute(&ctx, &agent, request).await) }.boxed_local()
    }))
    .unwrap()
}

fn answer() -> ModelResponse {
    ModelResponse {
        message: AssistantMessage {
            content: "Done".into(),
            tool_calls: vec![],
        },
        usage: Default::default(),
    }
}

#[test]
fn ordinary_history_uses_the_same_engine_and_keeps_prepared_requests_out_of_context() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let scenario = Rc::new(support::Scenario::default());
    let create = support::factory(&host, scenario.clone());
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let configured_history = history.clone();
    host.install(vec![
        tembra::Orb::new("fixture.history-agent", 1).service(
            tembra::Service::new("fixture.history-agent", move |declaration| {
                Ok((create(declaration)?, configured_history.clone()))
            })
            .method(
                "execute",
                |env, context, input: history::Request| async move {
                    Ok(env.1.execute(&context, &env.0, input).await)
                },
            ),
        ),
    ])
    .unwrap();
    let managed = host
        .get("fixture.history-agent", vec!["main".into()], "alice")
        .unwrap();
    let result: history::ExecutionResult = pool
        .run_until(host.root("alice", move |ctx| {
            async move { managed.call(&ctx, "execute", request(at)).await }.boxed_local()
        }))
        .unwrap();
    let result = result.unwrap();
    assert_eq!(result.output.messages.len(), 3);
    assert_eq!(scenario.effects.borrow().len(), 1);
    let page: conversations::Page = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Read,
        Read::new(result.checkpoint.head()),
    );
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::PreparedRequest { .. }
            ))
            .count(),
        2
    );
    let head = result.checkpoint.head();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                history
                    .context(&ctx, head, "current instructions".into())
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(context.messages.len(), 4);
    assert_eq!(context.instructions, "current instructions");
    let saved = serde_json::to_value(result.checkpoint).unwrap();
    assert!(saved.get("messages").is_none());
    assert!(saved["input"].get("context").is_none());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn uncertain_tool_reply_resumes_from_references_without_repeating_the_effect() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.uncertain_tool.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failed.checkpoint.unwrap();
    assert_eq!(
        serde_json::to_value(&checkpoint).unwrap()["phase"]["kind"],
        "tool_pending"
    );
    let agent = support::agent(&host, scenario.clone());
    let resumed = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(history.resume(&ctx, &agent, checkpoint).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(resumed.output.reason, FinishReason::Complete);
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(
        scenario.reconciled_tools.borrow().as_slice(),
        scenario.effects.borrow().as_slice()
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn recovery_uses_the_exact_prepared_projection_without_rewriting_canonical_messages() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "canonical".into(),
        }],
    );
    let operation: OperationId =
        serde_json::from_value(json!("11111111-1111-4111-8111-111111111111")).unwrap();
    let prepared = ModelRequest {
        context: AgentContext {
            instructions: "prepared instructions".into(),
            messages: vec![Message::User {
                content: "request-only projection".into(),
            }],
        },
        tools: request(at).tools,
    };
    let entry = EntryDraft::encode(Record::PreparedRequest {
        operation,
        request: prepared.clone(),
    })
    .unwrap();
    let entry_id = entry.id;
    let appended: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (at, vec![entry]),
    );
    let checkpoint:history::Checkpoint=serde_json::from_value(json!({"input":request(at),"head":appended.head,"turn":1,"usage":Usage::default(),"available_tools":prepared.tools.iter().map(|tool| &tool.name).collect::<Vec<_>>(),"phase":{"kind":"model_pending","operation":operation,"request":entry_id}})).unwrap();
    let model = Rc::new(MockModel::new([Step::Reply(answer())]));
    let agent = Agent::new(model.clone(), Rc::new(support::Scenario::default()));
    let read_history = history.clone();
    let completion = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(history.resume(&ctx, &agent, checkpoint).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(model.calls()[0].request, prepared);
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                read_history
                    .context(
                        &ctx,
                        completion.checkpoint.head(),
                        "canonical instructions".into(),
                    )
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(
        context.messages[0],
        Message::User {
            content: "canonical".into()
        }
    );
    assert_eq!(context.messages.len(), 2);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn large_history_does_not_expand_checkpoints_or_task_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        (0..1000)
            .map(|_| Message::User {
                content: "x".repeat(256),
            })
            .collect(),
    );
    let input = request(at);
    assert!(serde_json::to_vec(&input).unwrap().len() < 1500);
    let model = Rc::new(MockModel::new([Step::Reply(answer())]));
    let completed = execute(
        &mut pool,
        &host,
        history,
        Agent::new(model.clone(), Rc::new(support::Scenario::default())),
        input,
    )
    .unwrap();
    assert_eq!(model.calls()[0].request.context.messages.len(), 1000);
    assert!(serde_json::to_vec(&completed.checkpoint).unwrap().len() < 1500);
    assert_eq!(completed.output.messages.len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn branch_projection_ignores_later_parent_entries_and_unknown_records_fail_before_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "shared".into(),
        }],
    );
    let forked: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Fork,
        (
            ConversationId::new(),
            HistoryPoint {
                conversation: at.conversation,
                entry: at.entry.unwrap(),
            },
        ),
    );
    let _: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (
            at,
            vec![
                EntryDraft::encode(Record::Message(Message::User {
                    content: "parent only".into(),
                }))
                .unwrap(),
            ],
        ),
    );
    let model = Rc::new(MockModel::new([Step::Reply(answer())]));
    execute(
        &mut pool,
        &host,
        history.clone(),
        Agent::new(model.clone(), Rc::new(support::Scenario::default())),
        request(forked.head),
    )
    .unwrap();
    assert_eq!(model.calls()[0].request.context.messages.len(), 1);
    let fresh = seed(&mut pool, &host, &reference, vec![]);
    let bad: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (
            fresh,
            vec![EntryDraft::encode(json!({"unrecognised":true})).unwrap()],
        ),
    );
    let untouched = Rc::new(MockModel::new([Step::Reply(answer())]));
    let failure = execute(
        &mut pool,
        &host,
        history,
        Agent::new(untouched.clone(), Rc::new(support::Scenario::default())),
        request(bad.head),
    )
    .unwrap_err();
    assert!(failure.checkpoint.is_none());
    assert!(untouched.calls().is_empty());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn conflicting_append_keeps_the_confirmed_intent_and_never_overwrites_other_input() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.uncertain_tool.set(true);
    let failure = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failure.checkpoint.unwrap();
    let appended: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (
            checkpoint.head(),
            vec![
                EntryDraft::encode(Record::Message(Message::User {
                    content: "competing input".into(),
                }))
                .unwrap(),
            ],
        ),
    );
    for _ in 0..2 {
        let history = history.clone();
        let checkpoint = checkpoint.clone();
        let agent = support::agent(&host, scenario.clone());
        let failed = pool
            .run_until(host.root("alice", move |ctx| {
                async move { Ok(history.resume(&ctx, &agent, checkpoint).await) }.boxed_local()
            }))
            .unwrap()
            .unwrap_err();
        assert_eq!(failed.error.code, tembra::Code::Conflict);
        assert_eq!(
            serde_json::to_value(failed.checkpoint.unwrap()).unwrap()["phase"]["kind"],
            "tool_pending"
        );
    }
    assert_eq!(scenario.effects.borrow().len(), 1);
    let current: Option<conversations::Conversation> = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Get,
        (at.conversation,),
    );
    assert_eq!(current.unwrap().head, appended.head);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn invalid_checkpoint_references_are_rejected_before_any_provider_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Hello".into(),
        }],
    );
    let model = Rc::new(MockModel::new([Step::LoseReply(answer())]));
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        Agent::new(model, Rc::new(support::Scenario::default())),
        request(at),
    )
    .unwrap_err();
    let original = serde_json::to_value(failed.checkpoint.unwrap()).unwrap();
    for variant in 0..3 {
        let mut value = original.clone();
        match variant {
            0 => {
                value["phase"]["request"] =
                    serde_json::to_value(conversations::EntryId::new()).unwrap()
            }
            1 => {
                value["head"]["conversation"] = serde_json::to_value(ConversationId::new()).unwrap()
            }
            _ => value["turn"] = json!(u32::MAX),
        }
        let checkpoint: history::Checkpoint = serde_json::from_value(value).unwrap();
        let provider = Rc::new(MockModel::new([Step::Reply(answer())]));
        let agent = Agent::new(provider.clone(), Rc::new(support::Scenario::default()));
        let history = history.clone();
        let failed = pool
            .run_until(host.root("alice", move |ctx| {
                async move { Ok(history.resume(&ctx, &agent, checkpoint).await) }.boxed_local()
            }))
            .unwrap()
            .unwrap_err();
        assert!(failed.checkpoint.is_none());
        assert!(provider.calls().is_empty());
        assert!(provider.recoveries().is_empty());
    }
    pool.run_until(host.close()).unwrap();
}

#[test]
fn prepared_request_cannot_widen_the_original_tool_definitions() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let request = support::request();
    let mut pending = ModelRequest {
        context: request.context.clone(),
        tools: request.tools.clone(),
    };
    pending.tools[0].name = ToolName::new("unavailable.tool").unwrap();
    let mut value = serde_json::to_value(State::start(request).unwrap()).unwrap();
    value["turn"] = json!(1);
    value["phase"] = json!({"kind":"model_pending","operation":"11111111-1111-4111-8111-111111111111","request":pending});
    let state: State = serde_json::from_value(value).unwrap();
    let provider = Rc::new(MockModel::new([Step::Reply(answer())]));
    let agent = Agent::new(provider.clone(), Rc::new(support::Scenario::default()));
    let failure = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(agent.resume(&ctx, state).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap_err();
    assert!(failure.state.is_none());
    assert!(provider.calls().is_empty());
    assert!(provider.recoveries().is_empty());
    pool.run_until(host.close()).unwrap();
}

fn settle(
    pool: &mut LocalPool,
    host: &Host,
    history: History,
    tools: Rc<support::Scenario>,
    checkpoint: history::Checkpoint,
    acknowledge_unknown: bool,
) -> tembra::Result<history::ToolSettlement> {
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            history
                .settle_tools(&ctx, tools.as_ref(), checkpoint, acknowledge_unknown, None)
                .await
        }
        .boxed_local()
    }))
}

#[test]
fn settlement_recovers_a_receipt_and_blocks_remaining_calls_without_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add three".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.tool_count.set(3);
    scenario.uncertain_tool.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failed.checkpoint.unwrap();
    assert!(checkpoint.has_unfinished_tools());
    assert!(matches!(
        checkpoint.pending(),
        Some(history::Pending::Tool { next: 0, .. })
    ));
    let settled = settle(
        &mut pool,
        &host,
        history.clone(),
        scenario.clone(),
        checkpoint,
        false,
    )
    .unwrap();
    let history::ToolSettlement::Settled(checkpoint) = settled else {
        panic!("confirmed receipt remained unknown")
    };
    assert!(!checkpoint.has_unfinished_tools());
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.models.borrow().len(), 1);
    assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
    let head = checkpoint.head();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move { history.context(&ctx, head, String::new()).await }.boxed_local()
        }))
        .unwrap();
    assert!(matches!(
        &context.messages[2],
        Message::Tool {
            result: ToolResult::Success(_),
            ..
        }
    ));
    assert!(context.messages[3..].iter().all(|message| matches!(message, Message::Tool { result: ToolResult::Blocked(reason), .. } if reason == "not executed")));
    assert_eq!(context.messages.len(), 5);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn settlement_requires_acknowledgment_for_unknown_effects_and_allows_a_fresh_turn() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.uncertain_tool.set(true);
    scenario.unknown_tool.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failed.checkpoint.unwrap();
    let original_head = checkpoint.head();
    let unknown = settle(
        &mut pool,
        &host,
        history.clone(),
        scenario.clone(),
        checkpoint,
        false,
    )
    .unwrap();
    let history::ToolSettlement::Unknown(checkpoint) = unknown else {
        panic!("unknown effect was accepted without acknowledgment")
    };
    assert_eq!(checkpoint.head(), original_head);
    let settled = settle(
        &mut pool,
        &host,
        history.clone(),
        scenario.clone(),
        checkpoint,
        true,
    )
    .unwrap();
    let history::ToolSettlement::Settled(checkpoint) = settled else {
        panic!("acknowledged effect was not closed")
    };
    let head = checkpoint.head();
    let read_history = history.clone();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move { read_history.context(&ctx, head, String::new()).await }.boxed_local()
        }))
        .unwrap();
    assert!(
        matches!(&context.messages[2], Message::Tool { result: ToolResult::Failed(reason), .. } if reason == "outcome unknown; not replayed")
    );
    let new_user: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (
            head,
            vec![
                EntryDraft::encode(Record::Message(Message::User {
                    content: "Explain the result".into(),
                }))
                .unwrap(),
            ],
        ),
    );
    let next = execute(
        &mut pool,
        &host,
        history,
        support::agent(&host, scenario.clone()),
        request(new_user.head),
    )
    .unwrap();
    assert_eq!(next.output.reason, FinishReason::Complete);
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.models.borrow().len(), 2);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn settlement_never_uses_safe_retry_as_permission_to_execute() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.not_started.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    scenario.safe_retry.set(true);
    let settled = settle(
        &mut pool,
        &host,
        history.clone(),
        scenario.clone(),
        *failed.checkpoint.unwrap(),
        false,
    )
    .unwrap();
    let history::ToolSettlement::Settled(checkpoint) = settled else {
        panic!("safe-to-retry operation was not closed")
    };
    let head = checkpoint.head();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move { history.context(&ctx, head, String::new()).await }.boxed_local()
        }))
        .unwrap();
    assert!(
        matches!(&context.messages[2], Message::Tool { result: ToolResult::Blocked(reason), .. } if reason == "not retried after interruption")
    );
    assert!(scenario.effects.borrow().is_empty());
    assert_eq!(scenario.tool_inputs.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn settlement_refuses_a_changed_head_before_provider_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.uncertain_tool.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failed.checkpoint.unwrap();
    let _: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (
            checkpoint.head(),
            vec![
                EntryDraft::encode(Record::Message(Message::Tool {
                    call: ToolCallId::new("call-1").unwrap(),
                    result: ToolResult::Success(json!(1)),
                }))
                .unwrap(),
            ],
        ),
    );
    let error = match settle(
        &mut pool,
        &host,
        history,
        scenario.clone(),
        checkpoint,
        true,
    ) {
        Err(error) => error,
        Ok(_) => panic!("changed history was accepted"),
    };
    assert_eq!(error.code, tembra::Code::Conflict);
    assert!(scenario.reconciled_tools.borrow().is_empty());
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn unavailable_recovery_requires_explicit_acknowledgment_and_never_dispatches() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.uncertain_tool.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failed.checkpoint.unwrap();
    scenario.unavailable_recovery.set(true);
    assert!(
        settle(
            &mut pool,
            &host,
            history.clone(),
            scenario.clone(),
            checkpoint.clone(),
            false
        )
        .is_err()
    );
    let current: Option<conversations::Conversation> = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Get,
        (at.conversation,),
    );
    assert_eq!(current.unwrap().head, checkpoint.head());
    let acknowledged = settle(
        &mut pool,
        &host,
        history.clone(),
        scenario.clone(),
        checkpoint,
        true,
    )
    .unwrap();
    let history::ToolSettlement::Settled(checkpoint) = acknowledged else {
        panic!("unavailable recovery blocked acknowledgment")
    };
    let head = checkpoint.head();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move { history.context(&ctx, head, String::new()).await }.boxed_local()
        }))
        .unwrap();
    assert!(
        matches!(&context.messages[2], Message::Tool { result: ToolResult::Failed(reason), .. } if reason == "outcome unknown; not replayed")
    );
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.models.borrow().len(), 1);
    assert_eq!(scenario.tool_inputs.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn settlement_blocks_prepared_calls_without_recovery_and_honors_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let call = ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        name: ToolName::new("counter.add").unwrap(),
        arguments: json!({"amount":1}),
    };
    let assistant = EntryDraft::encode(Record::Message(Message::Assistant(AssistantMessage {
        content: String::new(),
        tool_calls: vec![call],
    })))
    .unwrap();
    let assistant_id = assistant.id;
    let appended: WriteReceipt = invoke(
        &mut pool,
        &host,
        &reference,
        conversations::Method::Append,
        (at, vec![assistant]),
    );
    let checkpoint: history::Checkpoint = serde_json::from_value(json!({
        "input":request(at), "head":appended.head, "turn":1, "usage":Usage::default(),
        "available_tools":request(at).tools.iter().map(|tool| &tool.name).collect::<Vec<_>>(),
        "phase":{"kind":"prepare_tools", "assistant":assistant_id, "next":0}
    }))
    .unwrap();
    assert_eq!(
        checkpoint.pending(),
        Some(history::Pending::Tools { next: 0 })
    );
    assert!(checkpoint.has_unfinished_tools());
    let scenario = Rc::new(support::Scenario::default());
    let cancelled_history = history.clone();
    let cancelled_tools = scenario.clone();
    let cancelled_checkpoint = checkpoint.clone();
    let cancelled = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                ctx.cancel()?;
                cancelled_history
                    .settle_tools(
                        &ctx,
                        cancelled_tools.as_ref(),
                        cancelled_checkpoint,
                        true,
                        None,
                    )
                    .await
            }
            .boxed_local()
        }))
        .unwrap_err();
    assert_eq!(cancelled.code, tembra::Code::Cancelled);
    let settled = settle(
        &mut pool,
        &host,
        history.clone(),
        scenario.clone(),
        checkpoint,
        false,
    )
    .unwrap();
    let history::ToolSettlement::Settled(checkpoint) = settled else {
        panic!("undispatched calls were uncertain")
    };
    let head = checkpoint.head();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move { history.context(&ctx, head, String::new()).await }.boxed_local()
        }))
        .unwrap();
    assert!(
        matches!(&context.messages[2], Message::Tool { result: ToolResult::Blocked(reason), .. } if reason == "not executed")
    );
    assert!(scenario.reconciled_tools.borrow().is_empty());
    assert!(scenario.effects.borrow().is_empty());
    assert!(scenario.models.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn settlement_applies_the_declared_result_transform_before_history_confirmation() {
    use std::cell::Cell;
    use swarmie_agent_loop::hooks::{Hooks, ToolOutput};
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let (host, reference, history) = open(&pool, &owner);
    let at = seed(
        &mut pool,
        &host,
        &reference,
        vec![Message::User {
            content: "Add one".into(),
        }],
    );
    let scenario = Rc::new(support::Scenario::default());
    scenario.uncertain_tool.set(true);
    let failed = execute(
        &mut pool,
        &host,
        history.clone(),
        support::agent(&host, scenario.clone()),
        request(at),
    )
    .unwrap_err();
    let checkpoint = *failed.checkpoint.unwrap();
    let service = "fixture.settlement-hooks";
    let result_hook = Hooks::default().after_tool_call;
    let bound_history = history.clone();
    let bound_tools = scenario.clone();
    let transformed = Rc::new(Cell::new(0));
    let observed = transformed.clone();
    host.install(vec![
        Hooks::default()
            .declare(tembra::Orb::new(service, 1))
            .service(
                tembra::Service::new(service, move |declaration| {
                    let hook: tembra::HookHandle<ToolOutput, ToolOutput> =
                        declaration.hook(&Hooks::default().after_tool_call)?;
                    Ok((bound_history.clone(), bound_tools.clone(), hook))
                })
                .method(
                    "settle",
                    |env, context, checkpoint: history::Checkpoint| async move {
                        match env
                            .0
                            .settle_tools(&context, env.1.as_ref(), checkpoint, false, Some(&env.2))
                            .await?
                        {
                            history::ToolSettlement::Settled(checkpoint) => Ok(checkpoint),
                            history::ToolSettlement::Unknown(_) => Err(tembra::Error::invalid(
                                "fixture.unknown",
                                "receipt remained unknown",
                            )),
                        }
                    },
                ),
            ),
        tembra::Orb::new("fixture.redaction", 1)
            .requires(tembra::core::Requirement {
                orb: service.into(),
                api_version: 1,
                methods: Default::default(),
                hooks: ["after_tool_call".into()].into(),
            })
            .contribute(service, &result_hook, 0, move |_, mut output| {
                observed.set(observed.get() + 1);
                assert_eq!(output.result, ToolResult::Success(json!(1)));
                output.result = ToolResult::Success(json!("redacted"));
                async move { Ok(output) }
            }),
    ])
    .unwrap();
    let managed = host.get(service, vec!["main".into()], "alice").unwrap();
    let checkpoint: history::Checkpoint = pool
        .run_until(host.root("alice", move |context| {
            async move { managed.call(&context, "settle", checkpoint).await }.boxed_local()
        }))
        .unwrap();
    let head = checkpoint.head();
    let context = pool
        .run_until(host.root("alice", move |context| {
            async move { history.context(&context, head, String::new()).await }.boxed_local()
        }))
        .unwrap();
    assert!(
        matches!(&context.messages[2], Message::Tool { result: ToolResult::Success(value), .. } if value == &json!("redacted"))
    );
    assert_eq!(transformed.get(), 1);
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
    assert_eq!(context.messages.len(), 3);
    pool.run_until(host.close()).unwrap();
}
