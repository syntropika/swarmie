use futures::{FutureExt, executor::LocalPool};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU16, rc::Rc};
use swarmie_conversations::*;
use swarmie_storage_sqlite::{SqliteConversationStorage, SqliteOwner};
use tembra::{Code, Host};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Fact {
    Message(String),
    ToolResult(u32),
}

fn draft(text: &str) -> EntryDraft {
    EntryDraft::encode(Fact::Message(text.into())).unwrap()
}

fn write(
    pool: &mut LocalPool,
    host: &Host,
    storage: Rc<SqliteConversationStorage>,
    principal: &str,
    value: Write,
) -> tembra::Result<WriteConfirmation> {
    pool.run_until(host.root(principal, move |ctx| {
        async move { storage.write(&ctx, value).await }.boxed_local()
    }))
}

fn create(pool: &mut LocalPool, host: &Host, storage: &Rc<SqliteConversationStorage>) -> Head {
    let id = ConversationId::new();
    assert_eq!(
        write(
            pool,
            host,
            storage.clone(),
            "alice",
            Write::Create { id, origin: None }
        )
        .unwrap(),
        WriteConfirmation::Committed
    );
    Head {
        conversation: id,
        entry: None,
    }
}

fn append(
    pool: &mut LocalPool,
    host: &Host,
    storage: &Rc<SqliteConversationStorage>,
    at: Head,
    entries: Vec<EntryDraft>,
) -> Head {
    let write_value = Write::Append { at, entries };
    let head = write_value.head().unwrap();
    assert_eq!(
        write(pool, host, storage.clone(), "alice", write_value).unwrap(),
        WriteConfirmation::Committed
    );
    head
}

fn fork(
    pool: &mut LocalPool,
    host: &Host,
    storage: &Rc<SqliteConversationStorage>,
    origin: HistoryPoint,
) -> Head {
    let value = Write::Create {
        id: ConversationId::new(),
        origin: Some(origin),
    };
    let head = value.head().unwrap();
    write(pool, host, storage.clone(), "alice", value).unwrap();
    head
}

fn facts(pool: &mut LocalPool, storage: &SqliteConversationStorage, at: Head) -> Vec<Fact> {
    pool.run_until(storage.read("alice", Read::new(at)))
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.decode().unwrap())
        .collect()
}

#[test]
fn nested_branches_reuse_fixed_ancestor_prefixes_across_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let storage = Rc::new(SqliteConversationStorage::open(&file).unwrap());
    let root = create(&mut pool, &host, &storage);
    let first = draft("one");
    let second = draft("two");
    let parent = append(
        &mut pool,
        &host,
        &storage,
        root,
        vec![first.clone(), second.clone()],
    );
    let branch = fork(
        &mut pool,
        &host,
        &storage,
        HistoryPoint {
            conversation: root.conversation,
            entry: first.id,
        },
    );
    let own = draft("branch");
    let branch = append(&mut pool, &host, &storage, branch, vec![own.clone()]);
    let grandchild = fork(
        &mut pool,
        &host,
        &storage,
        HistoryPoint {
            conversation: branch.conversation,
            entry: own.id,
        },
    );
    let grandchild = append(
        &mut pool,
        &host,
        &storage,
        grandchild,
        vec![draft("grandchild")],
    );
    let truncated = fork(
        &mut pool,
        &host,
        &storage,
        HistoryPoint {
            conversation: grandchild.conversation,
            entry: first.id,
        },
    );
    let parent = append(
        &mut pool,
        &host,
        &storage,
        parent,
        vec![draft("later parent")],
    );
    let branch = append(
        &mut pool,
        &host,
        &storage,
        branch,
        vec![draft("later branch")],
    );
    assert_eq!(
        facts(&mut pool, &storage, grandchild),
        vec![
            Fact::Message("one".into()),
            Fact::Message("branch".into()),
            Fact::Message("grandchild".into())
        ]
    );
    assert_eq!(
        facts(&mut pool, &storage, truncated),
        vec![Fact::Message("one".into())]
    );
    assert_eq!(facts(&mut pool, &storage, parent).len(), 3);
    assert_eq!(facts(&mut pool, &storage, branch).len(), 3);
    let connection = rusqlite::Connection::open(&file).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM swarmie_conversation_entries",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        6,
        "forks do not copy prefix payloads"
    );
    pool.run_until(storage.close()).unwrap();
    let reopened = SqliteConversationStorage::open(&file).unwrap();
    assert_eq!(facts(&mut pool, &reopened, grandchild).len(), 3);
    let page = pool
        .run_until(reopened.read("alice", Read::new(grandchild)))
        .unwrap();
    assert_eq!(page.entries[0].conversation, root.conversation);
    assert_eq!(page.entries[1].conversation, branch.conversation);
    pool.run_until(reopened.close()).unwrap();
    pool.run_until(host.close()).unwrap();
}

#[test]
fn pages_keep_the_selected_head_and_reject_foreign_or_invisible_cursors() {
    let directory = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let storage =
        Rc::new(SqliteConversationStorage::open(directory.path().join("history.sqlite")).unwrap());
    let root = create(&mut pool, &host, &storage);
    let one = draft("one");
    let two = draft("two");
    let excluded = draft("excluded");
    let parent = append(
        &mut pool,
        &host,
        &storage,
        root,
        vec![one.clone(), two.clone(), excluded.clone()],
    );
    let branch = fork(
        &mut pool,
        &host,
        &storage,
        HistoryPoint {
            conversation: root.conversation,
            entry: two.id,
        },
    );
    let three = draft("three");
    let four = draft("four");
    let selected = append(
        &mut pool,
        &host,
        &storage,
        branch,
        vec![three.clone(), four.clone()],
    );
    append(
        &mut pool,
        &host,
        &storage,
        selected,
        vec![draft("after snapshot")],
    );
    let mut request = Read {
        at: selected,
        after: None,
        limit: NonZeroU16::new(1).unwrap(),
    };
    let mut ids = Vec::new();
    loop {
        let page = pool
            .run_until(storage.read("alice", request.clone()))
            .unwrap();
        ids.extend(page.entries.iter().map(|entry| entry.id));
        let Some(next) = page.next else { break };
        request.after = Some(next);
    }
    assert_eq!(ids, vec![one.id, two.id, three.id, four.id]);
    for after in [excluded.id, EntryId::new()] {
        assert!(
            pool.run_until(storage.read(
                "alice",
                Read {
                    at: selected,
                    after: Some(after),
                    limit: Read::MAX_LIMIT
                }
            ))
            .is_err()
        );
    }
    assert!(
        pool.run_until(storage.read(
            "alice",
            Read::new(Head {
                conversation: branch.conversation,
                entry: parent.entry
            })
        ))
        .is_err()
    );
    assert!(
        pool.run_until(storage.read(
            "alice",
            Read::new(Head {
                conversation: branch.conversation,
                entry: None
            })
        ))
        .is_err()
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn stale_heads_and_duplicate_entries_roll_back_the_whole_append() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let storage = Rc::new(SqliteConversationStorage::open(&file).unwrap());
    let empty = create(&mut pool, &host, &storage);
    let original = draft("original");
    let current = append(&mut pool, &host, &storage, empty, vec![original.clone()]);
    let stale = write(
        &mut pool,
        &host,
        storage.clone(),
        "alice",
        Write::Append {
            at: empty,
            entries: vec![draft("stale")],
        },
    )
    .unwrap_err();
    assert_eq!(stale.code, Code::Conflict);
    let failed = write(
        &mut pool,
        &host,
        storage.clone(),
        "alice",
        Write::Append {
            at: current,
            entries: vec![draft("rolled back"), original],
        },
    )
    .unwrap_err();
    assert_eq!(failed.code, Code::Conflict);
    assert_eq!(
        pool.run_until(storage.get("alice", empty.conversation))
            .unwrap()
            .unwrap()
            .head,
        current
    );
    assert_eq!(
        facts(&mut pool, &storage, current),
        vec![Fact::Message("original".into())]
    );
    let same = draft("duplicate");
    assert!(
        write(
            &mut pool,
            &host,
            storage.clone(),
            "alice",
            Write::Append {
                at: current,
                entries: vec![same.clone(), same]
            }
        )
        .is_err()
    );
    assert!(
        write(
            &mut pool,
            &host,
            storage.clone(),
            "alice",
            Write::Append {
                at: current,
                entries: vec![]
            }
        )
        .is_err()
    );
    let connection = rusqlite::Connection::open(file).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM swarmie_conversation_entries",
                [],
                |row| row.get::<_, u32>(0)
            )
            .unwrap(),
        1,
        "no orphan entries may escape rollback"
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn every_history_access_and_fork_checks_the_invoking_principal() {
    let directory = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let storage =
        Rc::new(SqliteConversationStorage::open(directory.path().join("history.sqlite")).unwrap());
    let root = create(&mut pool, &host, &storage);
    let head = append(&mut pool, &host, &storage, root, vec![draft("private")]);
    for principal in ["bob", "host"] {
        assert!(matches!(
            pool.run_until(storage.get(principal, root.conversation)),
            Err(StorageError::Refused(Rejection::Forbidden))
        ));
        assert!(
            pool.run_until(storage.read(principal, Read::new(head)))
                .is_err()
        );
        let error = write(
            &mut pool,
            &host,
            storage.clone(),
            principal,
            Write::Append {
                at: head,
                entries: vec![draft("intruder")],
            },
        )
        .unwrap_err();
        assert_eq!(error.code, Code::Forbidden);
        let error = write(
            &mut pool,
            &host,
            storage.clone(),
            principal,
            Write::Create {
                id: ConversationId::new(),
                origin: Some(HistoryPoint {
                    conversation: root.conversation,
                    entry: head.entry.unwrap(),
                }),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, Code::Forbidden);
    }
    assert_eq!(facts(&mut pool, &storage, head).len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn prepared_writes_never_fall_back_after_scope_close_foreign_owner_or_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let owner = SqliteOwner::new(directory.path().join("history.sqlite"));
    let storage = Rc::new(owner.conversations().unwrap());
    let root = create(&mut pool, &host, &storage);
    let checked = storage.clone();
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            let scope = owner.authority().prepare(&ctx)?;
            let derived = scope.context().clone();
            assert_eq!(
                checked
                    .write(
                        &derived,
                        Write::Append {
                            at: root,
                            entries: vec![draft("aborted")]
                        }
                    )
                    .await?,
                WriteConfirmation::Staged
            );
            assert_eq!(
                checked.get("alice", root.conversation).await?.unwrap().head,
                root,
                "staged writes are invisible"
            );
            drop(scope);
            assert_eq!(
                checked
                    .write(
                        &derived,
                        Write::Append {
                            at: root,
                            entries: vec![draft("late")]
                        }
                    )
                    .await
                    .unwrap_err()
                    .code,
                Code::Forbidden
            );
            let foreign = swarmie_storage::StorageOwner::default();
            let scope = foreign.prepare(&ctx)?;
            assert_eq!(
                checked
                    .write(
                        scope.context(),
                        Write::Append {
                            at: root,
                            entries: vec![draft("foreign")]
                        }
                    )
                    .await
                    .unwrap_err()
                    .code,
                Code::Forbidden
            );
            let scope = owner.authority().prepare(&ctx)?;
            ctx.cancel()?;
            assert_eq!(
                checked
                    .write(
                        scope.context(),
                        Write::Append {
                            at: root,
                            entries: vec![draft("cancelled")]
                        }
                    )
                    .await
                    .unwrap_err()
                    .code,
                Code::Cancelled
            );
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    assert!(facts(&mut pool, &storage, root).is_empty());
    assert_eq!(
        pool.run_until(storage.get("alice", root.conversation))
            .unwrap()
            .unwrap()
            .head,
        root
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn independent_leases_share_health_but_close_only_their_own_capability() {
    use swarmie_durable_tasks::TaskStorage;
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(directory.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let first = Rc::new(owner.conversations().unwrap());
    let second = Rc::new(owner.conversations().unwrap());
    let tasks = owner.tasks().unwrap();
    let head = create(&mut pool, &host, &first);
    pool.run_until(first.close()).unwrap();
    assert!(matches!(
        pool.run_until(first.get("alice", head.conversation)),
        Err(StorageError::Owner(swarmie_storage::OwnerError::Closed))
    ));
    assert!(
        pool.run_until(second.get("alice", head.conversation))
            .unwrap()
            .is_some()
    );
    assert!(
        pool.run_until(tasks.get(swarmie_durable_tasks::TaskId::new()))
            .is_ok()
    );
    owner.authority().require_recovery();
    assert!(
        pool.run_until(second.read("alice", Read::new(head)))
            .is_err()
    );
    assert!(
        write(
            &mut pool,
            &host,
            second.clone(),
            "alice",
            Write::Create {
                id: ConversationId::new(),
                origin: None
            }
        )
        .is_err()
    );
    pool.run_until(second.close()).unwrap();
    pool.run_until(tasks.close()).unwrap();
    assert!(owner.authority().check().is_err());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn ordinary_orb_works_without_a_manager_or_agent_and_releases_its_lease() {
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(directory.path().join("history.sqlite"));
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![orb(owner.conversation_factory())])
        .unwrap();
    let reference = Rc::new(
        host.get(SERVICE, vec![INSTANCE_KEY.into()], "host")
            .unwrap(),
    );
    let id = ConversationId::new();
    let use_reference = reference.clone();
    let readback = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                let created: WriteReceipt = use_reference
                    .call(&ctx, Method::Create.as_str(), (id,))
                    .await?;
                assert_eq!(created.confirmation, WriteConfirmation::Committed);
                let fact = EntryDraft::encode(Fact::ToolResult(7)).unwrap();
                let appended: WriteReceipt = use_reference
                    .call(&ctx, Method::Append.as_str(), (created.head, vec![fact]))
                    .await?;
                let page: Page = use_reference
                    .call(&ctx, Method::Read.as_str(), Read::new(appended.head))
                    .await?;
                page.entries[0].decode::<Fact>().map_err(Into::into)
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(readback, Fact::ToolResult(7));
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().calls, 0);
    assert_eq!(host.kernel().diagnostics().resources, 0);
    assert!(matches!(
        owner.authority().check(),
        Err(swarmie_storage::OwnerError::Closed)
    ));
}

#[test]
fn page_limits_apply_to_large_prefixes_and_invalid_lineage_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("history.sqlite");
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let storage = Rc::new(SqliteConversationStorage::open(&file).unwrap());
    let root = create(&mut pool, &host, &storage);
    let values: Vec<_> = (0..1_000)
        .map(|number| EntryDraft::encode(Fact::ToolResult(number)).unwrap())
        .collect();
    let last = values.last().unwrap().id;
    let root = append(&mut pool, &host, &storage, root, values);
    let branch = fork(
        &mut pool,
        &host,
        &storage,
        HistoryPoint {
            conversation: root.conversation,
            entry: last,
        },
    );
    let page = pool
        .run_until(storage.read(
            "alice",
            Read {
                at: branch,
                after: None,
                limit: NonZeroU16::new(u16::MAX).unwrap(),
            },
        ))
        .unwrap();
    assert_eq!(page.entries.len(), usize::from(Read::MAX_LIMIT.get()));
    assert_eq!(page.next, page.entries.last().map(|entry| entry.id));
    let connection = rusqlite::Connection::open(file).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM swarmie_conversation_entries WHERE conversation=?1",
                [branch.conversation.to_string()],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute(
            "UPDATE swarmie_conversations SET parent=?1,parent_entry=?2 WHERE id=?3",
            rusqlite::params![
                branch.conversation.to_string(),
                last.to_string(),
                root.conversation.to_string()
            ],
        )
        .unwrap();
    assert!(matches!(
        pool.run_until(storage.read("alice", Read::new(branch))),
        Err(StorageError::Invalid(_))
    ));
    pool.run_until(host.close()).unwrap();
}
