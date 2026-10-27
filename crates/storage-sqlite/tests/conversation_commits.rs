use futures::{FutureExt, executor::LocalPool};
use serde::{Deserialize, Serialize};
use std::{
    num::{NonZeroU32, NonZeroU64},
    rc::Rc,
};
use swarmie_conversations::{self as history, *};
use swarmie_durable_tasks::{self as tasks, TaskStorage};
use swarmie_storage_sqlite::SqliteOwner;
use tembra::{Host, Orb, Reference, Service};

const TASK: &str = "fixture.conversation-commit";

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Mode {
    Commit,
    CallbackFailure,
    ConflictingAppend,
}

#[derive(Clone, Serialize, Deserialize)]
struct Input {
    head: Head,
    entry: EntryDraft,
    mode: Mode,
}

fn definition() -> tasks::TaskDefinition {
    tasks::TaskDefinition::new(TASK, NonZeroU32::new(1).unwrap()).unwrap()
}

fn task_orb(conversations: Rc<Reference>) -> Orb {
    Orb::new(TASK, 1).service(
        Service::new(TASK, move |_| Ok(conversations.clone())).method(
            tasks::TaskMethod::Execute.as_str(),
            |conversations, ctx, execution: tasks::Execution| async move {
                let input: Input = serde_json::from_value(execution.input()).unwrap();
                let conversations = conversations.as_ref().clone();
                execution
                    .commit_with(&ctx, move |writing| {
                        async move {
                            let receipt: WriteReceipt = conversations
                                .call(
                                    &writing,
                                    Method::Append.as_str(),
                                    (input.head, vec![input.entry]),
                                )
                                .await?;
                            assert_eq!(receipt.confirmation, WriteConfirmation::Staged);
                            match input.mode {
                                Mode::Commit => {}
                                Mode::CallbackFailure => {
                                    return Err(tembra::Error::invalid(
                                        "fixture.rollback",
                                        "preparation refused",
                                    ));
                                }
                                Mode::ConflictingAppend => {
                                    let _: WriteReceipt = conversations
                                        .call(
                                            &writing,
                                            Method::Append.as_str(),
                                            (
                                                input.head,
                                                vec![EntryDraft::encode("conflict").unwrap()],
                                            ),
                                        )
                                        .await?;
                                }
                            }
                            Ok(serde_json::to_value(receipt.head).unwrap())
                        }
                        .boxed_local()
                    })
                    .await?;
                Ok(tasks::TaskOutcome::Completed(
                    serde_json::json!({"confirmed":true}),
                ))
            },
        ),
    )
}

fn invoke<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: &str,
    args: A,
) -> R {
    let reference = reference.clone();
    let method = method.to_owned();
    pool.run_until(host.root(principal, move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    }))
    .unwrap()
}

fn exercise(mode: Mode) -> (Option<serde_json::Value>, Vec<Entry>, tasks::TaskStatus) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("joint.sqlite");
    let owner = SqliteOwner::new(file.clone());
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![history::orb(owner.conversation_factory())])
        .unwrap();
    let conversations = Rc::new(
        host.get(history::SERVICE, vec![history::INSTANCE_KEY.into()], "host")
            .unwrap(),
    );
    let id = ConversationId::new();
    let created: WriteReceipt = invoke(
        &mut pool,
        &host,
        &conversations,
        "alice",
        Method::Create.as_str(),
        (id,),
    );
    host.install(vec![
        tasks::orb(
            host.clone(),
            owner.task_factory(),
            tasks::Definitions::new([definition()]).unwrap(),
            tasks::Options::new(
                pool.spawner(),
                NonZeroU64::new(60_000).unwrap(),
                Rc::new(|duration| {
                    async move { futures_timer::Delay::new(duration).await }.boxed_local()
                }),
            ),
        ),
        task_orb(conversations.clone()),
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
    let entry = EntryDraft::encode("tool result").unwrap();
    let entry_id = entry.id;
    let spec = tasks::TaskSpec {
        id: tasks::TaskId::new(),
        definition: definition(),
        input: serde_json::to_value(Input {
            head: created.head,
            entry,
            mode,
        })
        .unwrap(),
    };
    let task_id = spec.id;
    let _: tasks::TaskId = invoke(
        &mut pool,
        &host,
        &scheduler,
        "alice",
        tasks::Method::Create.as_str(),
        (spec,),
    );
    invoke::<_, ()>(
        &mut pool,
        &host,
        &scheduler,
        "host",
        tasks::Method::Resume.as_str(),
        (),
    );
    pool.run_until_stalled();
    let record: Option<tasks::TaskRecord> = invoke(
        &mut pool,
        &host,
        &scheduler,
        "alice",
        tasks::Method::Get.as_str(),
        (task_id,),
    );
    let record = record.unwrap();
    assert!(record.assignment.is_none(), "physical assignment settled");
    let conversation: Option<Conversation> = invoke(
        &mut pool,
        &host,
        &conversations,
        "alice",
        Method::Get.as_str(),
        (id,),
    );
    let head = conversation.unwrap().head;
    if matches!(mode, Mode::Commit) {
        assert_eq!(head.entry, Some(entry_id))
    } else {
        assert_eq!(head, created.head)
    }
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().calls, 0);
    assert_eq!(host.kernel().diagnostics().resources, 0);
    assert!(owner.authority().check().is_err());
    // Read authoritative state through a new owner, not cached execution values.
    let connection = rusqlite::Connection::open(&file).unwrap();
    let physical_entries: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM swarmie_conversation_entries",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        physical_entries,
        if matches!(mode, Mode::Commit) { 1 } else { 0 },
        "a rolled-back head must not conceal orphan entries"
    );
    let reopened = SqliteOwner::new(file);
    let stored_tasks = reopened.tasks().unwrap();
    let stored_history = reopened.conversations().unwrap();
    let stored_task = pool.run_until(stored_tasks.get(task_id)).unwrap().unwrap();
    let page = pool
        .run_until(stored_history.read("alice", Read::new(head)))
        .unwrap();
    assert_eq!(stored_task.checkpoint, record.checkpoint);
    assert_eq!(stored_task.status, record.status);
    pool.run_until(stored_history.close()).unwrap();
    pool.run_until(stored_tasks.close()).unwrap();
    (stored_task.checkpoint, page.entries, stored_task.status)
}

#[test]
fn managed_conversation_append_and_execution_checkpoint_survive_together() {
    let (checkpoint, entries, status) = exercise(Mode::Commit);
    assert!(matches!(status, tasks::TaskStatus::Completed(_)));
    assert_eq!(entries.len(), 1);
    let head: Head = serde_json::from_value(checkpoint.unwrap()).unwrap();
    assert_eq!(head.entry, Some(entries[0].id));
}

#[test]
fn preparation_failure_discards_history_and_checkpoint() {
    let (checkpoint, entries, status) = exercise(Mode::CallbackFailure);
    assert!(matches!(status, tasks::TaskStatus::Failed(_)));
    assert!(checkpoint.is_none());
    assert!(entries.is_empty());
}

#[test]
fn conflicting_domain_write_rolls_back_prior_append_and_checkpoint() {
    let (checkpoint, entries, status) = exercise(Mode::ConflictingAppend);
    assert!(matches!(status, tasks::TaskStatus::Failed(_)));
    assert!(checkpoint.is_none());
    assert!(entries.is_empty());
}
