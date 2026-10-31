#![cfg(all(feature = "conversations", feature = "durable-tasks"))]

#[path = "support/request.rs"]
mod input;
#[path = "support/scenario.rs"]
mod scenario;

use futures::{FutureExt, executor::LocalPool};
use std::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
    rc::Rc,
};
use swarmie_agent_loop::{
    conversation::{self as history, History, Record},
    *,
};
use swarmie_conversations::{
    self as conversations, ConversationId, EntryDraft, Head, Read, WriteReceipt,
};
use swarmie_durable_tasks::{self as tasks, TaskRecord, TaskStatus};
use swarmie_storage::sqlite::{SqliteOwner, SqliteTaskStorage};
use tembra::{Code, Host, Orb, Reference, Service};

#[derive(Clone, Copy, PartialEq)]
enum Gate {
    Open,
    Prepared,
    PreparedTool,
    ToolResult,
}

fn guarded(inner: Rc<Reference>, gate: Rc<Cell<Gate>>) -> Orb {
    Orb::new("fixture.history-gate", 1).service(
        Service::new("fixture.history-gate", move |_| {
            Ok((inner.clone(), gate.clone()))
        })
        .method(
            conversations::Method::Get.as_str(),
            |env, ctx, args: (ConversationId,)| async move {
                env.0
                    .call::<_, Option<conversations::Conversation>>(
                        &ctx,
                        conversations::Method::Get.as_str(),
                        args,
                    )
                    .await
            },
        )
        .method(
            conversations::Method::Read.as_str(),
            |env, ctx, args: Read| async move {
                env.0
                    .call::<_, conversations::Page>(
                        &ctx,
                        conversations::Method::Read.as_str(),
                        args,
                    )
                    .await
            },
        )
        .method(
            conversations::Method::Append.as_str(),
            |env, ctx, (head, entries): (Head, Vec<EntryDraft>)| async move {
                let refuse_staged_tool = env.1.get() == Gate::PreparedTool
                    && entries.iter().any(|entry| {
                        matches!(
                            serde_json::from_value::<Record>(entry.data.clone()).unwrap(),
                            Record::PreparedTool { .. }
                        )
                    });
                let reject = entries.iter().any(|entry| {
                    match serde_json::from_value::<Record>(entry.data.clone()).unwrap() {
                        Record::PreparedRequest { .. } => env.1.get() == Gate::Prepared,
                        Record::Message(Message::Tool { .. }) => env.1.get() == Gate::ToolResult,
                        _ => false,
                    }
                });
                if reject {
                    env.1.set(Gate::Open);
                    return Err(tembra::Error::new(
                        tembra::Code::Unavailable,
                        "fixture.history-refused",
                        "history preparation refused",
                    ));
                }
                let receipt = env
                    .0
                    .call::<_, WriteReceipt>(
                        &ctx,
                        conversations::Method::Append.as_str(),
                        (head, entries),
                    )
                    .await?;
                if refuse_staged_tool {
                    env.1.set(Gate::Open);
                    return Err(tembra::Error::new(
                        tembra::Code::Unavailable,
                        "fixture.history-refused",
                        "refused after staging prepared tool",
                    ));
                }
                Ok(receipt)
            },
        ),
    )
}

fn open(
    pool: &LocalPool,
    owner: &SqliteOwner,
    scenario: Rc<scenario::Scenario>,
    gate: Rc<Cell<Gate>>,
) -> (Host, Rc<Reference>, Rc<Reference>) {
    open_with_model(
        pool,
        owner,
        scenario.clone(),
        scenario,
        gate,
        owner.task_factory(),
    )
}

fn open_with_model(
    pool: &LocalPool,
    owner: &SqliteOwner,
    model: Rc<dyn Model>,
    scenario: Rc<scenario::Scenario>,
    gate: Rc<Cell<Gate>>,
    storage: tasks::StorageFactory,
) -> (Host, Rc<Reference>, Rc<Reference>) {
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
    host.install(vec![guarded(reference.clone(), gate)])
        .unwrap();
    let binding_host = host.clone();
    host.install(vec![
        tasks::orb(
            host.clone(),
            storage,
            tasks::Definitions::new([history::durable::definition()]).unwrap(),
            tasks::Options::new(
                pool.spawner(),
                NonZeroU64::new(60_000).unwrap(),
                Rc::new(|duration| {
                    async move { futures_timer::Delay::new(duration).await }.boxed_local()
                }),
            ),
        ),
        history::durable::orb(move |declaration| {
            Ok((
                Agent::new(model.clone(), scenario.clone()),
                History::bind(
                    &declaration,
                    binding_host.clone(),
                    "fixture.history-gate",
                    vec!["main".into()],
                )?,
            ))
        }),
    ])
    .unwrap();
    let scheduler = Rc::new(
        host.get(tasks::SERVICE, vec![tasks::INSTANCE_KEY.into()], "host")
            .unwrap(),
    );
    (host, reference, scheduler)
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
    let method = method.to_string();
    pool.run_until(host.root(principal, move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    }))
    .unwrap()
}

fn create(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    scheduler: &Rc<Reference>,
) -> tasks::TaskId {
    let created: WriteReceipt = call(
        pool,
        host,
        reference,
        "alice",
        conversations::Method::Create.as_str(),
        (ConversationId::new(),),
    );
    let base = input::request();
    let entries: Vec<_> = base
        .context
        .messages
        .into_iter()
        .map(|message| EntryDraft::encode(Record::Message(message)).unwrap())
        .collect();
    let appended: WriteReceipt = call(
        pool,
        host,
        reference,
        "alice",
        conversations::Method::Append.as_str(),
        (created.head, entries),
    );
    let request = history::Request::new(
        appended.head,
        base.context.instructions,
        base.tools,
        base.max_turns,
    );
    call(
        pool,
        host,
        scheduler,
        "alice",
        tasks::Method::Create.as_str(),
        (tasks::TaskSpec {
            id: tasks::TaskId::new(),
            definition: history::durable::definition(),
            input: serde_json::to_value(request).unwrap(),
        },),
    )
}

fn resume(pool: &mut LocalPool, host: &Host, scheduler: &Rc<Reference>) {
    call::<_, ()>(
        pool,
        host,
        scheduler,
        "host",
        tasks::Method::Resume.as_str(),
        (),
    );
    pool.run_until_stalled();
}

fn record(
    pool: &mut LocalPool,
    host: &Host,
    scheduler: &Rc<Reference>,
    id: tasks::TaskId,
) -> TaskRecord {
    call::<_, Option<TaskRecord>>(
        pool,
        host,
        scheduler,
        "alice",
        tasks::Method::Get.as_str(),
        (id,),
    )
    .unwrap()
}

#[test]
fn durable_history_reopens_and_reconciles_a_lost_tool_reply_without_duplicate_messages() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let owner = SqliteOwner::new(&file);
    let scenario = Rc::new(scenario::Scenario::default());
    scenario.uncertain_tool.set(true);
    let (host, reference, scheduler) = open(
        &pool,
        &owner,
        scenario.clone(),
        Rc::new(Cell::new(Gate::Open)),
    );
    let id = create(&mut pool, &host, &reference, &scheduler);
    resume(&mut pool, &host, &scheduler);
    let first = record(&mut pool, &host, &scheduler, id);
    assert_eq!(first.status, TaskStatus::Ready);
    assert!(first.assignment.is_none());
    assert_eq!(
        first.checkpoint.as_ref().unwrap()["phase"]["kind"],
        "tool_pending"
    );
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
    let owner = SqliteOwner::new(&file);
    let (host, reference, scheduler) = open(
        &pool,
        &owner,
        scenario.clone(),
        Rc::new(Cell::new(Gate::Open)),
    );
    resume(&mut pool, &host, &scheduler);
    let last = record(&mut pool, &host, &scheduler, id);
    assert!(matches!(last.status, TaskStatus::Completed(_)));
    assert!(last.assignment.is_none());
    let checkpoint: history::Checkpoint = serde_json::from_value(last.checkpoint.unwrap()).unwrap();
    assert!(serde_json::to_vec(&checkpoint).unwrap().len() < 1500);
    let page: conversations::Page = call(
        &mut pool,
        &host,
        &reference,
        "alice",
        conversations::Method::Read.as_str(),
        Read::new(checkpoint.head()),
    );
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::Message(Message::Tool { .. })
            ))
            .count(),
        1
    );
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(
        scenario.reconciled_tools.borrow().as_slice(),
        scenario.effects.borrow().as_slice()
    );
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().calls, 0);
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn refused_prepared_request_prevents_dispatch_and_leaves_no_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let scenario = Rc::new(scenario::Scenario::default());
    let (host, reference, scheduler) = open(
        &pool,
        &owner,
        scenario.clone(),
        Rc::new(Cell::new(Gate::Prepared)),
    );
    let id = create(&mut pool, &host, &reference, &scheduler);
    resume(&mut pool, &host, &scheduler);
    let record = record(&mut pool, &host, &scheduler, id);
    assert_eq!(record.status, TaskStatus::Ready);
    assert!(record.checkpoint.is_none());
    assert!(scenario.models.borrow().is_empty());
    assert!(scenario.effects.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn refused_tool_result_keeps_pending_intent_then_recovers_without_repeating_effect() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let scenario = Rc::new(scenario::Scenario::default());
    let (host, reference, scheduler) = open(
        &pool,
        &owner,
        scenario.clone(),
        Rc::new(Cell::new(Gate::ToolResult)),
    );
    let id = create(&mut pool, &host, &reference, &scheduler);
    resume(&mut pool, &host, &scheduler);
    let first = record(&mut pool, &host, &scheduler, id);
    assert_eq!(first.status, TaskStatus::Ready);
    assert_eq!(
        first.checkpoint.as_ref().unwrap()["phase"]["kind"],
        "tool_pending"
    );
    let saved: history::Checkpoint = serde_json::from_value(first.checkpoint.unwrap()).unwrap();
    let page: conversations::Page = call(
        &mut pool,
        &host,
        &reference,
        "alice",
        conversations::Method::Read.as_str(),
        Read::new(saved.head()),
    );
    assert!(!page.entries.iter().any(|entry| matches!(
        entry.decode::<Record>().unwrap(),
        Record::Message(Message::Tool { .. })
    )));
    assert_eq!(scenario.effects.borrow().len(), 1);
    resume(&mut pool, &host, &scheduler);
    let last = record(&mut pool, &host, &scheduler, id);
    assert!(matches!(last.status, TaskStatus::Completed(_)));
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn prepared_tool_and_checkpoint_commit_together_and_reopen_with_exact_arguments() {
    use serde_json::json;
    use swarmie_agent_loop::hooks::{HookName, Hooks};
    use swarmie_providers::mock::{MockModel, Step};
    use tembra::core::Requirement;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let scenario = Rc::new(scenario::Scenario::default());
    scenario.uncertain_tool.set(true);
    let model = Rc::new(MockModel::new([
        Step::Reply(ModelResponse {
            protection: swarmie_providers::SubmissionProtection::Unprotected,
            message: AssistantMessage {
                provider: None,
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: ToolCallId::new("call-1").unwrap(),
                    name: ToolName::new("counter.add").unwrap(),
                    arguments: json!({"amount":99}),
                }],
            },
            usage: Usage::default(),
        }),
        Step::Reply(ModelResponse {
            protection: swarmie_providers::SubmissionProtection::Unprotected,
            message: AssistantMessage {
                provider: None,
                content: "Done".into(),
                tool_calls: vec![],
            },
            usage: Usage::default(),
        }),
    ]));
    let preparations = Rc::new(Cell::new(0));
    let install_hook = |host: &Host| {
        let count = preparations.clone();
        host.install(vec![
            Orb::new("fixture.prepare-tool", 1)
                .requires(Requirement {
                    orb: history::durable::SERVICE.into(),
                    api_version: 1,
                    methods: Default::default(),
                    hooks: [HookName::BeforeToolCall.as_str().into()].into(),
                })
                .contribute(
                    history::durable::SERVICE,
                    &Hooks::default().before_tool_call,
                    0,
                    move |_, mut v| {
                        count.set(count.get() + 1);
                        v.call.arguments = json!({"amount":1});
                        async { Ok(v) }
                    },
                ),
        ])
        .unwrap();
    };
    let owner = SqliteOwner::new(&file);
    let (host, reference, scheduler) = open_with_model(
        &pool,
        &owner,
        model.clone(),
        scenario.clone(),
        Rc::new(Cell::new(Gate::PreparedTool)),
        owner.task_factory(),
    );
    install_hook(&host);
    let id = create(&mut pool, &host, &reference, &scheduler);
    resume(&mut pool, &host, &scheduler);
    let refused = record(&mut pool, &host, &scheduler, id);
    assert_eq!(refused.status, TaskStatus::Ready);
    let checkpoint = refused.checkpoint.unwrap();
    assert_eq!(checkpoint["phase"]["kind"], "prepare_tools");
    assert!(scenario.tool_inputs.borrow().is_empty());
    let saved: history::Checkpoint = serde_json::from_value(checkpoint).unwrap();
    let page: conversations::Page = call(
        &mut pool,
        &host,
        &reference,
        "alice",
        conversations::Method::Read.as_str(),
        Read::new(saved.head()),
    );
    assert!(!page.entries.iter().any(|entry| matches!(
        entry.decode::<Record>().unwrap(),
        Record::PreparedTool { .. }
    )));
    // Read the actual current head as well: a rolled-back record must not be hidden
    // merely because the checkpoint retained its previous immutable selection.
    let conversation: Option<conversations::Conversation> = call(
        &mut pool,
        &host,
        &reference,
        "alice",
        conversations::Method::Get.as_str(),
        (saved.head().conversation,),
    );
    assert_eq!(conversation.unwrap().head, saved.head());
    resume(&mut pool, &host, &scheduler);
    let pending = record(&mut pool, &host, &scheduler, id);
    assert_eq!(pending.status, TaskStatus::Ready);
    assert_eq!(
        pending.checkpoint.as_ref().unwrap()["phase"]["kind"],
        "tool_pending"
    );
    assert!(!pending.checkpoint.as_ref().unwrap()["phase"]["prepared"].is_null());
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();

    let owner = SqliteOwner::new(&file);
    let (host, reference, scheduler) = open_with_model(
        &pool,
        &owner,
        model,
        scenario.clone(),
        Rc::new(Cell::new(Gate::Open)),
        owner.task_factory(),
    );
    install_hook(&host);
    resume(&mut pool, &host, &scheduler);
    let completed = record(&mut pool, &host, &scheduler, id);
    assert!(matches!(completed.status, TaskStatus::Completed(_)));
    let saved: history::Checkpoint = serde_json::from_value(completed.checkpoint.unwrap()).unwrap();
    let page: conversations::Page = call(
        &mut pool,
        &host,
        &reference,
        "alice",
        conversations::Method::Read.as_str(),
        Read::new(saved.head()),
    );
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(
                entry.decode::<Record>().unwrap(),
                Record::PreparedTool { .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        scenario.tool_inputs.borrow()[0].arguments,
        json!({"amount":1})
    );
    assert_eq!(
        *scenario.tool_inputs.borrow(),
        *scenario.recovered_inputs.borrow()
    );
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(preparations.get(), 2);
    pool.run_until(host.close()).unwrap();
}

#[derive(Clone, Copy, Debug)]
enum ConfirmationCase {
    ModelPending,
    ToolPending,
}

impl ConfirmationCase {
    fn as_str(self) -> &'static str {
        match self {
            Self::ModelPending => "model_pending",
            Self::ToolPending => "tool_pending",
        }
    }
}

/// Refuse one selected checkpoint before SQLite applies its transaction.
struct RefusingStorage {
    inner: SqliteTaskStorage,
    case: ConfirmationCase,
    code: Code,
    refused: Cell<bool>,
    joint: Cell<Option<bool>>,
    before: RefCell<Option<TaskRecord>>,
}

impl RefusingStorage {
    async fn check(
        &self,
        changes: &[tasks::TaskChange],
        joint: bool,
    ) -> Result<(), tasks::StorageError> {
        if !self.refused.get() {
            for change in changes {
                if let tasks::TaskChange::Checkpoint { id, checkpoint, .. } = change
                    && checkpoint["phase"]["kind"] == self.case.as_str()
                {
                    use tasks::TaskStorage;
                    let before = self.inner.get(*id).await?;
                    self.before.replace(before);
                    self.refused.set(true);
                    self.joint.set(Some(joint));
                    return Err(tasks::StorageError::Refused(tembra::Error::new(
                        self.code,
                        "fixture.confirmation-refused",
                        "checkpoint confirmation refused",
                    )));
                }
            }
        }
        Ok(())
    }
}

impl tasks::TaskStorage for RefusingStorage {
    fn owner(&self) -> swarmie_storage::StorageOwner {
        self.inner.owner()
    }

    fn changes(&self) -> swarmie_storage::ChangeSource {
        self.inner.changes()
    }

    fn get(
        &self,
        id: tasks::TaskId,
    ) -> futures::future::LocalBoxFuture<'_, Result<Option<TaskRecord>, tasks::StorageError>> {
        self.inner.get(id)
    }

    fn list(
        &self,
        principal: Option<String>,
        page: tasks::TaskPageRequest,
    ) -> futures::future::LocalBoxFuture<'_, Result<Vec<tasks::TaskSummary>, tasks::StorageError>>
    {
        self.inner.list(principal, page)
    }

    fn candidates(
        &self,
        after: u64,
        limit: usize,
    ) -> futures::future::LocalBoxFuture<'_, Result<tasks::CandidateSnapshot, tasks::StorageError>>
    {
        self.inner.candidates(after, limit)
    }

    fn commit(
        &self,
        changes: Vec<tasks::TaskChange>,
    ) -> futures::future::LocalBoxFuture<'_, Result<tasks::CommitOutcome, tasks::StorageError>>
    {
        async move {
            self.check(&changes, false).await?;
            self.inner.commit(changes).await
        }
        .boxed_local()
    }

    fn commit_with(
        &self,
        changes: Vec<tasks::TaskChange>,
        prepared: swarmie_storage::PreparedChanges,
    ) -> futures::future::LocalBoxFuture<'_, Result<tasks::CommitOutcome, tasks::StorageError>>
    {
        async move {
            self.check(&changes, true).await?;
            self.inner.commit_with(changes, prepared).await
        }
        .boxed_local()
    }

    fn close(&self) -> futures::future::LocalBoxFuture<'_, Result<(), tasks::StorageError>> {
        self.inner.close()
    }
}

#[test]
fn checkpoint_only_and_joint_refusals_share_recovery_classification() {
    use swarmie_agent_loop::hooks::{Cause, ExecutionStatus, HookName, Hooks};
    use tembra::core::Requirement;

    for case in [
        ConfirmationCase::ModelPending,
        ConfirmationCase::ToolPending,
    ] {
        let joint = matches!(case, ConfirmationCase::ModelPending);
        for code in [Code::Conflict, Code::Unavailable] {
            let dir = tempfile::tempdir().unwrap();
            let owner = SqliteOwner::new(dir.path().join("history.sqlite"));
            let mut pool = LocalPool::new();
            let scenario = Rc::new(scenario::Scenario::default());
            let storage = Rc::new(RefusingStorage {
                inner: owner.tasks().unwrap(),
                case,
                code,
                refused: Cell::new(false),
                joint: Cell::new(None),
                before: RefCell::new(None),
            });
            let factory = storage.clone();
            let (host, reference, scheduler) = open_with_model(
                &pool,
                &owner,
                scenario.clone(),
                scenario.clone(),
                Rc::new(Cell::new(Gate::Open)),
                Rc::new(move || {
                    let storage = factory.clone();
                    async move { Ok(storage as Rc<dyn tasks::TaskStorage>) }.boxed_local()
                }),
            );
            let failures = Rc::new(RefCell::new(Vec::<Cause>::new()));
            let observed = failures.clone();
            host.install(vec![
                Orb::new("fixture.confirmation-observer", 1)
                    .requires(Requirement {
                        orb: history::durable::SERVICE.into(),
                        api_version: 1,
                        methods: Default::default(),
                        hooks: [HookName::AfterTurnAgent.as_str().into()].into(),
                    })
                    .contribute(
                        history::durable::SERVICE,
                        &Hooks::default().after_turn_agent,
                        0,
                        move |_, outcome| {
                            if let ExecutionStatus::Interrupted(cause) = outcome.status {
                                observed.borrow_mut().push(cause);
                            }
                            async { Ok(()) }
                        },
                    ),
            ])
            .unwrap();
            let id = create(&mut pool, &host, &reference, &scheduler);
            resume(&mut pool, &host, &scheduler);
            assert_eq!(storage.joint.get(), Some(joint));
            let failure = failures.borrow()[0].clone();
            assert_eq!(failure.code, Code::Unavailable, "{case:?}: {code:?}");
            assert_eq!(
                failure.reason,
                FailureReason::RecoveryRequired.as_str(),
                "{case:?}: {code:?}"
            );
            let refused = record(&mut pool, &host, &scheduler, id);
            let before = storage.before.borrow().clone().unwrap();
            assert_eq!(refused.status, TaskStatus::Ready);
            assert!(refused.assignment.is_none());
            assert_eq!(refused.checkpoint, before.checkpoint);
            let head = if let Some(checkpoint) = before.checkpoint {
                serde_json::from_value::<history::Checkpoint>(checkpoint)
                    .unwrap()
                    .head()
            } else {
                serde_json::from_value::<history::Request>(before.spec.input)
                    .unwrap()
                    .at
            };
            let current: Option<conversations::Conversation> = call(
                &mut pool,
                &host,
                &reference,
                "alice",
                conversations::Method::Get.as_str(),
                (head.conversation,),
            );
            assert_eq!(current.unwrap().head, head);
            assert_eq!(scenario.models.borrow().len(), usize::from(!joint));
            assert!(scenario.effects.borrow().is_empty());
            assert!(scenario.tool_inputs.borrow().is_empty());
            assert!(scenario.reconciled_tools.borrow().is_empty());
            resume(&mut pool, &host, &scheduler);
            assert!(matches!(
                record(&mut pool, &host, &scheduler, id).status,
                TaskStatus::Completed(_)
            ));
            assert_eq!(scenario.effects.borrow().len(), 1);
            assert_eq!(scenario.tool_inputs.borrow().len(), 1);
            assert!(scenario.reconciled_tools.borrow().is_empty());
            pool.run_until(host.close()).unwrap();
        }
    }
}
