//! Scheduler recovery through actual shared SQLite commits and uncertain reply boundaries.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    path::Path,
    rc::Rc,
    time::Duration,
};

use serde_json::json;
use swarmie_durable_tasks_fixture::concurrency::{StorageControl, storage_facet};
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, EventQuery, NewEvent, SessionStore,
    SessionStoreInput, SessionStoreRef,
};
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

#[derive(Clone, Copy, Default)]
enum Reply {
    #[default]
    Confirmed,
    Lost,
    Corrupt,
    Uncommitted,
}

#[derive(Default)]
struct Control {
    reply: Cell<Reply>,
    held: Cell<bool>,
    entered: Cell<bool>,
    preparations: Cell<usize>,
    effects: Cell<usize>,
    requests: RefCell<Vec<TaskSessionCommitRequest>>,
    identity: RefCell<Option<TaskSessionCommitIdentity>>,
    sessions: RefCell<Option<SessionStoreRef>>,
    release: core::CancellationToken,
}

fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(15), test(driver.clone()))
                .await
                .expect("joint scheduler acceptance timed out")
                .unwrap();
        }));
}

#[tembra::contract(local, name = "fixture.joint-handler", version = 1)]
trait HandlerContract {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskDecision>;
}

struct Handler {
    control: Rc<Control>,
}
impl HandlerContractProvider for Handler {
    async fn step(&self, invocation: TaskInvocation, _context: Context) -> Result<TaskDecision> {
        let TaskInvocation::Run { task, .. } = invocation else {
            return Ok(TaskTransition::RejectMigration {
                reason: "no migration".into(),
            }
            .into());
        };
        if task.input["peer"].as_bool() == Some(true) {
            return Ok(TaskTransition::Complete {
                value: json!({"peer":true}),
            }
            .into());
        }
        if task.phase == "effect" {
            let sessions = self.control.sessions.borrow().clone().unwrap();
            let events = sessions.events(EventQuery::new("chat")).await?.events;
            if events.len() != 1 || events[0].event.event_id != "task:intent" {
                return Err(RuntimeError::invalid(
                    "fixture_missing_intent",
                    "effect entered without its atomic intent",
                ));
            }
            self.control.effects.set(self.control.effects.get() + 1);
            return Ok(TaskTransition::Complete {
                value: json!({"effect":true}),
            }
            .into());
        }
        self.control
            .preparations
            .set(self.control.preparations.get() + 1);
        self.control.entered.set(true);
        if self.control.held.get() {
            // This physically held handler settles after logical cancellation, as an actual
            // non-cooperative contribution may. Its returned history must still be discarded.
            self.control.release.cancelled().await;
        }
        let identity = self.control.identity.borrow().clone().unwrap();
        let event_id = if task.input["malformed"].as_str() == Some("empty_id") {
            String::new()
        } else {
            "task:intent".into()
        };
        let payload = match task.input["malformed"].as_str() {
            Some("oversized") => {
                json!({"value":"x".repeat(swarmie_session_contracts::MAX_PAGE_BYTES + 1)})
            }
            // This passes portable append validation, then exceeds the native stored-event
            // budget inside the joint SQL transaction, after its task write was attempted.
            Some("oversized_stored") => json!({"value":"x".repeat(300 * 1024)}),
            _ => json!({"operation_id":"task:effect"}),
        };
        Ok(TaskDecision {
            transition: TaskTransition::Continue {
                checkpoint: json!({"prepared":true}),
                phase: "effect".into(),
            },
            session_appends: vec![TaskSessionAppend {
                session_store_id: identity.session_store_id,
                append: AppendEvents {
                    session_id: "chat".into(),
                    batch_id: "task:intent".into(),
                    expected_head: None,
                    events: vec![NewEvent {
                        event_id,
                        kind: "fixture.intent".into(),
                        version: 1,
                        correlation: Correlation {
                            task_id: Some(task.id),
                            ..Default::default()
                        },
                        occurred_at_ms: None,
                        payload,
                    }],
                },
            }],
        })
    }
}

fn handler(control: Rc<Control>) -> Result<NativeFacet> {
    NativeFacet::new("fixture.joint-handler", vec![], move |_, setup| {
        let control = control.clone();
        setup.serve_with_options(
            HandlerContract,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &task_definitions()?,
                    "fixture.joint",
                    json!({"kind":"fixture.joint","version":1}),
                    "step",
                )?;
                Ok(Handler {
                    control: control.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Proxy {
    backend: TaskSessionCommitRef,
    control: Rc<Control>,
}
impl TaskSessionCommitProvider for Proxy {
    async fn identity(&self, _context: Context) -> Result<TaskSessionCommitIdentity> {
        self.backend.identity().await
    }
    async fn commit(
        &self,
        request: TaskSessionCommitRequest,
        _context: Context,
    ) -> Result<TaskSessionCommitResult> {
        self.control.requests.borrow_mut().push(request.clone());
        let staged = !request.operation.history.is_empty();
        if staged && matches!(self.control.reply.get(), Reply::Uncommitted) {
            self.control.reply.set(Reply::Confirmed);
            return Err(RuntimeError::unavailable(
                "fixture_before_commit",
                "no SQL call reached the backend",
            ));
        }
        let result = self.backend.commit(request).await?;
        if !staged {
            return Ok(result);
        }
        if let TaskSessionCommitResult::Committed { mut receipt } = result {
            match self.control.reply.replace(Reply::Confirmed) {
                Reply::Confirmed | Reply::Uncommitted => {
                    Ok(TaskSessionCommitResult::Committed { receipt })
                }
                Reply::Lost => Err(RuntimeError::unavailable(
                    "fixture_lost_reply",
                    "joint SQL committed before the reply was lost",
                )),
                Reply::Corrupt => {
                    receipt.head.store_id = "foreign-store".into();
                    Ok(TaskSessionCommitResult::Committed { receipt })
                }
            }
        } else {
            Ok(result)
        }
    }
}

fn proxy(backend: TaskSessionCommitRef, control: Rc<Control>) -> Result<NativeFacet> {
    NativeFacet::new("fixture.joint-proxy", vec![], move |_, setup| {
        let backend = backend.clone();
        let control = control.clone();
        setup.serve_with_options(
            TaskSessionCommit,
            move |_, _, _| {
                Ok(Proxy {
                    backend: backend.clone(),
                    control: control.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Rig {
    host: Host,
    backend: Host,
    tasks: DurableTasksRef,
    storage: TaskStorageRef,
    storage_control: Rc<StorageControl>,
    sessions: SessionStoreRef,
}

async fn open(root: &Path, driver: Rc<TokioDriver>, control: Rc<Control>) -> Result<Rig> {
    let backend = Host::with_driver("joint-backend", "application", driver.clone())?;
    let (owner, handle) = swarmie_sqlite_local::facet(
        &swarmie_sqlite_local::Configuration {
            database: root.join("joint.sqlite"),
        },
        vec![
            swarmie_task_storage_sqlite::backend::sqlite_schema(),
            swarmie_session_store_sqlite::backend::sqlite_schema(),
            swarmie_task_session_sqlite::sqlite_schema(),
        ],
    )?;
    let mut facets = vec![owner];
    facets.extend(swarmie_task_session_sqlite::facets_with_database(
        &handle,
        &Default::default(),
        driver.clone(),
    )?);
    let readiness = facets
        .into_iter()
        .map(|facet| backend.install(facet, core::LoadOptions::default()))
        .collect::<Result<Vec<_>>>()?;
    for ready in readiness {
        ready.await?;
    }
    let storage = backend.get(TaskStorage, TaskStorageInput {}).await?;
    let sessions = backend.get(SessionStore, SessionStoreInput {}).await?;
    sessions
        .create(CreateSession {
            session_id: "chat".into(),
            metadata: json!({}),
        })
        .await?;
    let joint = backend
        .get(TaskSessionCommit, TaskSessionCommitInput {})
        .await?;
    *control.identity.borrow_mut() = Some(joint.identity().await?);
    *control.sessions.borrow_mut() = Some(sessions.clone());
    let host = Host::with_driver("joint-scheduler", "application", driver.clone())?;
    let storage_control = StorageControl::new(storage.clone());
    for facet in [
        storage_facet(storage_control.clone())?,
        proxy(joint, control.clone())?,
        handler(control)?,
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                commit: swarmie_durable_tasks::CommitStrategy::TaskSession,
                poll_interval_ms: 2,
                ..Default::default()
            },
            driver,
        )?,
    ] {
        host.install(facet, core::LoadOptions::default())?.await?;
    }
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    Ok(Rig {
        host,
        backend,
        tasks,
        storage,
        storage_control,
        sessions,
    })
}

fn task() -> TaskSpec {
    TaskSpec {
        id: "task".into(),
        kind: "fixture.joint".into(),
        definition_version: 1,
        input: json!({}),
        checkpoint: json!({}),
        phase: "prepare".into(),
    }
}

#[test]
fn a_valid_joint_decision_commits_intent_before_the_next_effect() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let control = Rc::new(Control::default());
        let rig = open(directory.path(), driver, control.clone()).await?;
        rig.tasks.create(task()).await?;
        assert!(matches!(
            rig.tasks.wait(TaskLookup { id: "task".into() }).await?,
            TaskOutcome::Completed { .. }
        ));
        let events = rig.sessions.events(EventQuery::new("chat")).await?.events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.event_id, "task:intent");
        assert_eq!(
            control
                .requests
                .borrow()
                .iter()
                .filter(|request| !request.operation.history.is_empty())
                .count(),
            1
        );
        assert_eq!(control.preparations.get(), 1);
        assert_eq!(control.effects.get(), 1);
        assert_eq!(
            rig.storage_control.commits.get(),
            0,
            "explicit joint selection must not write through TaskStorage"
        );
        assert!(
            control.requests.borrow().len() >= 4,
            "creation, reservation, continuation and completion all use joint commits"
        );
        rig.host.close().await?;
        assert_eq!(rig.storage_control.commits.get(), 0);
        rig.backend.close().await
    });
}

#[test]
fn lost_or_corrupt_joint_replies_recover_committed_history_without_replaying_preparation() {
    run(|driver| async move {
        for reply in [Reply::Lost, Reply::Corrupt] {
            let directory = tempfile::tempdir().unwrap();
            let control = Rc::new(Control::default());
            control.reply.set(reply);
            let rig = open(directory.path(), driver.clone(), control.clone()).await?;
            rig.tasks.create(task()).await?;
            loop {
                match rig.tasks.resume().await {
                    Err(error) if error.reason == "durable_tasks_stopped" => break,
                    Err(error) => return Err(error),
                    Ok(_) => tokio::time::sleep(Duration::from_millis(2)).await,
                }
            }
            let record = rig
                .storage
                .get(TaskLookup { id: "task".into() })
                .await?
                .unwrap();
            assert_eq!(record.phase, "effect");
            assert_eq!(record.checkpoint, json!({"prepared":true}));
            assert_eq!(record.status, TaskStatus::Running);
            assert!(record.outcome.is_none());
            assert_eq!(
                rig.sessions
                    .events(EventQuery::new("chat"))
                    .await?
                    .events
                    .len(),
                1
            );
            assert_eq!(control.preparations.get(), 1);
            assert_eq!(control.effects.get(), 0);
            rig.host.close().await?;
            assert_eq!(
                rig.storage_control.commits.get(),
                0,
                "reservation restoration must also use the selected joint path"
            );
            rig.backend.close().await?;
            drop(rig);
            let resumed = open(directory.path(), driver.clone(), control.clone()).await?;
            assert!(matches!(
                resumed.tasks.wait(TaskLookup { id: "task".into() }).await?,
                TaskOutcome::Completed { .. }
            ));
            assert_eq!(
                resumed
                    .sessions
                    .events(EventQuery::new("chat"))
                    .await?
                    .events
                    .len(),
                1
            );
            assert_eq!(control.preparations.get(), 1);
            assert_eq!(control.effects.get(), 1);
            assert_eq!(
                control
                    .requests
                    .borrow()
                    .iter()
                    .filter(|request| !request.operation.history.is_empty())
                    .count(),
                1,
                "reopen resumes authoritative progress instead of reconstructing a new joint request"
            );
            resumed.host.close().await?;
            resumed.backend.close().await?;
        }
        Ok(())
    });
}

#[test]
fn cancellation_supersedes_staged_history_before_the_joint_decision_commit() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let control = Rc::new(Control::default());
        control.held.set(true);
        let rig = open(directory.path(), driver, control.clone()).await?;
        rig.tasks.create(task()).await?;
        while !control.entered.get() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let intent = rig.tasks.cancel(TaskLookup { id: "task".into() }).await?;
        assert!(intent.cancellation_requested);
        control.release.cancel(RuntimeError::aborted(
            "fixture_release",
            "settle the held preparation",
        ));
        assert!(matches!(
            rig.tasks.wait(TaskLookup { id: "task".into() }).await?,
            TaskOutcome::Aborted { .. }
        ));
        assert!(
            rig.sessions
                .events(EventQuery::new("chat"))
                .await?
                .events
                .is_empty()
        );
        assert!(
            control
                .requests
                .borrow()
                .iter()
                .all(|request| request.operation.history.is_empty())
        );
        assert_eq!(control.effects.get(), 0);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn an_uncommitted_unknown_reply_recovers_the_old_checkpoint_before_the_effect() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let control = Rc::new(Control::default());
        control.reply.set(Reply::Uncommitted);
        let rig = open(directory.path(), driver.clone(), control.clone()).await?;
        rig.tasks.create(task()).await?;
        loop {
            match rig.tasks.resume().await {
                Err(error) if error.reason == "durable_tasks_stopped" => break,
                Err(error) => return Err(error),
                Ok(_) => tokio::time::sleep(Duration::from_millis(2)).await,
            }
        }
        let record = rig
            .storage
            .get(TaskLookup { id: "task".into() })
            .await?
            .unwrap();
        assert_eq!(record.phase, "prepare");
        assert_eq!(record.checkpoint, json!({}));
        assert!(
            rig.sessions
                .events(EventQuery::new("chat"))
                .await?
                .events
                .is_empty()
        );
        assert_eq!(control.effects.get(), 0);
        rig.host.close().await?;
        rig.backend.close().await?;
        drop(rig);
        let resumed = open(directory.path(), driver, control.clone()).await?;
        assert!(matches!(
            resumed.tasks.wait(TaskLookup { id: "task".into() }).await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(control.preparations.get(), 2);
        assert_eq!(control.effects.get(), 1);
        assert_eq!(
            resumed
                .sessions
                .events(EventQuery::new("chat"))
                .await?
                .events
                .len(),
            1
        );
        resumed.host.close().await?;
        resumed.backend.close().await
    });
}

#[test]
fn malformed_staged_history_faults_only_its_task_before_any_history_mutation() {
    run(|driver| async move {
        for (malformed, refusal, provider_calls) in [
            ("empty_id", "session_store_invalid", 0),
            ("oversized", "session_store_too_large", 0),
            ("oversized_stored", "session_store_too_large", 1),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let control = Rc::new(Control::default());
            let rig = open(directory.path(), driver.clone(), control.clone()).await?;
            let mut malformed_task = task();
            malformed_task.input = json!({"malformed":malformed});
            rig.tasks.create(malformed_task).await?;
            let outcome = rig.tasks.wait(TaskLookup { id: "task".into() }).await?;
            let TaskOutcome::Faulted { error } = outcome else {
                panic!("known history refusal must fault the task: {outcome:?}");
            };
            assert!(error.contains(refusal), "{error}");
            let record = rig
                .storage
                .get(TaskLookup { id: "task".into() })
                .await?
                .unwrap();
            assert_eq!(record.phase, "prepare");
            assert_eq!(record.checkpoint, json!({}));
            assert_eq!(control.effects.get(), 0);
            assert!(
                rig.sessions
                    .events(EventQuery::new("chat"))
                    .await?
                    .events
                    .is_empty()
            );
            assert_eq!(
                control
                    .requests
                    .borrow()
                    .iter()
                    .filter(|request| !request.operation.history.is_empty())
                    .count(),
                provider_calls,
                "local refusals stay local; stored-event refusal must exercise the native rollback"
            );

            // Exercise the same scheduler after the refusal, rather than merely inspecting the
            // faulted record: a global scheduler failure would reject this independent task.
            let mut peer = task();
            peer.id = "peer".into();
            peer.input = json!({"peer":true});
            rig.tasks.create(peer).await?;
            assert_eq!(
                rig.tasks.wait(TaskLookup { id: "peer".into() }).await?,
                TaskOutcome::Completed {
                    value: json!({"peer":true})
                }
            );
            assert!(
                rig.sessions
                    .events(EventQuery::new("chat"))
                    .await?
                    .events
                    .is_empty()
            );
            assert_eq!(rig.storage_control.commits.get(), 0);
            rig.host.close().await?;
            rig.backend.close().await?;
        }
        Ok(())
    });
}
