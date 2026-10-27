use futures::{FutureExt, channel::oneshot, executor::LocalPool, future::LocalBoxFuture};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
    rc::Rc,
};
use swarmie_durable_tasks::*;
use swarmie_storage_core::{ChangeSource, StorageOwner};
use tembra::{Code, Host, Orb, Reference, Service};

#[derive(Default)]
struct Memory {
    records: RefCell<BTreeMap<TaskId, TaskRecord>>,
    sequence: Cell<u64>,
    receipts: RefCell<SignalReceipts>,
    now: Cell<u64>,
    closed: Cell<bool>,
    owner: StorageOwner,
    changes: ChangeSource,
    unknown: Cell<bool>,
    unavailable: Cell<bool>,
    checkpoint_calls: Cell<u32>,
    claim_calls: Cell<u32>,
    reject_claim: RefCell<Option<Rejection>>,
    bad_claim: Cell<Option<TaskId>>,
    unavailable_renew: Cell<bool>,
    renewed: Cell<u32>,
    candidate_reads: Cell<u32>,
    gate: RefCell<Option<oneshot::Receiver<()>>>,
}
impl TaskStorage for Memory {
    fn owner(&self) -> StorageOwner {
        self.owner.clone()
    }
    fn changes(&self) -> ChangeSource {
        self.changes.clone()
    }
    fn get(&self, id: TaskId) -> LocalBoxFuture<'_, Result<Option<TaskRecord>, StorageError>> {
        async move { Ok(self.records.borrow().get(&id).cloned()) }.boxed_local()
    }
    fn list(
        &self,
        principal: Option<String>,
        page: TaskPageRequest,
    ) -> LocalBoxFuture<'_, Result<Vec<TaskSummary>, StorageError>> {
        async move {
            let mut rows: Vec<_> = self
                .records
                .borrow()
                .values()
                .filter(|r| {
                    r.sequence > page.after && principal.as_ref().is_none_or(|p| p == &r.principal)
                })
                .map(TaskSummary::from)
                .collect();
            rows.sort_by_key(|r| r.sequence);
            rows.truncate(page.limit.get() as usize);
            Ok(rows)
        }
        .boxed_local()
    }
    fn candidates(
        &self,
        after: u64,
        limit: usize,
    ) -> LocalBoxFuture<'_, Result<CandidateSnapshot, StorageError>> {
        async move {
            self.candidate_reads.set(self.candidate_reads.get() + 1);
            let records = self.records.borrow();
            let mut candidates: Vec<_> = records
                .values()
                .filter(|r| {
                    r.sequence > after
                        && (r.status == TaskStatus::Ready || matches!(r.status, TaskStatus::Waiting(WaitState::Until(at)) if at <= self.now.get()))
                        && r.assignment
                            .as_ref()
                            .is_none_or(|a| a.expires_at <= self.now.get())
                })
                .map(TaskSummary::from)
                .collect();
            candidates.sort_by_key(|r| r.sequence);
            candidates.truncate(limit);
            Ok(CandidateSnapshot {
                now: self.now.get(),
                candidates,
                next_wake: records.values().filter_map(|r| match r.status { TaskStatus::Waiting(WaitState::Until(at)) => Some(at), _ => None }).min(),
            })
        }
        .boxed_local()
    }
    fn commit(
        &self,
        changes: Vec<TaskChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            assert!(!self.closed.get(), "storage must outlive physical writes");
            self.owner.check()?;
            let checkpoint = changes
                .iter()
                .any(|c| matches!(c, TaskChange::Checkpoint { .. }));
            let notify = changes.iter().any(|c| {
                matches!(
                    c,
                    TaskChange::Create { .. }
                        | TaskChange::Release { .. }
                        | TaskChange::Signal { .. }
                )
            });
            let claims: Vec<_> = changes
                .iter()
                .filter_map(|c| {
                    if let TaskChange::Acquire { id, .. } = c {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();
            if checkpoint {
                self.checkpoint_calls.set(self.checkpoint_calls.get() + 1);
                let gate = self.gate.borrow_mut().take();
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                if self.unavailable.replace(false) {
                    return Err(StorageError::Unavailable(
                        "injected transient outage".into(),
                    ));
                }
            }
            if !claims.is_empty() {
                self.claim_calls.set(self.claim_calls.get() + 1);
                if let Some(reason) = self.reject_claim.borrow().clone() {
                    assert!(
                        self.claim_calls.get() < 3,
                        "persistent refusal must not spin"
                    );
                    return Ok(CommitOutcome::Rejected(reason));
                }
                if claims.iter().any(|id| self.bad_claim.get() == Some(*id)) {
                    return Err(StorageError::Invalid("corrupt candidate payload".into()));
                }
            }
            if changes
                .iter()
                .any(|c| matches!(c, TaskChange::Renew { .. }))
            {
                if self.unavailable_renew.replace(false) {
                    return Err(StorageError::Unavailable("injected renewal outage".into()));
                }
                self.renewed.set(self.renewed.get() + 1);
            }
            let receipts = required_receipts(&changes)
                .into_iter()
                .map(|key| (key, self.receipts.borrow().get(&key).cloned()))
                .collect();
            let next = match apply_changes(
                &self.records.borrow(),
                &receipts,
                self.now.get(),
                self.sequence.get(),
                changes,
            ) {
                Ok(next) => next,
                Err(reason) => return Ok(CommitOutcome::Rejected(reason)),
            };
            let grants = claims
                .into_iter()
                .map(|id| AssignmentGrant {
                    id,
                    assignment: next.records[&id].assignment.clone().unwrap(),
                })
                .collect();
            self.receipts.borrow_mut().extend(next.signals);
            self.sequence.set(next.sequence);
            *self.records.borrow_mut() = next.records;
            if checkpoint && self.unknown.replace(false) {
                self.owner.require_recovery();
                return Ok(CommitOutcome::Unknown {
                    message: "injected acknowledgement loss".into(),
                });
            }
            if notify {
                self.changes.invalidate();
            }
            Ok(CommitOutcome::Applied { grants })
        }
        .boxed_local()
    }
    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move {
            assert!(self.gate.borrow().is_none());
            self.closed.set(true);
            self.owner.closed();
            Ok(())
        }
        .boxed_local()
    }
}
fn definition() -> TaskDefinition {
    TaskDefinition::new("fixture.task", NonZeroU32::new(1).unwrap()).unwrap()
}
fn simple() -> Orb {
    Orb::new("fixture.task", 1).service(Service::new("fixture.task", |_| Ok(())).method(
        TaskMethod::Execute.as_str(),
        |_, ctx, e: Execution| async move {
            e.check(&ctx)?;
            Ok(TaskOutcome::Completed(e.input()))
        },
    ))
}
struct Harness {
    pool: LocalPool,
    host: Host,
    tasks: Rc<Reference>,
    storage: Rc<Memory>,
}
impl Harness {
    fn new(storage: Rc<Memory>, handler: Orb, configure: impl FnOnce(&mut Options)) -> Self {
        let pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let mut options = Options::new(
            pool.spawner(),
            NonZeroU64::new(100).unwrap(),
            Rc::new(|_| futures::future::pending().boxed_local()),
        );
        configure(&mut options);
        let resource = storage.clone();
        let factory = Rc::new(move || {
            let s = resource.clone();
            async move { Ok(s as Rc<dyn TaskStorage>) }.boxed_local()
        });
        host.install(vec![
            orb(
                host.clone(),
                factory,
                Definitions::new([definition()]).unwrap(),
                options,
            ),
            handler,
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
        }
    }
    fn call<A: 'static, R: 'static>(&mut self, method: Method, args: A) -> tembra::Result<R> {
        let tasks = self.tasks.clone();
        self.pool
            .run_until(self.host.root(HOST_PRINCIPAL, move |ctx| {
                async move { tasks.call(&ctx, method.as_str(), args).await }.boxed_local()
            }))
    }
    fn create(&mut self, input: Value) -> TaskId {
        self.call(
            Method::Create,
            (TaskSpec {
                id: TaskId::new(),
                definition: definition(),
                input,
            },),
        )
        .unwrap()
    }
    fn resume(&mut self) {
        self.call::<_, ()>(Method::Resume, ()).unwrap();
        self.pool.run_until_stalled();
    }
    fn status(&mut self) -> SchedulerStatus {
        self.call(Method::Inspect, ()).unwrap()
    }
    fn close(&mut self) {
        self.pool.run_until(self.host.close()).unwrap();
        assert!(self.storage.closed.get());
    }
}
#[test]
fn standalone_is_paused_and_failure_isolated() {
    let handler =
        Orb::new("fixture.task", 1).service(Service::new("fixture.task", |_| Ok(())).method(
            TaskMethod::Execute.as_str(),
            |_, _, e: Execution| async move {
                if e.input() == false {
                    Err(tembra::Error::invalid(
                        "fixture.failure",
                        "injected failure",
                    ))
                } else {
                    Ok(TaskOutcome::Completed(e.input()))
                }
            },
        ));
    let mut h = Harness::new(Rc::default(), handler, |_| {});
    let bad = h.create(false.into());
    let good = h.create(true.into());
    h.pool.run_until_stalled();
    assert!(
        h.storage
            .records
            .borrow()
            .values()
            .all(|r| r.assignment.is_none() && r.status == TaskStatus::Ready)
    );
    h.resume();
    assert!(matches!(
        h.storage.records.borrow()[&bad].status,
        TaskStatus::Failed(_)
    ));
    assert_eq!(
        h.storage.records.borrow()[&good].status,
        TaskStatus::Completed(true.into())
    );
    h.close();
}
struct InvalidOrder;
impl SchedulerStrategy for InvalidOrder {
    fn order(&self, c: &[TaskSummary]) -> tembra::Result<Vec<TaskId>> {
        Ok(vec![c[0].id; c.len()])
    }
}
#[test]
fn invalid_strategy_never_acquires() {
    let mut h = Harness::new(Rc::default(), simple(), |o| {
        o.strategy = Rc::new(InvalidOrder)
    });
    h.create(1.into());
    h.create(2.into());
    h.resume();
    assert!(matches!(
        h.status().diagnostic,
        Some(SchedulerDiagnostic::Strategy(_))
    ));
    assert!(
        h.storage
            .records
            .borrow()
            .values()
            .all(|r| r.assignment.is_none())
    );
    h.close();
}
#[test]
fn missing_definition_is_reported_without_attempt() {
    let mut h = Harness::new(Rc::default(), Orb::new("fixture.absent", 1), |_| {});
    let id = h.create(1.into());
    h.resume();
    assert_eq!(h.status().blocked, vec![id]);
    assert!(h.storage.records.borrow()[&id].assignment.is_none());
    h.close();
}
#[test]
fn blocked_prefix_does_not_starve_available_work() {
    let mut h = Harness::new(Rc::default(), simple(), |_| {});
    for _ in 0..130 {
        let spec = TaskSpec {
            id: TaskId::new(),
            definition: TaskDefinition::new("fixture.absent", NonZeroU32::new(1).unwrap()).unwrap(),
            input: Value::Null,
        };
        h.call::<_, TaskId>(Method::Create, (spec,)).unwrap();
    }
    let id = h.create(1.into());
    h.resume();
    assert_eq!(
        h.storage.records.borrow()[&id].status,
        TaskStatus::Completed(1.into())
    );
    h.close();
}
#[test]
fn unknown_checkpoint_freezes_owner_and_does_not_adopt_unconfirmed_progress() {
    let storage = Rc::new(Memory::default());
    storage.unknown.set(true);
    let observed = Rc::new(RefCell::new(None));
    let capture = observed.clone();
    let handler = Orb::new("fixture.task", 1).service(
        Service::new("fixture.task", move |_| Ok(capture.clone())).method(
            TaskMethod::Execute.as_str(),
            |seen, ctx, e: Execution| async move {
                assert!(e.commit(&ctx, 40.into()).await.is_err());
                *seen.borrow_mut() = Some(e.checkpoint());
                assert!(e.commit(&ctx, 41.into()).await.is_err());
                Ok(TaskOutcome::Completed(Value::Null))
            },
        ),
    );
    let mut h = Harness::new(storage.clone(), handler, |_| {});
    let id = h.create(100.into());
    h.resume();
    assert_eq!(*observed.borrow(), Some(None));
    assert_eq!(storage.checkpoint_calls.get(), 1);
    assert_eq!(storage.records.borrow()[&id].checkpoint, Some(40.into()));
    assert_eq!(storage.records.borrow()[&id].status, TaskStatus::Ready);
    assert!(storage.owner.check().is_err());
    assert!(h.call::<_, ()>(Method::Wake, ()).is_err());
    h.close();
}
fn gated(
    early: bool,
    entered: Rc<RefCell<Vec<TaskId>>>,
    gates: Rc<RefCell<BTreeMap<TaskId, oneshot::Receiver<()>>>>,
) -> Orb {
    Orb::new("fixture.task", 1).service(
        Service::new("fixture.task", move |_| {
            Ok((entered.clone(), gates.clone()))
        })
        .method(
            TaskMethod::Execute.as_str(),
            move |env, ctx, e: Execution| async move {
                if early {
                    e.complete(&ctx, e.input()).await?;
                }
                env.0.borrow_mut().push(e.id());
                let gate = env.1.borrow_mut().remove(&e.id());
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                e.check(&ctx)
                    .or_else(|err| if early { Ok(()) } else { Err(err) })?;
                Ok(TaskOutcome::Completed(e.input()))
            },
        ),
    )
}
#[test]
fn early_complete_retains_slot_and_fifo_follows_creation_even_at_equal_time() {
    let entered = Rc::new(RefCell::new(vec![]));
    let gates = Rc::new(RefCell::new(BTreeMap::new()));
    let mut h = Harness::new(
        Rc::default(),
        gated(true, entered.clone(), gates.clone()),
        |o| o.max_executions = 1,
    );
    let first: TaskId = serde_json::from_str("\"ffffffff-ffff-4fff-8fff-ffffffffffff\"").unwrap();
    let second: TaskId = serde_json::from_str("\"00000000-0000-4000-8000-000000000001\"").unwrap();
    for (id, input) in [(first, 1), (second, 2)] {
        h.call::<_, TaskId>(
            Method::Create,
            (TaskSpec {
                id,
                definition: definition(),
                input: input.into(),
            },),
        )
        .unwrap();
    }
    let (tx, rx) = oneshot::channel();
    gates.borrow_mut().insert(first, rx);
    h.resume();
    assert_eq!(*entered.borrow(), vec![first]);
    assert!(h.storage.records.borrow()[&first].assignment.is_some());
    assert!(h.storage.records.borrow()[&second].assignment.is_none());
    tx.send(()).unwrap();
    h.pool.run_until_stalled();
    assert_eq!(*entered.borrow(), vec![first, second]);
    assert!(
        h.storage
            .records
            .borrow()
            .values()
            .all(|r| r.assignment.is_none())
    );
    h.close();
}
#[test]
fn resume_returns_promptly_is_idempotent_and_new_work_starts_while_another_is_active() {
    let entered = Rc::new(RefCell::new(vec![]));
    let gates = Rc::new(RefCell::new(BTreeMap::new()));
    let mut h = Harness::new(
        Rc::default(),
        gated(false, entered.clone(), gates.clone()),
        |_| {},
    );
    let first = h.create(1.into());
    let (tx, rx) = oneshot::channel();
    gates.borrow_mut().insert(first, rx);
    h.resume();
    assert_eq!(*entered.borrow(), vec![first]);
    for _ in 0..20 {
        h.call::<_, ()>(Method::Resume, ()).unwrap();
        h.call::<_, ()>(Method::Wake, ()).unwrap();
    }
    let second = h.create(2.into());
    h.pool.run_until_stalled();
    assert_eq!(*entered.borrow(), vec![first, second]);
    assert_eq!(h.status().active, 1);
    tx.send(()).unwrap();
    h.pool.run_until_stalled();
    h.close();
}
#[test]
fn shutdown_preserves_queued_work_instead_of_failing_dispatch() {
    let entered = Rc::new(RefCell::new(vec![]));
    let gates = Rc::new(RefCell::new(BTreeMap::new()));
    let mut h = Harness::new(
        Rc::default(),
        gated(false, entered.clone(), gates.clone()),
        |o| o.max_executions = 1,
    );
    let first = h.create(1.into());
    let second = h.create(2.into());
    let (tx, rx) = oneshot::channel();
    gates.borrow_mut().insert(first, rx);
    h.resume();
    let mut closing = Box::pin(h.host.close());
    h.pool.run_until(async {
        assert!(futures::poll!(&mut closing).is_pending());
    });
    h.pool.run_until_stalled();
    tx.send(()).unwrap();
    h.pool.run_until(closing).unwrap();
    assert_eq!(*entered.borrow(), vec![first]);
    let queued = h.storage.records.borrow()[&second].clone();
    assert_eq!(queued.status, TaskStatus::Ready);
    assert_eq!(queued.revision, 0);
    assert!(queued.assignment.is_none());
    assert!(h.storage.closed.get());
}
#[test]
fn dropped_write_waiter_is_joined_before_storage_close() {
    let storage = Rc::new(Memory::default());
    let (tx, rx) = oneshot::channel();
    *storage.gate.borrow_mut() = Some(rx);
    let handler =
        Orb::new("fixture.task", 1).service(Service::new("fixture.task", |_| Ok(())).method(
            TaskMethod::Execute.as_str(),
            |_, ctx, e: Execution| async move {
                let mut write = Box::pin(e.commit(&ctx, 40.into()));
                assert!(futures::poll!(&mut write).is_pending());
                drop(write);
                Ok(TaskOutcome::Completed(Value::Null))
            },
        ));
    let mut h = Harness::new(storage.clone(), handler, |_| {});
    let id = h.create(1.into());
    h.resume();
    assert_eq!(storage.checkpoint_calls.get(), 1);
    let mut closing = Box::pin(h.host.close());
    h.pool.run_until(async {
        assert!(futures::poll!(&mut closing).is_pending());
    });
    assert!(!storage.closed.get());
    tx.send(()).unwrap();
    h.pool.run_until(closing).unwrap();
    assert_eq!(storage.records.borrow()[&id].checkpoint, Some(40.into()));
    assert_eq!(storage.records.borrow()[&id].status, TaskStatus::Ready);
    assert!(storage.records.borrow()[&id].assignment.is_none());
    assert!(storage.closed.get());
}
#[test]
fn transient_checkpoint_error_retains_typed_cause_and_waits_for_explicit_wake() {
    let storage = Rc::new(Memory::default());
    storage.unavailable.set(true);
    let handler =
        Orb::new("fixture.task", 1).service(Service::new("fixture.task", |_| Ok(())).method(
            TaskMethod::Execute.as_str(),
            |_, ctx, e: Execution| async move {
                e.commit(&ctx, 40.into()).await?;
                Ok(TaskOutcome::Completed(e.input()))
            },
        ));
    let mut h = Harness::new(storage.clone(), handler, |_| {});
    let id = h.create(1.into());
    h.resume();
    assert_eq!(storage.records.borrow()[&id].status, TaskStatus::Ready);
    assert_eq!(storage.checkpoint_calls.get(), 1);
    assert!(matches!(
        h.status().diagnostic,
        Some(SchedulerDiagnostic::Interrupted {
            error: TaskFailure {
                code: Code::Unavailable,
                ..
            },
            ..
        })
    ));
    h.pool.run_until_stalled();
    assert_eq!(storage.checkpoint_calls.get(), 1);
    h.call::<_, ()>(Method::Wake, ()).unwrap();
    h.pool.run_until_stalled();
    assert_eq!(
        storage.records.borrow()[&id].status,
        TaskStatus::Completed(1.into())
    );
    h.close();
}
#[test]
fn renewal_and_expiry_use_authority_time_and_cancel_nested_context() {
    for expired in [false, true] {
        let storage = Rc::new(Memory::default());
        let (tx, rx) = oneshot::channel();
        let timer = Rc::new(RefCell::new(Some(rx)));
        let observed = Rc::new(RefCell::new(None));
        let capture = observed.clone();
        let handler = Orb::new("fixture.task", 1).service(
            Service::new("fixture.task", move |_| Ok(capture.clone())).method(
                TaskMethod::Execute.as_str(),
                |seen, ctx, e: Execution| async move {
                    e.cancelled().await;
                    *seen.borrow_mut() = Some(ctx.check().unwrap_err().code);
                    e.check(&ctx)?;
                    Ok(TaskOutcome::Completed(Value::Null))
                },
            ),
        );
        let mut h = Harness::new(storage.clone(), handler, move |o| {
            o.timer = Rc::new(move |_| {
                let rx = timer.borrow_mut().take();
                async move {
                    if let Some(rx) = rx {
                        let _ = rx.await;
                    } else {
                        futures::future::pending().await
                    }
                }
                .boxed_local()
            })
        });
        let id = h.create(1.into());
        h.resume();
        storage.now.set(if expired { 100 } else { 40 });
        tx.send(()).unwrap();
        h.pool.run_until_stalled();
        if expired {
            assert_eq!(*observed.borrow(), Some(Code::Cancelled));
            assert_eq!(storage.records.borrow()[&id].status, TaskStatus::Ready);
            assert!(storage.records.borrow()[&id].assignment.is_none());
        } else {
            let r = storage.records.borrow()[&id].clone();
            assert_eq!(r.assignment.unwrap().expires_at, 140);
            assert_eq!(r.revision, 0);
        }
        h.close();
    }
}
#[test]
fn portable_execution_is_refused() {
    assert!(serde_json::from_value::<Execution>(serde_json::json!({})).is_err());
}

#[test]
fn transient_renewal_outage_keeps_valid_handler_and_retries_next_tick() {
    let storage = Rc::new(Memory::default());
    let (ticks, receiver) = futures::channel::mpsc::unbounded::<()>();
    let receiver = Rc::new(futures::lock::Mutex::new(receiver));
    let entered = Rc::new(RefCell::new(vec![]));
    let gates = Rc::new(RefCell::new(BTreeMap::new()));
    let mut h = Harness::new(
        storage.clone(),
        gated(false, entered.clone(), gates.clone()),
        move |o| {
            o.timer = Rc::new(move |_| {
                let receiver = receiver.clone();
                async move {
                    let _ = futures::StreamExt::next(&mut *receiver.lock().await).await;
                }
                .boxed_local()
            });
        },
    );
    let id = h.create(1.into());
    let (finish, gate) = oneshot::channel();
    gates.borrow_mut().insert(id, gate);
    h.resume();
    storage.now.set(40);
    storage.unavailable_renew.set(true);
    ticks.unbounded_send(()).unwrap();
    h.pool.run_until_stalled();
    assert_eq!(h.status().active, 1);
    assert!(h.status().interrupted.is_empty());
    storage.now.set(50);
    ticks.unbounded_send(()).unwrap();
    h.pool.run_until_stalled();
    assert_eq!(storage.renewed.get(), 1);
    assert_eq!(
        storage.records.borrow()[&id]
            .assignment
            .as_ref()
            .unwrap()
            .expires_at,
        150
    );
    finish.send(()).unwrap();
    h.pool.run_until_stalled();
    assert_eq!(
        storage.records.borrow()[&id].status,
        TaskStatus::Completed(1.into())
    );
    assert_eq!(*entered.borrow(), vec![id]);
    h.close();
}

#[test]
fn permanent_claim_refusal_is_diagnostic_and_does_not_spin() {
    for rejection in [
        Rejection::Exhausted,
        Rejection::Terminal,
        Rejection::Invalid,
    ] {
        let storage = Rc::new(Memory::default());
        *storage.reject_claim.borrow_mut() = Some(rejection);
        let mut h = Harness::new(storage.clone(), simple(), |_| {});
        h.create(1.into());
        h.resume();
        assert_eq!(storage.claim_calls.get(), 1);
        assert!(h.status().diagnostic.is_some());
        h.close();
    }
}

#[test]
fn malformed_candidate_does_not_block_healthy_claims() {
    let storage = Rc::new(Memory::default());
    let mut h = Harness::new(storage.clone(), simple(), |_| {});
    let bad = h.create(0.into());
    let good = h.create(1.into());
    storage.bad_claim.set(Some(bad));
    h.resume();
    assert_eq!(storage.records.borrow()[&bad].status, TaskStatus::Ready);
    assert!(storage.records.borrow()[&bad].assignment.is_none());
    assert_eq!(
        storage.records.borrow()[&good].status,
        TaskStatus::Completed(1.into())
    );
    assert_eq!(h.status().interrupted, vec![bad]);
    h.close();
}

#[test]
fn blocked_backlog_yields_between_metadata_pages() {
    let storage = Rc::new(Memory::default());
    let mut h = Harness::new(storage.clone(), simple(), |_| {});
    for _ in 0..130 {
        h.call::<_, TaskId>(
            Method::Create,
            (TaskSpec {
                id: TaskId::new(),
                definition: TaskDefinition::new("fixture.absent", NonZeroU32::new(1).unwrap())
                    .unwrap(),
                input: Value::Null,
            },),
        )
        .unwrap();
    }
    h.call::<_, ()>(Method::Resume, ()).unwrap();
    let before = storage.candidate_reads.get();
    let observed = Rc::new(Cell::new(0));
    let captured = observed.clone();
    let owned = storage.clone();
    futures::task::LocalSpawnExt::spawn_local(&h.pool.spawner(), async move {
        captured.set(owned.candidate_reads.get() - before);
    })
    .unwrap();
    h.pool.run_until_stalled();
    assert!(
        observed.get() <= 1,
        "one planning turn reads at most one page"
    );
    assert!(storage.candidate_reads.get() - before >= 2);
    h.close();
}

#[test]
fn adapter_without_joint_support_refuses_before_checkpoint_submission() {
    let storage = Rc::new(Memory::default());
    let owner = storage.owner();
    let handler = Orb::new("fixture.task", 1).service(
        Service::new("fixture.task", move |_| Ok(owner.clone())).method(
            TaskMethod::Execute.as_str(),
            |owner, ctx, e: Execution| async move {
                let error = e
                    .commit_with(&ctx, move |ctx| {
                        async move {
                            owner.stage(&ctx, 7_u64)?;
                            Ok(40.into())
                        }
                        .boxed_local()
                    })
                    .await
                    .unwrap_err();
                assert_eq!(error.code, Code::Unsupported);
                assert_eq!(e.checkpoint(), None);
                Ok(TaskOutcome::Completed(Value::Null))
            },
        ),
    );
    let mut h = Harness::new(storage.clone(), handler, |_| {});
    let id = h.create(Value::Null);
    h.resume();
    assert_eq!(storage.checkpoint_calls.get(), 0);
    assert_eq!(storage.records.borrow()[&id].checkpoint, None);
    h.close();
}

#[test]
fn early_signal_resumes_while_previous_attempt_resources_are_still_closing() {
    use std::collections::BTreeSet;
    for capacity in [1, 32, 0] {
        let senders = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(0));
        let addresses = Rc::new(RefCell::new(BTreeSet::new()));
        let (outgoing, finished, seen) = (senders.clone(), closed.clone(), addresses.clone());
        let handler = Orb::new("fixture.task", 1).service(
            Service::new("fixture.task", move |declaration| {
                let address = declaration.address()?.key;
                assert_eq!(address.len(), 2);
                assert!(seen.borrow_mut().insert(address.clone()));
                let (send, receive) = oneshot::channel();
                outgoing.borrow_mut().push(send);
                let finished = finished.clone();
                declaration.resource(
                    "closing-gate",
                    1,
                    || Ok(()),
                    move |_, _| {
                        async move {
                            receive.await.unwrap();
                            finished.set(finished.get() + 1);
                            Ok(())
                        }
                        .boxed_local()
                    },
                )?;
                Ok(address)
            })
            .method(
                TaskMethod::Execute.as_str(),
                |address, context, execution: Execution| async move {
                    execution.check(&context)?;
                    assert_eq!(address[0], execution.id().to_string());
                    if execution.checkpoint().is_none() {
                        Ok(TaskOutcome::Waiting {
                            checkpoint: 1.into(),
                            condition: WaitFor::Signal(
                                serde_json::from_value(execution.input()).unwrap(),
                            ),
                        })
                    } else {
                        assert_eq!(execution.signal().unwrap().value, Value::from(7));
                        Ok(TaskOutcome::Completed(9.into()))
                    }
                },
            ),
        );
        let mut h = Harness::new(Rc::default(), handler, |options| {
            options.max_executions = capacity
        });
        for _ in 0..128 {
            let key = SignalKey::new();
            let id = h.create(serde_json::to_value(key).unwrap());
            h.call::<_, ()>(Method::Signal, (id, key, Value::from(7)))
                .unwrap();
        }
        h.resume();
        assert_eq!(
            closed.get(),
            0,
            "all prior cleanup remains deliberately blocked"
        );
        assert_eq!(addresses.borrow().len(), 256);
        assert!(
            h.storage
                .records
                .borrow()
                .values()
                .all(|record| record.status == TaskStatus::Completed(9.into())
                    && record.assignment.is_none())
        );
        assert!(h.status().interrupted.is_empty());
        for send in senders.borrow_mut().drain(..) {
            send.send(()).unwrap();
        }
        h.pool.run_until_stalled();
        assert_eq!(closed.get(), 256);
        h.close();
    }
}

#[test]
fn more_than_one_completion_group_settles_without_losing_pending_calls() {
    let entered = Rc::new(RefCell::new(vec![]));
    let gates = Rc::new(RefCell::new(BTreeMap::new()));
    let mut h = Harness::new(
        Rc::default(),
        gated(false, entered.clone(), gates.clone()),
        |options| options.max_executions = 0,
    );
    let mut sends = vec![];
    for _ in 0..160 {
        let id = h.create(1.into());
        let (send, receive) = oneshot::channel();
        gates.borrow_mut().insert(id, receive);
        sends.push(send);
    }
    h.resume();
    assert_eq!(entered.borrow().len(), 160);
    assert_eq!(h.status().active, 160);
    for send in sends {
        send.send(()).unwrap();
    }
    h.pool.run_until_stalled();
    assert_eq!(h.status().active, 0);
    assert!(h.status().interrupted.is_empty());
    assert!(
        h.storage
            .records
            .borrow()
            .values()
            .all(|record| record.status == TaskStatus::Completed(1.into())
                && record.assignment.is_none())
    );
    h.close();
}

#[test]
fn authority_loss_during_ready_completions_revokes_remaining_work() {
    let storage = Rc::new(Memory::default());
    let gates = Rc::new(RefCell::new(
        BTreeMap::<TaskId, oneshot::Receiver<()>>::new(),
    ));
    let (authority, incoming) = (storage.clone(), gates.clone());
    let handler = Orb::new("fixture.task", 1).service(
        Service::new("fixture.task", move |_| {
            Ok((authority.clone(), incoming.clone()))
        })
        .method(
            TaskMethod::Execute.as_str(),
            |env, _, execution: Execution| async move {
                let gate = env.1.borrow_mut().remove(&execution.id()).unwrap();
                gate.await.unwrap();
                if execution.input() == 0 {
                    env.0.owner.require_recovery();
                }
                Ok(TaskOutcome::Completed(execution.input()))
            },
        ),
    );
    let mut h = Harness::new(storage.clone(), handler, |_| {});
    let mut sends = vec![];
    for index in 0..160 {
        let id = h.create(index.into());
        let (send, receive) = oneshot::channel();
        gates.borrow_mut().insert(id, receive);
        sends.push(send);
    }
    h.resume();
    assert_eq!(h.status().active, 160);
    for send in sends {
        send.send(()).unwrap();
    }
    h.pool.run_until_stalled();
    assert!(storage.owner.check().is_err());
    assert_eq!(h.status().active, 0);
    assert!(
        storage
            .records
            .borrow()
            .values()
            .all(|record| record.status == TaskStatus::Ready && record.revision == 0)
    );
    h.close();
}

#[test]
fn shutdown_from_a_ready_completion_never_admits_queued_work() {
    let gates = Rc::new(RefCell::new(
        BTreeMap::<TaskId, oneshot::Receiver<()>>::new(),
    ));
    let host = Rc::new(RefCell::new(None::<Host>));
    let (incoming, current_host) = (gates.clone(), host.clone());
    let handler = Orb::new("fixture.task", 1).service(
        Service::new("fixture.task", move |_| {
            Ok((incoming.clone(), current_host.clone()))
        })
        .method(
            TaskMethod::Execute.as_str(),
            |env, _, execution: Execution| async move {
                let gate = env.0.borrow_mut().remove(&execution.id()).unwrap();
                gate.await.unwrap();
                if execution.input() == 0 {
                    let host = env.1.borrow().as_ref().unwrap().clone();
                    let mut closing = Box::pin(host.close());
                    assert!(futures::poll!(&mut closing).is_pending());
                }
                Ok(TaskOutcome::Completed(execution.input()))
            },
        ),
    );
    let mut h = Harness::new(Rc::default(), handler, |options| {
        options.max_executions = 128
    });
    *host.borrow_mut() = Some(h.host.clone());
    let mut sends = vec![];
    let mut queued = vec![];
    for index in 0..160 {
        let id = h.create(index.into());
        let (send, receive) = oneshot::channel();
        gates.borrow_mut().insert(id, receive);
        sends.push(send);
        if index >= 128 {
            queued.push(id);
        }
    }
    h.resume();
    assert_eq!(h.status().active, 128);
    for send in sends {
        send.send(()).unwrap();
    }
    h.pool.run_until_stalled();
    assert!(queued.iter().all(|id| {
        let records = h.storage.records.borrow();
        records[id].status == TaskStatus::Ready && records[id].assignment.is_none()
    }));
    h.close();
}

#[test]
fn self_cancelled_execution_is_held_without_automatic_readmission() {
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let handler = Orb::new("fixture.task", 1).service(
        Service::new("fixture.task", move |_| Ok(observed.clone())).method(
            TaskMethod::Execute.as_str(),
            |calls, context, _: Execution| async move {
                calls.set(calls.get() + 1);
                assert_eq!(
                    calls.get(),
                    1,
                    "cancellation must never implicitly readmit the task"
                );
                context.cancel()?;
                Ok(TaskOutcome::Completed(Value::Null))
            },
        ),
    );
    let mut h = Harness::new(Rc::default(), handler, |_| {});
    let id = h.create(1.into());
    h.resume();
    assert_eq!(calls.get(), 1);
    assert_eq!(h.storage.records.borrow()[&id].status, TaskStatus::Ready);
    assert!(h.storage.records.borrow()[&id].assignment.is_none());
    assert_eq!(h.status().interrupted, vec![id]);
    h.close();
}

#[test]
fn durable_delay_uses_the_deadline_timer_and_releases_capacity() {
    let storage = Rc::new(Memory::default());
    storage.now.set(10);
    let (send, receive) = oneshot::channel();
    let timer = async move {
        receive.await.unwrap();
    }
    .boxed_local()
    .shared();
    let handler =
        Orb::new("fixture.task", 1).service(Service::new("fixture.task", |_| Ok(())).method(
            TaskMethod::Execute.as_str(),
            |_, context, execution: Execution| async move {
                execution.check(&context)?;
                if execution.input() == 50 && execution.checkpoint().is_none() {
                    Ok(TaskOutcome::Waiting {
                        checkpoint: 7.into(),
                        condition: WaitFor::Delay { milliseconds: 50 },
                    })
                } else {
                    Ok(TaskOutcome::Completed(execution.input()))
                }
            },
        ));
    let mut h = Harness::new(storage.clone(), handler, |options| {
        options.max_executions = 1;
        options.lease_duration_ms = NonZeroU64::new(300_000).unwrap();
        options.timer = Rc::new(move |duration| {
            if duration == std::time::Duration::from_millis(50) {
                timer.clone().boxed_local()
            } else {
                futures::future::pending().boxed_local()
            }
        });
    });
    let delayed = h.create(50.into());
    let ready = h.create(0.into());
    h.resume();
    assert_eq!(
        storage.records.borrow()[&delayed].status,
        TaskStatus::Waiting(WaitState::Until(60))
    );
    assert!(storage.records.borrow()[&delayed].assignment.is_none());
    assert_eq!(
        storage.records.borrow()[&ready].status,
        TaskStatus::Completed(0.into())
    );
    assert_eq!(h.status().active, 0);
    storage.now.set(60);
    send.send(()).unwrap();
    h.pool.run_until_stalled();
    let record = storage.records.borrow()[&delayed].clone();
    assert_eq!(record.status, TaskStatus::Completed(50.into()));
    assert_eq!(record.checkpoint, Some(7.into()));
    assert_eq!(record.revision, 2);
    assert!(record.assignment.is_none());
    h.close();
}

#[test]
fn signal_owner_authorization_and_committed_notification_resume_waiting_work() {
    let key = SignalKey::new();
    let handler =
        Orb::new("fixture.task", 1).service(Service::new("fixture.task", |_| Ok(())).method(
            TaskMethod::Execute.as_str(),
            |_, _, execution: Execution| async move {
                if execution.checkpoint().is_none() {
                    Ok(TaskOutcome::Waiting {
                        checkpoint: 1.into(),
                        condition: WaitFor::Signal(
                            serde_json::from_value(execution.input()).unwrap(),
                        ),
                    })
                } else {
                    Ok(TaskOutcome::Completed(execution.signal().unwrap().value))
                }
            },
        ));
    let mut h = Harness::new(Rc::default(), handler, |_| {});
    let spec = TaskSpec {
        id: TaskId::new(),
        definition: definition(),
        input: serde_json::to_value(key).unwrap(),
    };
    let id = spec.id;
    let reference = h.tasks.clone();
    h.pool
        .run_until(h.host.root("alice", move |context| {
            async move {
                reference
                    .call::<_, TaskId>(&context, Method::Create.as_str(), (spec,))
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
    h.resume();
    assert_eq!(
        h.storage.records.borrow()[&id].status,
        TaskStatus::Waiting(WaitState::Signal(key))
    );
    let reference = h.tasks.clone();
    let refused = h
        .pool
        .run_until(h.host.root("bob", move |context| {
            async move {
                reference
                    .call::<_, ()>(&context, Method::Signal.as_str(), (id, key, Value::from(7)))
                    .await
            }
            .boxed_local()
        }))
        .unwrap_err();
    assert_eq!(refused.code, tembra::Code::Forbidden);
    assert!(h.storage.receipts.borrow().is_empty());
    let reference = h.tasks.clone();
    h.pool
        .run_until(h.host.root("alice", move |context| {
            async move {
                reference
                    .call::<_, ()>(&context, Method::Signal.as_str(), (id, key, Value::from(7)))
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
    h.pool.run_until_stalled();
    assert_eq!(
        h.storage.records.borrow()[&id].status,
        TaskStatus::Completed(7.into())
    );
    assert!(h.storage.records.borrow()[&id].assignment.is_none());
    assert!(h.storage.receipts.borrow()[&(id, key)].consumed);
    h.close();
}
