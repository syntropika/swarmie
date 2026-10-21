use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};

use futures::FutureExt;
use swarmie_task_contracts::{
    CommitResult, DurableTasks, DurableTasksProvider, ResumeResult, StorageOwner, TaskLookup,
    TaskOutcome, TaskPage, TaskQuery, TaskRecord, TaskSpec, TaskStatus, TaskStorage,
    TaskStorageInput, TaskStorageRef, TaskTransaction, TaskWrite, task_definitions,
};
use tembra::{core, prelude::*};

use crate::{
    Configuration, FACET_ID,
    reads::is_contention,
    scheduler::{Engine, Wake},
    work::Work,
};

/// Declares a compiled scheduler with a replaceable TaskStorage dependency. The supplied Driver
/// is the existing Host timer adapter, not another executor. Its lifecycle Resource exclusively
/// claims storage before the managed activity starts, and releases only after physical work settles.
/// Closing or retiring this Facet preserves every committed task and checkpoint.
///
/// # Errors
/// Returns invalid configuration/declaration errors immediately. Dependency, ownership and
/// Resource startup failures are reported by the actual Tembra installation readiness waiter.
pub fn facet(configuration: &Configuration, driver: Rc<dyn core::Driver>) -> Result<NativeFacet> {
    configuration.validate()?;
    let configuration = configuration.clone();
    NativeFacet::new(FACET_ID, vec![], move |scope, setup| {
        let scope = scope.clone();
        let driver = driver.clone();
        let configuration = configuration.clone();
        let registry = scope.registry(&task_definitions()?)?;
        setup.serve_with_options(
            DurableTasks,
            move |_, _, declaration| {
                let storage = declaration.get(TaskStorage, TaskStorageInput {})?;
                let opening = storage.clone();
                let closing = storage.clone();
                let opening_scope = scope.clone();
                let closing_scope = scope.clone();
                let reservations = Rc::new(RefCell::new(BTreeSet::new()));
                let cleanup_reservations = reservations.clone();
                let mut resource = ResourceDeclaration::new(move |_| {
                    let storage = opening.clone();
                    opening_scope.root(move |context| {
                        async move {
                            let owner = StorageOwner {
                                id: uuid::Uuid::new_v4().to_string(),
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
                .with_close(move |owner: Rc<StorageOwner>| {
                    let storage = closing.clone();
                    let reservations = cleanup_reservations.borrow().iter().cloned().collect::<Vec<_>>();
                    closing_scope.root(move |context| {
                        async move {
                            let context = Context::from_core(context);
                            let restored = restore_reservations(&storage, &context, &owner.id, reservations).await;
                            let released = storage.with_context(&context).release_owner(owner.as_ref().clone()).await;
                            match (restored, released) {
                                (Ok(()), result) | (result, Ok(())) => result,
                                (Err(primary), Err(release)) => Err(RuntimeError::unavailable("durable_tasks_cleanup", format!("reservation cleanup failed: {primary}; owner release failed: {release}"))),
                            }
                        }
                        .boxed_local()
                    })
                });
                resource
                    .deps
                    .push(core::ResourceDependency::Reference(storage.core().clone()));
                let owner = declaration
                    .core_mut()
                    .resource("scheduler-owner", resource)?;
                let initial_storage = storage.clone();
                let committed_sequence = declaration.state_with("committed_sequence", StateDeclaration::with_initializer(move |context| {
                    let storage = initial_storage.clone();
                    async move {
                        storage.with_context(&context).scan(TaskQuery { limit:1, statuses:vec![TaskStatus::Pending], ..Default::default() }).await.map(|page| page.sequence)
                    }
                })?)?;
                let engine = Rc::new(Engine {
                    storage,
                    owner,
                    registry: registry.clone(),
                    configuration: configuration.clone(),
                    driver: driver.clone(),
                    wake: Wake::default(),
                    active: RefCell::new(BTreeMap::new()),
                    inflight: RefCell::new(BTreeSet::new()),
                    failure: RefCell::new(None),
                    rejected_migrations: RefCell::new(BTreeMap::new()),
                    reservations,
                    committed_sequence,
                    work: RefCell::new(Work::default()),
                    reconcile_gate: futures::lock::Mutex::new(()),
                });
                let activity = engine.clone();
                declaration
                    .core_mut()
                    .task(move |scope| activity.clone().run(scope).boxed_local())?;
                Ok(Service { engine })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Service {
    engine: Rc<Engine>,
}

/// The Resource closer runs after managed activity and physical invocation ownership settles.
/// Restore only this owner's reservations; preexisting interrupted records retain crash metadata.
/// Abrupt process termination does not run this cleanup.
async fn restore_reservations(
    storage: &TaskStorageRef,
    context: &Context,
    owner: &str,
    reservations: Vec<String>,
) -> Result<()> {
    for id in reservations {
        let mut restored = false;
        for _ in 0..8 {
            let Some(mut record) = storage
                .with_context(context)
                .get(TaskLookup { id: id.clone() })
                .await?
            else {
                break;
            };
            if record.status != TaskStatus::Running {
                restored = true;
                break;
            }
            let expected_revision = record.revision;
            record.revision = record.revision.checked_add(1).ok_or_else(|| {
                RuntimeError::invalid("durable_tasks_revision", "task revision exhausted")
            })?;
            record.status = TaskStatus::Pending;
            match storage
                .with_context(context)
                .commit(TaskTransaction {
                    owner: owner.to_owned(),
                    writes: vec![TaskWrite {
                        expected_revision: Some(expected_revision),
                        record,
                    }],
                })
                .await?
            {
                CommitResult::Committed { .. } => {
                    restored = true;
                    break;
                }
                CommitResult::Conflict { .. } => {}
            }
        }
        if !restored {
            return Err(RuntimeError::unavailable(
                "durable_tasks_cleanup",
                "graceful reservation recovery repeatedly conflicted",
            ));
        }
    }
    Ok(())
}

impl DurableTasksProvider for Service {
    fn committed_sequence(&self) -> Readable<u64> {
        self.engine.committed_sequence.readable()
    }

    async fn create(&self, task: TaskSpec, context: Context) -> Result<TaskRecord> {
        self.engine.ensure_running()?;
        let record = TaskRecord::new(task, None);
        match self
            .engine
            .commit(
                &context,
                vec![TaskWrite {
                    expected_revision: None,
                    record: record.clone(),
                }],
            )
            .await?
        {
            CommitResult::Conflict { .. } => Err(RuntimeError::invalid(
                "durable_tasks_exists",
                "task identity is already committed",
            )),
            CommitResult::Committed { mut records, .. }
                if records.len() == 1 && records[0].id == record.id =>
            {
                Ok(records.remove(0))
            }
            CommitResult::Committed { .. } => Err(RuntimeError::invalid(
                "durable_tasks_storage_receipt",
                "storage commit did not return the created task",
            )),
        }
    }

    async fn get(&self, request: TaskLookup, context: Context) -> Result<Option<TaskRecord>> {
        self.engine.get(&context, &request.id).await
    }

    async fn wait(&self, request: TaskLookup, context: Context) -> Result<TaskOutcome> {
        loop {
            let record = self.engine.require(&context, &request.id).await?;
            if record.is_terminal() {
                return record.outcome.ok_or_else(|| {
                    RuntimeError::invalid("durable_tasks_outcome", "terminal task has no outcome")
                });
            }
            self.engine.ensure_running()?;
            let stopped = context.cancellation().cancelled();
            let timer = self.engine.driver.sleep(Duration::from_millis(
                self.engine.configuration.poll_interval_ms,
            ));
            if let futures::future::Either::Left((error, _)) =
                futures::future::select(stopped, timer).await
            {
                return Err(error);
            }
        }
    }

    async fn cancel(&self, request: TaskLookup, context: Context) -> Result<TaskRecord> {
        self.engine.ensure_running()?;
        for _ in 0..8 {
            let root = self.engine.require(&context, &request.id).await?;
            if root.is_terminal() {
                return Ok(root);
            }
            if !root.cancellation_requested {
                let mut next = root.clone();
                next.cancellation_requested = true;
                if matches!(
                    self.engine.update(&context, next).await?,
                    CommitResult::Conflict { .. }
                ) {
                    continue;
                }
            }
            // Persist root intent first. Work hints enqueue its active subtree; fresh
            // ancestor reads prevent admission even before descendant intent is copied.
            self.engine.schedule(&request.id);
            let active: Vec<_> = self.engine.active.borrow().keys().cloned().collect();
            for id in active {
                match self.engine.family(&context, &id).await {
                    Ok(family)
                        if family.task.id == request.id
                            || family
                                .ancestors
                                .iter()
                                .any(|ancestor| ancestor.id == request.id) =>
                    {
                        let token = self.engine.active.borrow().get(&id).cloned();
                        if let Some(token) = token {
                            token.cancel(RuntimeError::aborted(
                                "task_cancelled",
                                "cancellation requested",
                            ));
                        }
                    }
                    Ok(_) => {}
                    Err(error) if is_contention(&error) => self.engine.schedule(&id),
                    Err(error) => return Err(error),
                }
            }
            return self.engine.require(&context, &request.id).await;
        }
        Err(RuntimeError::unavailable(
            "durable_tasks_conflict",
            "cancellation repeatedly conflicted with committed progress",
        ))
    }

    async fn retry(&self, request: TaskLookup, context: Context) -> Result<TaskRecord> {
        self.engine.ensure_running()?;
        for _ in 0..8 {
            let mut task = self.engine.require(&context, &request.id).await?;
            if !matches!(task.status, TaskStatus::Parked | TaskStatus::Blocked)
                || task.cancellation_requested
            {
                return Err(RuntimeError::invalid(
                    "durable_tasks_retry",
                    "only uncancelled parked or blocked tasks can be retried",
                ));
            }
            task.status = TaskStatus::Pending;
            task.restarts = 0;
            task.blocking_reason = None;
            if matches!(
                self.engine.update(&context, task).await?,
                CommitResult::Committed { .. }
            ) {
                return self.engine.require(&context, &request.id).await;
            }
        }
        Err(RuntimeError::unavailable(
            "durable_tasks_conflict",
            "retry repeatedly conflicted with committed progress",
        ))
    }

    async fn resume(&self, context: Context) -> Result<ResumeResult> {
        self.engine.ensure_running()?;
        let before = self.engine.sequence(&context).await?;
        self.engine.work.borrow_mut().rescan = true;
        match self.engine.reconcile(&context).await {
            Ok(candidates) => {
                for task in candidates {
                    self.engine.schedule(&task.id);
                }
            }
            Err(error) if is_contention(&error) => {}
            Err(error) => return Err(error),
        }
        self.engine.wake.notify();
        let after = self.engine.sequence(&context).await?;
        Ok(ResumeResult {
            progressed: u32::try_from(after.saturating_sub(before)).unwrap_or(u32::MAX),
        })
    }

    async fn graph(&self, query: TaskQuery, context: Context) -> Result<TaskPage> {
        self.engine.storage.with_context(&context).scan(query).await
    }
}
