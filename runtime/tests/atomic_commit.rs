use futures::{FutureExt, channel::oneshot, executor::LocalPool, future::LocalBoxFuture};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    num::{NonZeroU32, NonZeroU64},
    rc::Rc,
};
use swarmie_atomic_journal as journal;
use swarmie_durable_tasks::{self as tasks, *};
use swarmie_storage::sqlite::{SqliteOwner, SqliteTaskStorage, SqliteWrite};
use swarmie_storage::{ChangeSource, StorageOwner};
use tembra::{Code, Context, Host, Orb, Reference, Service};

struct Journal(SqliteOwner, ChangeSource);
impl journal::Journal for Journal {
    fn append(&self, context: Context, entry: String) -> LocalBoxFuture<'_, tembra::Result<()>> {
        async move {
            self.0.stage(
                &context,
                SqliteWrite::new(move |tx| {
                    tx.execute("INSERT INTO journal(entry) VALUES(?1)", [entry])
                        .map_err(|e| {
                            tembra::Error::new(
                                Code::Conflict,
                                "fixture.journal.write",
                                e.to_string(),
                            )
                        })?;
                    Ok(())
                })
                .notifies(self.1.clone()),
            )
        }
        .boxed_local()
    }
}

/// Fault injection wraps a real SQLite joint transaction; it never emulates its writes.
struct Adapter {
    inner: SqliteTaskStorage,
    unknown: Cell<bool>,
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    entered: Cell<bool>,
    closed: Cell<bool>,
}
impl TaskStorage for Adapter {
    fn owner(&self) -> StorageOwner {
        self.inner.owner()
    }
    fn changes(&self) -> ChangeSource {
        self.inner.changes()
    }
    fn get(&self, id: TaskId) -> LocalBoxFuture<'_, Result<Option<TaskRecord>, StorageError>> {
        self.inner.get(id)
    }
    fn list(
        &self,
        p: Option<String>,
        page: TaskPageRequest,
    ) -> LocalBoxFuture<'_, Result<Vec<TaskSummary>, StorageError>> {
        self.inner.list(p, page)
    }
    fn candidates(
        &self,
        after: u64,
        limit: usize,
    ) -> LocalBoxFuture<'_, Result<CandidateSnapshot, StorageError>> {
        self.inner.candidates(after, limit)
    }
    fn commit(
        &self,
        changes: Vec<TaskChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        self.inner.commit(changes)
    }
    fn commit_with(
        &self,
        changes: Vec<TaskChange>,
        prepared: swarmie_storage::PreparedChanges,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            self.entered.set(true);
            let gate = self.gate.borrow_mut().take();
            if let Some(gate) = gate {
                gate.await.unwrap();
            }
            assert!(!self.closed.get());
            let outcome = self.inner.commit_with(changes, prepared).await?;
            if matches!(outcome, CommitOutcome::Applied { .. }) && self.unknown.replace(false) {
                return Ok(CommitOutcome::Unknown {
                    message: "injected lost acknowledgement".into(),
                });
            }
            Ok(outcome)
        }
        .boxed_local()
    }
    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move {
            self.inner.close().await?;
            self.closed.set(true);
            Ok(())
        }
        .boxed_local()
    }
}

type Plan =
    Rc<dyn Fn(Context, Execution, Rc<Reference>) -> LocalBoxFuture<'static, tembra::Result<Value>>>;
struct Harness {
    pool: LocalPool,
    host: Host,
    tasks: Rc<Reference>,
    storage: Rc<Adapter>,
    connection: Rc<rusqlite::Connection>,
    notices: Rc<Cell<usize>>,
    _subscription: swarmie_storage::Subscription,
    _dir: tempfile::TempDir,
}
impl Harness {
    fn new(plan: Plan) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("atomic.sqlite");
        let owner = SqliteOwner::new(file.clone());
        let storage = Rc::new(Adapter {
            inner: owner.tasks().unwrap(),
            unknown: Cell::new(false),
            gate: RefCell::new(None),
            entered: Cell::new(false),
            closed: Cell::new(false),
        });
        let connection = Rc::new(rusqlite::Connection::open(file).unwrap());
        connection
            .execute_batch("CREATE TABLE journal(entry TEXT PRIMARY KEY);")
            .unwrap();
        let changes = ChangeSource::default();
        let notices = Rc::new(Cell::new(0));
        let (read, count) = (connection.clone(), notices.clone());
        let subscription = changes.subscribe(move || {
            assert!(
                read.query_row("SELECT COUNT(*) FROM journal", [], |r| r.get::<_, u64>(0))
                    .unwrap()
                    > 0
            );
            let confirmed: usize = read.query_row(
                "SELECT COUNT(*) FROM swarmie_tasks WHERE json_extract(progress,'$.checkpoint')=7", [], |r| r.get(0),
            ).unwrap();
            assert_eq!(confirmed, 1, "notification observes both domain data and checkpoint");
            count.set(count.get() + 1);
        });
        let pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let factory = {
            let storage = storage.clone();
            Rc::new(move || {
                let storage = storage.clone();
                async move { Ok(storage as Rc<dyn TaskStorage>) }.boxed_local()
            })
        };
        let handler = {
            let host = host.clone();
            Orb::new("fixture.atomic_task", 1).service(
                Service::new("fixture.atomic_task", move |_| {
                    Ok((host.clone(), plan.clone()))
                })
                .method(
                    TaskMethod::Execute.as_str(),
                    |env, ctx, execution: Execution| async move {
                        let journal = Rc::new(env.0.get(
                            journal::SERVICE,
                            vec!["main".into()],
                            &ctx.principal()?,
                        )?);
                        env.1(ctx, execution, journal)
                            .await
                            .map(tasks::TaskOutcome::Completed)
                    },
                ),
            )
        };
        host.install(vec![
            tasks::orb(
                host.clone(),
                factory,
                Definitions::new([definition()]).unwrap(),
                Options::new(
                    pool.spawner(),
                    NonZeroU64::new(30_000).unwrap(),
                    Rc::new(|_| futures::future::pending().boxed_local()),
                ),
            ),
            handler,
            journal::orb(Rc::new(Journal(owner, changes))),
        ])
        .unwrap();
        let tasks = Rc::new(
            host.get(SERVICE, vec![INSTANCE_KEY.into()], HOST_PRINCIPAL)
                .unwrap(),
        );
        Self {
            pool,
            host,
            tasks,
            storage,
            connection,
            notices,
            _subscription: subscription,
            _dir: dir,
        }
    }
    fn call<A: 'static, R: 'static>(&mut self, method: Method, args: A) -> tembra::Result<R> {
        let tasks = self.tasks.clone();
        self.pool
            .run_until(self.host.root(HOST_PRINCIPAL, move |ctx| {
                async move { tasks.call(&ctx, method.as_str(), args).await }.boxed_local()
            }))
    }
    fn start(&mut self) -> TaskId {
        let id = self
            .call(
                Method::Create,
                (TaskSpec {
                    id: TaskId::new(),
                    definition: definition(),
                    input: Value::Null,
                },),
            )
            .unwrap();
        self.call::<_, ()>(Method::Resume, ()).unwrap();
        self.pool.run_until_stalled();
        id
    }
    fn entries(&self) -> usize {
        self.connection
            .query_row("SELECT COUNT(*) FROM journal", [], |r| r.get(0))
            .unwrap()
    }
    fn record(&mut self, id: TaskId) -> TaskRecord {
        self.pool.run_until(self.storage.get(id)).unwrap().unwrap()
    }
    fn close(&mut self) {
        self.pool.run_until(self.host.close()).unwrap();
        assert!(self.storage.closed.get());
    }
}
fn definition() -> TaskDefinition {
    TaskDefinition::new("fixture.atomic_task", NonZeroU32::new(1).unwrap()).unwrap()
}
async fn append(journal: &Reference, ctx: &Context, entry: &str) -> tembra::Result<()> {
    journal
        .call(ctx, journal::Method::Append.as_str(), (entry.to_owned(),))
        .await
}

#[test]
fn ordinary_domain_orb_and_checkpoint_confirm_in_one_real_transaction() {
    let observed = Rc::new(RefCell::new(None));
    let capture = observed.clone();
    let mut h = Harness::new(Rc::new(move |ctx, e, journal| {
        let capture = capture.clone();
        async move {
            e.commit_with(&ctx, move |ctx| {
                async move {
                    append(&journal, &ctx, "hello").await?;
                    Ok(7.into())
                }
                .boxed_local()
            })
            .await?;
            *capture.borrow_mut() = e.checkpoint();
            Ok(Value::Null)
        }
        .boxed_local()
    }));
    let id = h.start();
    assert_eq!(h.entries(), 1);
    assert_eq!(*observed.borrow(), Some(7.into()));
    assert_eq!(h.record(id).checkpoint, Some(7.into()));
    assert_eq!(h.notices.get(), 1);
    h.close();
}

#[test]
fn domain_constraint_failure_rolls_back_earlier_writes_and_checkpoint() {
    let observed = Rc::new(RefCell::new(None));
    let capture = observed.clone();
    let mut h = Harness::new(Rc::new(move |ctx, e, journal| {
        let capture = capture.clone();
        async move {
            let error = e
                .commit_with(&ctx, move |ctx| {
                    async move {
                        append(&journal, &ctx, "duplicate").await?;
                        append(&journal, &ctx, "duplicate").await?;
                        Ok(7.into())
                    }
                    .boxed_local()
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, Code::Conflict);
            assert_eq!(error.reason, "fixture.journal.write");
            *capture.borrow_mut() = Some(e.checkpoint());
            Ok(Value::Null)
        }
        .boxed_local()
    }));
    let id = h.start();
    assert_eq!(h.entries(), 0);
    assert_eq!(*observed.borrow(), Some(None));
    assert_eq!(h.record(id).checkpoint, None);
    assert_eq!(h.notices.get(), 0);
    assert!(h.storage.owner().check().is_ok());
    h.close();
}

#[test]
fn callback_error_discards_staging_without_entering_backend_commit() {
    let mut h = Harness::new(Rc::new(|ctx, e, journal| {
        async move {
            let error = e
                .commit_with(&ctx, move |ctx| {
                    async move {
                        append(&journal, &ctx, "discard").await?;
                        Err(tembra::Error::new(
                            Code::Forbidden,
                            "fixture.refused",
                            "injected refusal",
                        ))
                    }
                    .boxed_local()
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, Code::Forbidden);
            assert_eq!(e.checkpoint(), None);
            Ok(Value::Null)
        }
        .boxed_local()
    }));
    let id = h.start();
    assert!(!h.storage.entered.get());
    assert_eq!(h.entries(), 0);
    assert_eq!(h.record(id).checkpoint, None);
    h.close();
}

#[test]
fn unknown_joint_acknowledgement_freezes_owner_without_adopting_progress() {
    let seen = Rc::new(Cell::new(false));
    let capture = seen.clone();
    let mut h = Harness::new(Rc::new(move |ctx, e, journal| {
        let seen = capture.clone();
        async move {
            let error = e
                .commit_with(&ctx, move |ctx| {
                    async move {
                        append(&journal, &ctx, "uncertain").await?;
                        Ok(7.into())
                    }
                    .boxed_local()
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, Code::Unavailable);
            assert_eq!(e.checkpoint(), None);
            assert!(e.commit(&ctx, 8.into()).await.is_err());
            seen.set(true);
            Ok(Value::Null)
        }
        .boxed_local()
    }));
    h.storage.unknown.set(true);
    let id = h.start();
    assert!(seen.get());
    assert_eq!(h.entries(), 1);
    assert!(h.storage.owner().check().is_err());
    // Recovery uses a fresh authoritative connection, not the frozen capability.
    let progress: String = h
        .connection
        .query_row(
            "SELECT progress FROM swarmie_tasks WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&progress).unwrap()["checkpoint"],
        7
    );
    h.close();
}

#[test]
fn dropping_joint_waiter_does_not_release_storage_before_physical_commit() {
    let mut h = Harness::new(Rc::new(|ctx, e, journal| {
        async move {
            let mut write = Box::pin(e.commit_with(&ctx, move |ctx| {
                async move {
                    append(&journal, &ctx, "settled").await?;
                    Ok(7.into())
                }
                .boxed_local()
            }));
            assert!(futures::poll!(&mut write).is_pending());
            drop(write);
            Ok(Value::Null)
        }
        .boxed_local()
    }));
    let (send, receive) = oneshot::channel();
    *h.storage.gate.borrow_mut() = Some(receive);
    let id = h.start();
    assert!(h.storage.entered.get());
    assert_eq!(h.entries(), 0);
    let mut close = Box::pin(h.host.close());
    h.pool.run_until(async {
        assert!(futures::poll!(&mut close).is_pending());
    });
    assert!(!h.storage.closed.get());
    send.send(()).unwrap();
    h.pool.run_until(close).unwrap();
    assert!(h.storage.closed.get());
    assert_eq!(h.entries(), 1);
    let progress: String = h
        .connection
        .query_row(
            "SELECT progress FROM swarmie_tasks WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&progress).unwrap()["checkpoint"],
        7
    );
}

#[test]
fn changed_revision_expired_lease_and_successor_attempt_refuse_domain_commit() {
    #[derive(Clone, Copy)]
    enum Loss {
        Revision,
        Expiry,
        Successor,
    }
    for loss in [Loss::Revision, Loss::Expiry, Loss::Successor] {
        let connection = Rc::new(RefCell::new(None::<Rc<rusqlite::Connection>>));
        let capture = connection.clone();
        let refused = Rc::new(Cell::new(false));
        let seen = refused.clone();
        let mut h = Harness::new(Rc::new(move |ctx, e, journal| {
            let connection = capture.borrow().as_ref().unwrap().clone();
            let seen = seen.clone();
            async move {
                let id = e.id();
                let error = e.commit_with(&ctx, move |ctx| async move {
                    append(&journal, &ctx, "stale").await?;
                    match loss {
                        Loss::Revision => { connection.execute("UPDATE swarmie_tasks SET revision=1,progress=json_set(progress,'$.revision',1) WHERE id=?1", [id.to_string()]).unwrap(); }
                        Loss::Expiry => { connection.execute("UPDATE swarmie_tasks SET expires_at=0,assignment=json_set(assignment,'$.expires_at',0) WHERE id=?1", [id.to_string()]).unwrap(); }
                        Loss::Successor => { connection.execute("UPDATE swarmie_tasks SET assignment=json_set(assignment,'$.attempt',?2) WHERE id=?1", rusqlite::params![id.to_string(), AttemptId::new().to_string()]).unwrap(); }
                    }
                    Ok(7.into())
                }.boxed_local()).await.unwrap_err();
                assert_eq!(error.code, Code::Conflict);
                assert_eq!(e.checkpoint(), None);
                seen.set(true);
                Ok(Value::Null)
            }.boxed_local()
        }));
        *connection.borrow_mut() = Some(h.connection.clone());
        h.start();
        assert!(refused.get());
        assert_eq!(h.entries(), 0);
        assert_eq!(h.notices.get(), 0);
        assert!(h.storage.owner().check().is_ok());
        h.close();
    }
}

#[test]
fn cancellation_after_preparation_aborts_before_backend_admission() {
    let mut h = Harness::new(Rc::new(|ctx, e, journal| {
        async move {
            e.commit_with(&ctx, move |ctx| {
                async move {
                    append(&journal, &ctx, "cancelled").await?;
                    ctx.cancel()?;
                    Ok(7.into())
                }
                .boxed_local()
            })
            .await?;
            Ok(Value::Null)
        }
        .boxed_local()
    }));
    let id = h.start();
    assert!(!h.storage.entered.get());
    assert_eq!(h.entries(), 0);
    assert_eq!(h.record(id).checkpoint, None);
    assert_eq!(h.notices.get(), 0);
    h.close();
}

#[test]
fn sequential_joint_commits_reclaim_frames_while_the_execution_remains_live() {
    const COMMITS: usize = 4_100;
    let kernel = Rc::new(RefCell::new(None::<tembra::core::Host>));
    let observed = kernel.clone();
    let mut h = Harness::new(Rc::new(move |ctx, execution, journal| {
        let kernel = observed.clone();
        async move {
            for index in 0..COMMITS {
                let journal = journal.clone();
                execution
                    .commit_with(&ctx, move |ctx| {
                        async move {
                            append(&journal, &ctx, &index.to_string()).await?;
                            Ok(7.into())
                        }
                        .boxed_local()
                    })
                    .await?;
                assert_eq!(execution.checkpoint(), Some(7.into()));
                assert_eq!(kernel.borrow().as_ref().unwrap().context_stats().0, 0);
            }
            Ok(COMMITS.into())
        }
        .boxed_local()
    }));
    *kernel.borrow_mut() = Some(h.host.kernel());
    let id = h.start();
    let record = h.record(id);
    assert_eq!(record.status, TaskStatus::Completed(COMMITS.into()));
    assert_eq!(record.revision, COMMITS as u64 + 1);
    assert_eq!(record.checkpoint, Some(7.into()));
    assert_eq!(h.entries(), COMMITS);
    assert_eq!(h.notices.get(), COMMITS);
    assert_eq!(h.host.kernel().context_stats().1, 1);
    h.close();
}
