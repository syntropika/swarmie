#![cfg(feature = "context-projection")]

use futures::{FutureExt, executor::LocalPool, future::LocalBoxFuture};
use std::{cell::RefCell, num::NonZeroU32, rc::Rc};
use swarmie_agent_loop::{
    conversation::{self as history, History, Record},
    *,
};
use swarmie_context::{ArtifactRef, Configuration, Plan, Projection, Strategy};
use swarmie_conversations::{
    self as conversations, ConversationId, EntryDraft, Head, HistoryPoint, Method, Page, Read,
    WriteReceipt,
};
use swarmie_providers::mock::{MockModel, Step};
use swarmie_storage::sqlite::SqliteOwner;
use tembra::{Context, Host, Orb, Reference, Service};

struct NoTools;
impl Tools for NoTools {
    fn validate<'a>(
        &'a self,
        _: &'a Context,
        _: &'a ToolSpec,
        _: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async {
            Err(tembra::Error::invalid(
                "fixture.tools",
                "no tool validation expected",
            ))
        }
        .boxed_local()
    }
    fn execute<'a>(
        &'a self,
        _: &'a Context,
        _: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async {
            Err(tembra::Error::invalid(
                "fixture.tools",
                "no tool execution expected",
            ))
        }
        .boxed_local()
    }
}

fn response(content: &str) -> ModelResponse {
    ModelResponse {
        protection: swarmie_providers::SubmissionProtection::Clean,
        message: AssistantMessage {
            provider: None,
            content: content.into(),
            tool_calls: vec![],
        },
        usage: Usage::default(),
    }
}

fn open(
    pool: &LocalPool,
    path: &std::path::Path,
) -> (Host, Rc<Reference>, History, Rc<RefCell<Vec<Read>>>) {
    let owner = SqliteOwner::new(path);
    let host = Host::new(pool.spawner());
    host.install(vec![conversations::orb(owner.conversation_factory())])
        .unwrap();
    let raw = Rc::new(
        host.get(
            conversations::SERVICE,
            vec![conversations::INSTANCE_KEY.into()],
            "host",
        )
        .unwrap(),
    );
    let reads = Rc::new(RefCell::new(vec![]));
    let retained = reads.clone();
    let target = raw.clone();
    host.install(vec![
        Orb::new("fixture.projection-history", 1).service(
            Service::new("fixture.projection-history", move |_| {
                Ok((target.clone(), retained.clone()))
            })
            .method("read", |env, ctx, input: Read| async move {
                env.1.borrow_mut().push(input.clone());
                env.0.call::<_, Page>(&ctx, "read", input).await
            })
            .method("get", |env, ctx, input: (ConversationId,)| async move {
                env.0
                    .call::<_, Option<conversations::Conversation>>(&ctx, "get", input)
                    .await
            })
            .method(
                "append",
                |env, ctx, input: (Head, Vec<EntryDraft>)| async move {
                    env.0.call::<_, WriteReceipt>(&ctx, "append", input).await
                },
            ),
        ),
    ])
    .unwrap();
    let history = History::new(Rc::new(
        host.get("fixture.projection-history", vec!["main".into()], "host")
            .unwrap(),
    ));
    (host, raw, history, reads)
}

fn call<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    method: Method,
    args: A,
) -> R {
    let reference = reference.clone();
    pool.run_until(host.root("alice", move |context| {
        async move { reference.call(&context, method.as_str(), args).await }.boxed_local()
    }))
    .unwrap()
}
fn append(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    at: Head,
    records: Vec<Record>,
) -> Head {
    let entries = records
        .into_iter()
        .map(|record| EntryDraft::encode(record).unwrap())
        .collect::<Vec<_>>();
    let receipt: WriteReceipt = call(pool, host, reference, Method::Append, (at, entries));
    receipt.head
}
fn plan(through: Head) -> Plan {
    Plan {
        through: HistoryPoint {
            conversation: through.conversation,
            entry: through.entry.unwrap(),
        },
        previous: None,
        configuration: Configuration([1; 32]),
        strategy: Strategy::Textual {
            model: Configuration([2; 32]),
        },
    }
}
fn result(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    parent: Head,
    plan: Plan,
    content: &str,
) -> Projection {
    let operation = OperationId::new();
    let at = append(
        pool,
        host,
        reference,
        parent,
        vec![Record::ContextResult {
            operation,
            plan: plan.clone(),
            response: response(content),
        }],
    );
    Projection {
        operation,
        plan,
        artifact: ArtifactRef::new(at, parent.entry).unwrap(),
    }
}
fn create(pool: &mut LocalPool, host: &Host, reference: &Rc<Reference>) -> Head {
    let created: WriteReceipt = call(
        pool,
        host,
        reference,
        Method::Create,
        (ConversationId::new(),),
    );
    created.head
}

#[test]
fn future_artifact_lookup_keeps_the_original_input_head_and_never_reads_the_old_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let (host, raw, history, reads) = open(&pool, &dir.path().join("history.sqlite"));
    let empty = create(&mut pool, &host, &raw);
    let through = append(
        &mut pool,
        &host,
        &raw,
        empty,
        (0..300)
            .map(|index| {
                Record::Message(Message::User {
                    content: format!("Old source {index}: {}", "x".repeat(1024)),
                })
            })
            .collect(),
    );
    let pending = "🙂\"\\".repeat(349_525);
    let initial = append(
        &mut pool,
        &host,
        &raw,
        through,
        vec![Record::Message(Message::User {
            content: pending.clone(),
        })],
    );
    let steered = append(
        &mut pool,
        &host,
        &raw,
        initial,
        vec![Record::Message(Message::User {
            content: "Later steer".into(),
        })],
    );
    let projection = result(
        &mut pool,
        &host,
        &raw,
        steered,
        plan(through),
        "Recognized decisions",
    );
    let frozen = projection.clone();
    let selected = history.clone();
    let context = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                selected
                    .context_with_projection(&ctx, initial, "Current instructions".into(), frozen)
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(context.instructions, "Current instructions");
    assert_eq!(context.messages.len(), 2);
    assert_eq!(context.messages[1], Message::User { content: pending });
    let Message::Assistant(source) = &context.messages[0] else {
        panic!("typed Assistant source expected")
    };
    assert!(source.tool_calls.is_empty());
    assert!(
        source
            .content
            .contains("swarmie.context.textual_projection")
    );
    assert!(source.content.contains("Recognized decisions"));
    assert!(!source.content.contains("Later steer"));
    assert_eq!(reads.borrow().len(), 3);
    assert!(reads.borrow().iter().all(|read| read.after.is_some()));
    assert_eq!(reads.borrow()[1].at, projection.artifact.at());
    assert_eq!(reads.borrow()[1].limit.get(), 1);
    let all: Page = call(
        &mut pool,
        &host,
        &raw,
        Method::Read,
        Read {
            at: projection.artifact.at(),
            after: Some(through.entry.unwrap()),
            limit: Read::MAX_LIMIT,
        },
    );
    assert_eq!(
        all.entries
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::ContextResult { .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        all.entries
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::Message(Message::Assistant(_))
            ))
            .count(),
        0
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn invalid_parent_foreign_prefix_mismatched_plan_and_native_projection_fail_before_a_model_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let (host, raw, history, _) = open(&pool, &dir.path().join("history.sqlite"));
    let empty = create(&mut pool, &host, &raw);
    let through = append(
        &mut pool,
        &host,
        &raw,
        empty,
        vec![Record::Message(Message::User {
            content: "Old input".into(),
        })],
    );
    let initial = append(
        &mut pool,
        &host,
        &raw,
        through,
        vec![Record::Message(Message::User {
            content: "Retain this input".into(),
        })],
    );
    let projection = result(&mut pool, &host, &raw, initial, plan(through), "Checkpoint");
    let mut bad_parent = projection.clone();
    bad_parent.artifact = ArtifactRef::new(projection.artifact.at(), through.entry).unwrap();
    let mut foreign = projection.clone();
    foreign.plan.through.entry = swarmie_conversations::EntryId::new();
    let mut mismatch = projection.clone();
    mismatch.plan.configuration = Configuration([3; 32]);
    let mut native = projection.clone();
    native.plan.strategy = Strategy::Native {
        qualification: Configuration([4; 32]),
    };
    let model = Rc::new(MockModel::new([Step::Reply(response("Never dispatch"))]));
    for selected in [bad_parent, foreign, mismatch, native] {
        let mut input = history::Request::new(
            projection.artifact.at(),
            "Current instructions".into(),
            vec![],
            NonZeroU32::MIN,
        );
        input.projection = Some(selected);
        let agent = Agent::new(model.clone(), Rc::new(NoTools));
        let history = history.clone();
        let outcome = pool
            .run_until(host.root("alice", move |context| {
                async move { Ok(history.execute(&context, &agent, input).await) }.boxed_local()
            }))
            .unwrap();
        assert!(outcome.unwrap_err().checkpoint.is_none());
    }
    assert!(model.calls().is_empty());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn sqlite_reopen_recovers_the_frozen_projection_without_repeating_effects_or_persisting_a_synthetic_message()
 {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let (host, raw, history, _) = open(&pool, &path);
    let empty = create(&mut pool, &host, &raw);
    let through = append(
        &mut pool,
        &host,
        &raw,
        empty,
        vec![Record::Message(Message::User {
            content: "Old input".into(),
        })],
    );
    let initial = append(
        &mut pool,
        &host,
        &raw,
        through,
        vec![Record::Message(Message::User {
            content: "Current real input".into(),
        })],
    );
    let first = result(
        &mut pool,
        &host,
        &raw,
        initial,
        plan(through),
        "First frozen checkpoint",
    );
    let mut next_plan = plan(through);
    next_plan.previous = Some(first.artifact.entry());
    let second = result(
        &mut pool,
        &host,
        &raw,
        first.artifact.at(),
        next_plan,
        "A later active checkpoint",
    );
    let mut input = history::Request::new(
        second.artifact.at(),
        "Current instructions".into(),
        vec![],
        NonZeroU32::MIN,
    );
    input.projection = Some(first.clone());
    let model = Rc::new(MockModel::new([Step::LoseReply(response(
        "Recognized response",
    ))]));
    let agent = Agent::new(model.clone(), Rc::new(NoTools));
    let failure = pool
        .run_until(host.root("alice", move |context| {
            async move { Ok(history.execute(&context, &agent, input).await.unwrap_err()) }
                .boxed_local()
        }))
        .unwrap();
    let checkpoint = *failure.checkpoint.unwrap();
    assert_eq!(checkpoint.projection(), Some(&first));
    let encoded = serde_json::to_string(&checkpoint).unwrap();
    assert!(!encoded.contains("First frozen checkpoint"));
    pool.run_until(host.close()).unwrap();
    let (host, raw, history, _) = open(&pool, &path);
    let checkpoint = serde_json::from_str(&encoded).unwrap();
    let agent = Agent::new(model.clone(), Rc::new(NoTools));
    let done = pool
        .run_until(host.root("alice", move |context| {
            async move { Ok(history.resume(&context, &agent, checkpoint).await.unwrap()) }
                .boxed_local()
        }))
        .unwrap();
    assert_eq!(model.calls().len(), 1);
    assert_eq!(model.recoveries().len(), 1);
    assert_eq!(done.output.messages.len(), 1);
    assert!(
        serde_json::to_string(&model.calls()[0].request.context.messages[0])
            .unwrap()
            .contains("First frozen checkpoint")
    );
    assert!(
        !serde_json::to_string(&model.calls()[0].request.context.messages[0])
            .unwrap()
            .contains("A later active checkpoint")
    );
    let mut read = Read::new(done.checkpoint.head());
    let mut canonical = vec![];
    loop {
        let page: Page = call(&mut pool, &host, &raw, Method::Read, read.clone());
        canonical.extend(page.entries);
        if let Some(next) = page.next {
            read.after = Some(next);
        } else {
            break;
        }
    }
    assert_eq!(
        canonical
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::ContextResult { .. }
            ))
            .count(),
        2
    );
    let messages = canonical
        .iter()
        .filter_map(|entry| match entry.decode::<Record>().unwrap() {
            Record::Message(message) => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 3);
    assert!(
        !serde_json::to_string(&messages)
            .unwrap()
            .contains("swarmie.context.textual_projection")
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn adoption_advances_only_one_verified_evidence_append_and_preserves_the_immutable_task_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let (host, raw, history, _) = open(&pool, &dir.path().join("history.sqlite"));
    let empty = create(&mut pool, &host, &raw);
    let through = append(
        &mut pool,
        &host,
        &raw,
        empty,
        vec![Record::Message(Message::User {
            content: "Old source".into(),
        })],
    );
    let initial = append(
        &mut pool,
        &host,
        &raw,
        through,
        vec![Record::Message(Message::User {
            content: "Retain actual input".into(),
        })],
    );
    // Unsupported preparation fails locally before model intent, yielding a real
    // PrepareTurn checkpoint over the admitted immutable input.
    let blocked = Agent::new(Rc::new(MockModel::new([])), Rc::new(NoTools))
        .with_preparation(Rc::new(swarmie_context::BudgetPolicy::new(0)));
    let request = history::Request::new(
        initial,
        "Current instructions".into(),
        vec![],
        NonZeroU32::MIN,
    );
    let selected = history.clone();
    let checkpoint = pool
        .run_until(host.root("alice", move |context| {
            async move {
                Ok(*selected
                    .execute(&context, &blocked, request)
                    .await
                    .unwrap_err()
                    .checkpoint
                    .unwrap())
            }
            .boxed_local()
        }))
        .unwrap();
    let before = serde_json::to_value(&checkpoint).unwrap();
    let projection = result(
        &mut pool,
        &host,
        &raw,
        initial,
        plan(through),
        "Usable checkpoint",
    );
    let selected = history.clone();
    let adopted_projection = projection.clone();
    let adopted = pool
        .run_until(host.root("alice", move |context| {
            async move {
                selected
                    .adopt_projection(&context, checkpoint, adopted_projection)
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(adopted.head(), projection.artifact.at());
    assert_eq!(adopted.projection(), Some(&projection));
    let encoded = serde_json::to_value(&adopted).unwrap();
    assert_eq!(encoded["input"], before["input"]);
    assert!(!encoded.to_string().contains("Usable checkpoint"));
    let model = Rc::new(MockModel::new([Step::Reply(response("Done"))]));
    let observed = model.clone();
    let agent = Agent::new(model, Rc::new(NoTools));
    let done = pool
        .run_until(host.root("alice", move |context| {
            async move {
                Ok(history
                    .resume(&context, &agent, serde_json::from_value(encoded).unwrap())
                    .await
                    .unwrap())
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(observed.calls().len(), 1);
    assert_eq!(done.output.messages.len(), 1);
    let page: Page = call(
        &mut pool,
        &host,
        &raw,
        Method::Read,
        Read::new(done.checkpoint.head()),
    );
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::ContextResult { .. }
            ))
            .count(),
        1
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn projection_rebases_captured_sources_after_summary_and_before_admitted_user() {
    use swarmie_providers::{ContextSources, SourceKind, SourcePart, SourceScope};
    let dir = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let (host, raw, history, _) = open(&pool, &dir.path().join("history.sqlite"));
    let empty = create(&mut pool, &host, &raw);
    let through = append(
        &mut pool,
        &host,
        &raw,
        empty,
        vec![Record::Message(Message::User {
            content: "Old source".into(),
        })],
    );
    let initial = append(
        &mut pool,
        &host,
        &raw,
        through,
        vec![Record::Message(Message::User {
            content: "Retained input".into(),
        })],
    );
    let projection = result(
        &mut pool,
        &host,
        &raw,
        initial,
        plan(through),
        "Historical summary marker",
    );
    let sources = ContextSources::new(
        SourceScope {
            node: "node".into(),
            principal: "alice".into(),
            directory: "project".into(),
        },
        vec![
            SourcePart::new(
                SourceKind::ProjectInstructions,
                "id".into(),
                "revision".into(),
                "AGENTS.md".into(),
                "Current project marker".into(),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut request = history::Request::new(
        projection.artifact.at(),
        String::new(),
        vec![],
        NonZeroU32::new(2).unwrap(),
    )
    .with_sources(sources)
    .unwrap();
    request.projection = Some(projection);
    let model = Rc::new(MockModel::new([Step::Reply(response("Done"))]));
    let agent = Agent::new(model.clone(), Rc::new(NoTools));
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            history
                .execute(&ctx, &agent, request)
                .await
                .map_err(|failure| failure.error)
        }
        .boxed_local()
    }))
    .unwrap();
    let calls = model.calls();
    let logical = calls[0].request.context.model_messages().unwrap();
    assert_eq!(logical.len(), 3);
    assert!(
        matches!(&logical[0],Message::Assistant(message) if message.content.contains("Historical summary marker"))
    );
    assert!(
        matches!(&logical[1],Message::User{content} if content.contains("Current project marker"))
    );
    assert!(matches!(&logical[2],Message::User{content} if content=="Retained input"));
    pool.run_until(host.close()).unwrap();
}
