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

use crate::{Configuration, reads::is_contention, work::Work};

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

    fn wait(&self) -> impl Future<Output = ()> + '_ {
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

    pub async fn commit(&self, context: &Context, writes: Vec<TaskWrite>) -> Result<CommitResult> {
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
                if sequence > self.committed_sequence.value()? {
                    self.committed_sequence.replace(context, sequence)?;
                }
                self.wake.notify();
                Ok(CommitResult::Committed { sequence, records })
            }
            conflict @ CommitResult::Conflict { .. } => Ok(conflict),
        }
    }

    pub async fn update(&self, context: &Context, mut record: TaskRecord) -> Result<CommitResult> {
        let revision = record.revision;
        record.revision = revision.checked_add(1).ok_or_else(|| {
            RuntimeError::invalid("durable_tasks_revision", "task revision is exhausted")
        })?;
        self.commit(
            context,
            vec![TaskWrite {
                expected_revision: Some(revision),
                record,
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
