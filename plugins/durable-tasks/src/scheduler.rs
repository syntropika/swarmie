use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    task::{Poll, Waker},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use swarmie_task_contracts::{
    CommitResult, StorageOwner, TaskLookup, TaskRecord, TaskStorageRef, TaskTransaction, TaskWrite,
    validate_task_payload_size, validate_task_record_size,
};
use tembra::{Context, Resource, Result, RuntimeError, core};

use crate::{
    Configuration,
    mutation::{Change, Mutation},
    reads::is_contention,
    work::Work,
};

/// Coalesced local pump notification. Task transitions remain exclusively in TaskStorage.
#[derive(Default)]
pub(crate) struct Wake {
    pending: Cell<bool>,
    waiter: RefCell<Option<Waker>>,
}

impl Wake {
    pub fn notify(&self) {
        self.pending.set(true);
        let waiter = self.waiter.borrow_mut().take();
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub(crate) fn wait(&self) -> impl Future<Output = ()> + '_ {
        futures::future::poll_fn(|context| {
            if self.pending.replace(false) {
                Poll::Ready(())
            } else {
                *self.waiter.borrow_mut() = Some(context.waker().clone());
                Poll::Pending
            }
        })
    }
}

pub(crate) struct Engine {
    pub observations: Rc<crate::observation::Observations>,
    pub storage: TaskStorageRef,
    pub owner: Resource<StorageOwner>,
    pub registry: core::RegistryView,
    pub configuration: Configuration,
    pub driver: Rc<dyn core::Driver>,
    pub wake: Wake,
    /// Ephemeral invocation observations; none replaces a persisted task status.
    pub active: RefCell<BTreeMap<String, core::CancellationToken>>,
    pub inflight: RefCell<BTreeSet<String>>,
    pub failure: RefCell<Option<RuntimeError>>,
    pub rejected_migrations: RefCell<BTreeMap<String, core::RegisteredOperation>>,
    /// Reservations admitted by this owner only. IDs provide no execution authority and
    /// are retained only until a non-running commit or graceful Resource cleanup.
    pub reservations: Rc<RefCell<BTreeSet<String>>>,
    pub committed_sequence: tembra::Cell<u64>,
    pub work: RefCell<Work>,
    pub reconcile_gate: futures::lock::Mutex<()>,
}

impl Engine {
    /// Reactive publication is a retryable hint, never the authority for a committed write.
    pub fn publish_sequence(&self, context: &Context, sequence: u64) {
        let result = self.committed_sequence.value().and_then(|current| {
            if sequence > current {
                self.committed_sequence
                    .replace(context, sequence)
                    .map(|_| ())
            } else {
                Ok(())
            }
        });
        if let Err(error) = result {
            self.observations.record_error(&error);
        }
    }
    pub fn ensure_running(&self) -> Result<()> {
        if let Some(error) = self.failure.borrow().clone() {
            return Err(RuntimeError::unavailable(
                "durable_tasks_stopped",
                format!("scheduler activity stopped: {error}"),
            ));
        }
        self.owner.value()?;
        Ok(())
    }

    pub async fn get(&self, context: &Context, id: &str) -> Result<Option<TaskRecord>> {
        self.storage
            .with_context(context)
            .get(TaskLookup { id: id.to_owned() })
            .await
    }

    pub async fn require(&self, context: &Context, id: &str) -> Result<TaskRecord> {
        self.get(context, id)
            .await?
            .ok_or_else(|| RuntimeError::invalid("durable_tasks_missing", "task does not exist"))
    }

    pub fn schedule(&self, id: &str) {
        self.work.borrow_mut().schedule(id.to_owned());
        self.wake.notify();
    }

    pub async fn commit(
        &self,
        context: &Context,
        mutations: Vec<Mutation>,
    ) -> Result<CommitResult> {
        let mut changes = BTreeMap::new();
        let writes: Vec<_> = mutations
            .into_iter()
            .map(|mut mutation| {
                mutation.write.provenance = Some(mutation.change.provenance());
                changes.insert(
                    mutation.write.record.id.clone(),
                    (mutation.write.record.revision, mutation.change),
                );
                mutation.write
            })
            .collect();
        if writes.is_empty() {
            return Err(RuntimeError::invalid(
                "durable_tasks_transaction",
                "a task commit requires at least one write",
            ));
        }
        let owner = self.owner.value()?.id.clone();
        for write in &writes {
            validate_task_record_size(&write.record)?;
        }
        let transaction = TaskTransaction { owner, writes };
        validate_task_payload_size(&transaction)?;
        match self
            .storage
            .with_context(context)
            .commit(transaction)
            .await?
        {
            CommitResult::Committed { sequence, records } => {
                // Admission is infallible and never awaits plugins. Failed commits never reach it.
                for record in &records {
                    if let Some((revision, change)) = changes.remove(&record.id)
                        && revision == record.revision
                    {
                        self.observations.emit(change.event(record, sequence));
                    }
                }
                let mut work = self.work.borrow_mut();
                if work.initialized && sequence > work.sequence.saturating_add(1) {
                    work.rescan = true;
                }
                work.sequence = work.sequence.max(sequence);
                for record in &records {
                    work.observe(record);
                    if record.status != swarmie_task_contracts::TaskStatus::Running {
                        self.reservations.borrow_mut().remove(&record.id);
                    }
                }
                drop(work);
                // The durable receipt is final even if caller cancellation closes live state.
                self.publish_sequence(context, sequence);
                self.wake.notify();
                Ok(CommitResult::Committed { sequence, records })
            }
            conflict @ CommitResult::Conflict { .. } => Ok(conflict),
        }
    }

    pub async fn update(
        &self,
        context: &Context,
        mut record: TaskRecord,
        change: Change,
    ) -> Result<CommitResult> {
        let revision = record.revision;
        record.revision = revision.checked_add(1).ok_or_else(|| {
            RuntimeError::invalid("durable_tasks_revision", "task revision is exhausted")
        })?;
        self.commit(
            context,
            vec![Mutation {
                write: TaskWrite {
                    provenance: None,
                    expected_revision: Some(revision),
                    record,
                },
                change,
            }],
        )
        .await
    }

    pub async fn run(self: Rc<Self>, scope: core::TaskScope) -> Result<()> {
        self.failure.borrow_mut().take();
        let observed = self.clone();
        let subscription = self.registry.subscribe(move |_| {
            observed.work.borrow_mut().definitions_changed();
            observed.wake.notify();
            Ok(())
        });
        let subscription = match subscription {
            Ok(subscription) => subscription,
            Err(error) => {
                *self.failure.borrow_mut() = Some(error.clone());
                return Err(error);
            }
        };
        let mut phases = FuturesUnordered::<LocalBoxFuture<'static, Result<()>>>::new();
        let signal = scope.signal();
        let mut outcome: Result<()> = async {
            loop {
                if signal.is_cancelled() {
                    break;
                }
                let engine = self.clone();
                let candidates = scope
                    .root(move |context| {
                        async move { engine.reconcile(&Context::from_core(context)).await }
                            .boxed_local()
                    })
                    .await;
                let candidates = match candidates {
                    Ok(candidates) => candidates,
                    Err(error) if is_contention(&error) => Vec::new(),
                    Err(error) => return Err(error),
                };
                for candidate in candidates {
                    if self.inflight.borrow().len() >= self.configuration.max_parallel as usize {
                        self.work.borrow_mut().schedule(candidate.id);
                        continue;
                    }
                    if !self.inflight.borrow_mut().insert(candidate.id.clone()) {
                        continue;
                    }
                    let engine = self.clone();
                    let id = candidate.id;
                    let cleanup = self.clone();
                    let cleanup_id = id.clone();
                    let phase = scope.root(move |context| {
                        async move { engine.execute(id, Context::from_core(context)).await }
                            .boxed_local()
                    });
                    phases.push(
                        async move {
                            let result = phase.await;
                            cleanup.inflight.borrow_mut().remove(&cleanup_id);
                            cleanup.active.borrow_mut().remove(&cleanup_id);
                            cleanup.work.borrow_mut().settled(&cleanup_id);
                            cleanup.wake.notify();
                            match result {
                                Err(error) if is_contention(&error) => Ok(()),
                                result => result,
                            }
                        }
                        .boxed_local(),
                    );
                }
                // Native operations can settle synchronously. Yield through the existing
                // runtime Driver after bounded work so other Facets, timers and cancellation
                // are polled even when local notifications stay continuously ready.
                self.driver.sleep(Duration::from_millis(1)).await;
                let timer = self
                    .driver
                    .sleep(Duration::from_millis(self.configuration.poll_interval_ms));
                let notification = self.wake.wait().boxed_local();
                let stopped = signal.cancelled();
                let next = async {
                    if phases.is_empty() {
                        futures::future::pending().await
                    } else {
                        phases.next().await
                    }
                }
                .boxed_local();
                futures::select_biased! {
                    _ = stopped.fuse() => break,
                    result = next.fuse() => {
                        if let Some(result) = result {
                            result?;
                        }
                    },
                    _ = notification.fuse() => {},
                    _ = timer.fuse() => {},
                }
            }
            Ok(())
        }
        .await;
        // Report a stopped scheduler before joining non-cooperative physical work. The
        // owner claim remains held through drain, but new submissions must fail honestly.
        if let Err(error) = &outcome {
            *self.failure.borrow_mut() = Some(error.clone());
        }
        drop(subscription);
        // Cancellation asks each actual invocation to settle. Keep phase futures and their
        // receipts alive through physical completion before the owner Resource can close.
        let tokens: Vec<_> = self.active.borrow().values().cloned().collect();
        for token in tokens {
            token.cancel(RuntimeError::aborted(
                "durable_tasks_stopping",
                "scheduler is stopping",
            ));
        }
        while let Some(result) = phases.next().await {
            if let Err(error) = result
                && outcome.is_ok()
                && !signal.is_cancelled()
            {
                outcome = Err(error);
            }
        }
        if let Err(error) = &outcome {
            *self.failure.borrow_mut() = Some(error.clone());
        }
        outcome
    }
}

pub(crate) fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .ok_or_else(|| {
            RuntimeError::invalid(
                "durable_tasks_clock",
                "UTC clock is outside supported range",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use swarmie_task_contracts::{
        TaskOutboxConsumer, TaskOutboxQuery, TaskOutcome, TaskQuery, TaskSpec, TaskStatus,
        TaskStorage, TaskStorageInput, task_definitions,
    };
    use tembra::prelude::*;

    #[tembra::contract(local, name = "fixture.sequence-repair", version = 1)]
    trait SequenceRepair {
        #[state]
        fn sequence(&self) -> u64;

        #[state]
        fn projection(&self) -> u64;

        #[portable]
        async fn commit_terminal(&self) -> Result<CommitResult>;

        #[portable]
        async fn reconcile(&self) -> Result<()>;
    }

    struct RepairService {
        engine: Rc<Engine>,
        projection: Readable<u64>,
    }

    impl SequenceRepairProvider for RepairService {
        fn sequence(&self) -> Readable<u64> {
            self.engine.committed_sequence.readable()
        }

        fn projection(&self) -> Readable<u64> {
            self.projection.clone()
        }

        async fn commit_terminal(&self, context: Context) -> Result<CommitResult> {
            let mut record = TaskRecord::new(
                TaskSpec {
                    id: "confirmed-once".into(),
                    kind: "fixture.sequence-repair".into(),
                    definition_version: 1,
                    input: serde_json::Value::Null,
                    checkpoint: serde_json::Value::Null,
                    phase: "done".into(),
                },
                None,
            );
            record.status = TaskStatus::Terminal;
            record.outcome = Some(TaskOutcome::Completed {
                value: serde_json::Value::Null,
            });
            self.engine
                .commit(&context, vec![Mutation::create(record)])
                .await
        }

        async fn reconcile(&self, context: Context) -> Result<()> {
            assert!(self.engine.reconcile(&context).await?.is_empty());
            Ok(())
        }
    }

    #[test]
    fn confirmed_commit_survives_rejected_state_publication_and_reconciliation_repairs_it() {
        let driver = tembra::native::TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let directory = tempfile::tempdir().unwrap();
                let host = Host::with_driver("sequence-repair", "test", driver.clone()).unwrap();
                host.install(
                    swarmie_task_storage_sqlite::facet(
                        &swarmie_task_storage_sqlite::Configuration {
                            database: directory.path().join("tasks.sqlite"),
                            outbox: Default::default(),
                            outbox_legacy_owners: Default::default(),
                        },
                    )
                    .unwrap(),
                    core::LoadOptions::default(),
                )
                .unwrap()
                .await
                .unwrap();
                let storage = host.get(TaskStorage, TaskStorageInput {}).await.unwrap();
                let reject = Rc::new(std::cell::Cell::new(true));
                let captured = Rc::new(RefCell::new(None));
                let audit_scope = Rc::new(RefCell::new(None));
                let captured_scope = audit_scope.clone();
                let rejected = reject.clone();
                let selected = captured.clone();
                let factory_driver = driver.clone();
                host.install(
                    NativeFacet::new("fixture.sequence-repair", vec![], move |scope, setup| {
                        *captured_scope.borrow_mut() = Some(scope.clone());
                        let scope = scope.clone();
                        let driver = factory_driver.clone();
                        let reject = rejected.clone();
                        let captured = selected.clone();
                        let sequence_definition = setup.state("sequence")?;
                        let sequence_projection_definition = setup.state("sequence-projection")?;
                        setup.serve_with_options(
                            SequenceRepair,
                            move |_, _, declaration| {
                                let storage = declaration.get(TaskStorage, TaskStorageInput {})?;
                                let opening = storage.clone();
                                let closing = storage.clone();
                                let opening_scope = scope.clone();
                                let closing_scope = scope.clone();
                                let mut resource = ResourceDeclaration::new(move |_| {
                                    let storage = opening.clone();
                                    opening_scope.root(move |context| {
                                        async move {
                                            let owner = StorageOwner {
                                                id: "fixture.sequence-owner".into(),
                                            };
                                            storage
                                                .with_context(&Context::from_core(context))
                                                .claim_owner(owner.clone())
                                                .await?;
                                            Ok(owner)
                                        }
                                        .boxed_local()
                                    })
                                })
                                .with_close(
                                    move |owner: Rc<StorageOwner>| {
                                        let storage = closing.clone();
                                        closing_scope.root(move |context| {
                                            async move {
                                                storage
                                                    .with_context(&Context::from_core(context))
                                                    .release_owner(owner.as_ref().clone())
                                                    .await
                                            }
                                            .boxed_local()
                                        })
                                    },
                                );
                                resource.deps.push(core::ResourceDependency::Reference(
                                    storage.core().clone(),
                                ));
                                let owner = declaration.core_mut().resource("owner", resource)?;
                                let committed_sequence =
                                    declaration.state(sequence_definition.value(0_u64)?)?;
                                let refuse = reject.clone();
                                let projection = declaration.derive(
                                    &sequence_projection_definition,
                                    (committed_sequence.readable(),),
                                    move |(sequence,)| {
                                        if sequence > 0 && refuse.get() {
                                            return Err(RuntimeError::invalid(
                                                "fixture_projection_refused",
                                                "reject reactive publication before adoption",
                                            ));
                                        }
                                        Ok(sequence)
                                    },
                                )?;
                                let engine = Rc::new(Engine {
                                    observations: crate::observation::Observations::new(
                                        scope.clone(),
                                        driver.clone(),
                                    )?,
                                    storage,
                                    owner,
                                    registry: scope.registry(&task_definitions()?)?,
                                    configuration: Configuration::default(),
                                    driver: driver.clone(),
                                    wake: Wake::default(),
                                    active: RefCell::new(BTreeMap::new()),
                                    inflight: RefCell::new(BTreeSet::new()),
                                    failure: RefCell::new(None),
                                    rejected_migrations: RefCell::new(BTreeMap::new()),
                                    reservations: Rc::new(RefCell::new(BTreeSet::new())),
                                    committed_sequence,
                                    work: RefCell::new(Work::default()),
                                    reconcile_gate: futures::lock::Mutex::new(()),
                                });
                                *captured.borrow_mut() = Some(engine.clone());
                                Ok(RepairService { engine, projection })
                            },
                            core::ServeOptions {
                                lifetime: core::Lifetime::Host,
                                ..Default::default()
                            },
                        )
                    })
                    .unwrap(),
                    core::LoadOptions::default(),
                )
                .unwrap()
                .await
                .unwrap();
                let audit_scope = audit_scope.borrow().clone().unwrap();
                let audit_storage = storage.clone();
                audit_scope
                    .root(move |context| {
                        async move {
                            audit_storage
                                .with_context(&Context::from_core(context))
                                .register_outbox_consumer(TaskOutboxConsumer { id: "audit".into() })
                                .await
                        }
                        .boxed_local()
                    })
                    .await
                    .unwrap();
                let repair = host
                    .get(SequenceRepair, SequenceRepairInput {})
                    .await
                    .unwrap();
                repair.reconcile().await.unwrap();
                let engine = captured.borrow().clone().unwrap();
                // Prevent a storage scan from incidentally repairing the stale projection.
                engine.work.borrow_mut().next_probe = engine
                    .driver
                    .now()
                    .saturating_add(Duration::from_secs(3600));
                assert!(matches!(
                    repair.commit_terminal().await.unwrap(),
                    CommitResult::Committed { sequence: 1, .. }
                ));
                assert_eq!(engine.work.borrow().sequence, 1);
                assert!(engine.work.borrow().initialized);
                assert!(!engine.work.borrow().rescan);
                assert_eq!(repair.sequence().value().unwrap(), 0);
                assert_eq!(repair.projection().value().unwrap(), 0);
                let before = engine.observations.diagnostics();
                assert!(before.failed > 0);
                assert!(
                    before
                        .last_error
                        .as_deref()
                        .unwrap()
                        .contains("fixture_projection_refused")
                );
                assert!(before.skipped > 0);
                let durable = storage.scan(TaskQuery::default()).await.unwrap();
                assert_eq!(durable.sequence, 1);
                assert!(durable.records[0].is_terminal());
                reject.set(false);
                repair.reconcile().await.unwrap();
                assert_eq!(repair.sequence().value().unwrap(), 1);
                assert_eq!(repair.projection().value().unwrap(), 1);
                repair.reconcile().await.unwrap();
                let after = engine.observations.diagnostics();
                assert_eq!(after.enqueued, before.enqueued);
                assert_eq!(
                    after.skipped, before.skipped,
                    "repair must not emit lifecycle notifications again"
                );
                assert_eq!(
                    storage.scan(TaskQuery::default()).await.unwrap().sequence,
                    1
                );
                let events = audit_scope
                    .root(move |context| {
                        async move {
                            storage
                                .with_context(&Context::from_core(context))
                                .read_outbox(TaskOutboxQuery {
                                    consumer_id: "audit".into(),
                                    after: None,
                                    limit: 256,
                                })
                                .await
                        }
                        .boxed_local()
                    })
                    .await
                    .unwrap();
                assert_eq!(events.events.len(), 1);
                assert_eq!(events.events[0].revision, 1);
                host.close().await.unwrap();
            }));
    }
}
