#![cfg(feature = "durable-tasks")]

mod support;

use futures::{FutureExt, executor::LocalPool};
use std::{num::NonZeroU64, rc::Rc};
use support::{Scenario, agent, request};
use swarmie_agent_loop::{durable, *};
use swarmie_durable_tasks::{
    self as tasks, Definitions, Options, TaskId, TaskRecord, TaskSpec, TaskStatus,
};
use swarmie_storage_sqlite::{SqliteOwner, SqliteWrite};
use tembra::{Host, Reference};

fn open(pool: &LocalPool, owner: SqliteOwner, scenario: Rc<Scenario>) -> (Host, Rc<Reference>) {
    open_with_prepare(pool, owner, scenario, None)
}

fn open_with_prepare(
    pool: &LocalPool,
    owner: SqliteOwner,
    scenario: Rc<Scenario>,
    prepare: Option<durable::Prepare>,
) -> (Host, Rc<Reference>) {
    let host = Host::new(pool.spawner());
    let create = support::factory(&host, scenario);
    let task = if let Some(prepare) = prepare {
        tembra::Orb::new(durable::SERVICE, 1).service(
            tembra::Service::new(durable::SERVICE, move |declaration| {
                Ok((create(declaration)?, prepare.clone()))
            })
            .method(
                tasks::TaskMethod::Execute.as_str(),
                |env, ctx, execution: tasks::Execution| async move {
                    serde_json::to_value(
                        durable::execute_with(&env.0, &ctx, &execution, Some(env.1.clone()))
                            .await?,
                    )
                    .map(tasks::TaskOutcome::Completed)
                    .map_err(|error| tembra::Error::invalid("fixture.result", error.to_string()))
                },
            ),
        )
    } else {
        durable::orb(create)
    };
    host.install(vec![
        tasks::orb(
            host.clone(),
            owner.task_factory(),
            Definitions::new([durable::definition()]).unwrap(),
            Options::new(
                pool.spawner(),
                NonZeroU64::new(60_000).unwrap(),
                Rc::new(|duration| {
                    async move { futures_timer::Delay::new(duration).await }.boxed_local()
                }),
            ),
        ),
        task,
    ])
    .unwrap();
    let reference = Rc::new(
        host.get(
            tasks::SERVICE,
            vec![tasks::INSTANCE_KEY.into()],
            tasks::HOST_PRINCIPAL,
        )
        .unwrap(),
    );
    (host, reference)
}

fn call<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: tasks::Method,
    arguments: A,
) -> R {
    let reference = reference.clone();
    pool.run_until(host.root(principal, move |ctx| {
        async move { reference.call(&ctx, method.as_str(), arguments).await }.boxed_local()
    }))
    .unwrap()
}

fn create(pool: &mut LocalPool, host: &Host, reference: &Rc<Reference>) -> TaskId {
    call(
        pool,
        host,
        reference,
        "alice",
        tasks::Method::Create,
        (TaskSpec {
            id: TaskId::new(),
            definition: durable::definition(),
            input: serde_json::to_value(request()).unwrap(),
        },),
    )
}

fn resume(pool: &mut LocalPool, host: &Host, reference: &Rc<Reference>) {
    call::<_, ()>(pool, host, reference, "host", tasks::Method::Resume, ());
    pool.run_until_stalled();
}

fn record(pool: &mut LocalPool, host: &Host, reference: &Rc<Reference>, id: TaskId) -> TaskRecord {
    call::<_, Option<TaskRecord>>(pool, host, reference, "alice", tasks::Method::Get, (id,))
        .unwrap()
}

#[test]
fn same_engine_produces_the_same_result_through_sqlite_tasks() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let record = record(&mut pool, &host, &reference, id);
    let TaskStatus::Completed(value) = record.status else {
        panic!("task did not complete")
    };
    let durable_output: Output = serde_json::from_value(value).unwrap();
    assert!(record.assignment.is_none());
    let state: State = serde_json::from_value(record.checkpoint.unwrap()).unwrap();
    assert!(matches!(state.phase(), Phase::Finished { .. }));
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();

    let host = Host::new(pool.spawner());
    let standalone = agent(&host, Rc::new(Scenario::default()));
    let ordinary_output = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(standalone.execute(&ctx, request()).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(durable_output, ordinary_output);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn invalid_stored_progress_is_refused_before_provider_recovery_or_dispatch() {
    use tasks::{AttemptId, CommitOutcome, ExecutorId, TaskChange, TaskStorage};

    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(directory.path().join("tasks.sqlite"));
    let storage = owner.tasks().unwrap();
    let input = request();
    let mut checkpoint = serde_json::to_value(State::start(input.clone()).unwrap()).unwrap();
    checkpoint["turn"] = serde_json::json!(u32::MAX);
    let id = TaskId::new();
    let attempt = AttemptId::new();
    let outcome = pool
        .run_until(storage.commit(vec![
            TaskChange::Create {
                spec: TaskSpec {
                    id,
                    definition: durable::definition(),
                    input: serde_json::to_value(input).unwrap(),
                },
                principal: "alice".into(),
            },
            TaskChange::Acquire {
                id,
                revision: 0,
                executor: ExecutorId::new(),
                attempt,
                lease_ms: NonZeroU64::new(60_000).unwrap(),
            },
            TaskChange::Checkpoint {
                id,
                attempt,
                revision: 0,
                checkpoint: checkpoint.clone(),
            },
            TaskChange::Release { id, attempt },
        ]))
        .unwrap();
    assert!(matches!(outcome, CommitOutcome::Applied { .. }));
    let scenario = Rc::new(Scenario::default());
    let (host, reference) = open(&pool, owner, scenario.clone());
    resume(&mut pool, &host, &reference);
    let rejected = record(&mut pool, &host, &reference, id);
    let TaskStatus::Failed(error) = rejected.status else {
        panic!("invalid progress must fail")
    };
    assert_eq!(error.code, tembra::Code::Invalid);
    assert_eq!(error.reason, FailureReason::InvalidInput.as_str());
    assert_eq!(rejected.checkpoint, Some(checkpoint));
    assert!(rejected.assignment.is_none());
    assert!(scenario.models.borrow().is_empty());
    assert!(scenario.effects.borrow().is_empty());
    assert!(scenario.reconciled_models.borrow().is_empty());
    assert!(scenario.reconciled_tools.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
    pool.run_until(storage.close()).unwrap();
}

#[test]
fn restart_reconciles_a_lost_tool_reply_without_repeating_the_effect() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite");
    let scenario = Rc::new(Scenario::default());
    scenario.uncertain_tool.set(true);
    let (host, reference) = open(&pool, SqliteOwner::new(path.clone()), scenario.clone());
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let interrupted = record(&mut pool, &host, &reference, id);
    assert_eq!(interrupted.status, TaskStatus::Ready);
    assert!(interrupted.assignment.is_none());
    let state: State = serde_json::from_value(interrupted.checkpoint.unwrap()).unwrap();
    let Phase::ToolPending { operation, .. } = state.phase() else {
        panic!("missing tool intent")
    };
    assert_eq!(scenario.effects.borrow().as_slice(), &[*operation]);
    assert_eq!(scenario.models.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().calls, 0);

    let (host, reference) = open(&pool, SqliteOwner::new(path), scenario.clone());
    resume(&mut pool, &host, &reference);
    assert!(matches!(
        record(&mut pool, &host, &reference, id).status,
        TaskStatus::Completed(_)
    ));
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.reconciled_tools.borrow().as_slice(), &[*operation]);
    assert_eq!(scenario.models.borrow().len(), 2);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn unknown_tool_outcome_stays_pending_even_after_an_explicit_wake() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    scenario.uncertain_tool.set(true);
    scenario.unknown_tool.set(true);
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let before = record(&mut pool, &host, &reference, id);
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    let after = record(&mut pool, &host, &reference, id);
    assert_eq!(after.status, TaskStatus::Ready);
    assert_eq!(before.checkpoint, after.checkpoint);
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn a_lost_model_reply_is_recovered_without_reissuing_the_request() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    scenario.uncertain_model.set(true);
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let before = record(&mut pool, &host, &reference, id);
    let state: State = serde_json::from_value(before.checkpoint.unwrap()).unwrap();
    let Phase::ModelPending { operation, .. } = state.phase() else {
        panic!("missing model intent")
    };
    assert!(scenario.effects.borrow().is_empty());
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    assert!(matches!(
        record(&mut pool, &host, &reference, id).status,
        TaskStatus::Completed(_)
    ));
    assert_eq!(scenario.models.borrow().len(), 2);
    assert_eq!(
        scenario.reconciled_models.borrow().as_slice(),
        &[*operation]
    );
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn refused_intent_never_dispatches_and_can_resume_from_the_prior_confirmation() {
    for before_model in [true, false] {
        let mut pool = LocalPool::new();
        let directory = tempfile::tempdir().unwrap();
        let scenario = Rc::new(Scenario::default());
        let reject = Rc::new(std::cell::Cell::new(true));
        let gate = reject.clone();
        let prepare: durable::Prepare = Rc::new(move |_, state| {
            let reject = gate.clone();
            async move {
                let selected = if before_model {
                    matches!(state.phase(), Phase::ModelPending { .. })
                } else {
                    matches!(state.phase(), Phase::ToolPending { .. })
                };
                if selected && reject.replace(false) {
                    return Err(tembra::Error::new(
                        tembra::Code::Unavailable,
                        "fixture.refused",
                        "intent refused",
                    ));
                }
                Ok(())
            }
            .boxed_local()
        });
        let (host, reference) = open_with_prepare(
            &pool,
            SqliteOwner::new(directory.path().join("tasks.sqlite")),
            scenario.clone(),
            Some(prepare),
        );
        let id = create(&mut pool, &host, &reference);
        resume(&mut pool, &host, &reference);
        assert_eq!(scenario.models.borrow().len(), usize::from(!before_model));
        assert!(scenario.effects.borrow().is_empty());
        assert_eq!(
            record(&mut pool, &host, &reference, id).status,
            TaskStatus::Ready
        );
        call::<_, ()>(
            &mut pool,
            &host,
            &reference,
            "host",
            tasks::Method::Wake,
            (),
        );
        pool.run_until_stalled();
        assert!(matches!(
            record(&mut pool, &host, &reference, id).status,
            TaskStatus::Completed(_)
        ));
        assert_eq!(scenario.effects.borrow().len(), 1);
        pool.run_until(host.close()).unwrap();
    }
}

#[test]
fn tool_result_and_domain_writes_roll_back_together_then_reconcile() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite");
    let owner = SqliteOwner::new(path.clone());
    let scenario = Rc::new(Scenario::default());
    let reject = Rc::new(std::cell::Cell::new(true));
    let writer = owner.clone();
    let prepare: durable::Prepare = Rc::new(move |ctx, state| {
        let owner = writer.clone();
        let reject = reject.clone();
        async move {
            let fail = matches!(state.phase(), Phase::PrepareTurn) && reject.replace(false);
            let payload = serde_json::to_string(&state).unwrap();
            owner.stage(
                &ctx,
                SqliteWrite::new(move |tx| {
                    tx.execute_batch("CREATE TABLE IF NOT EXISTS progress (payload TEXT NOT NULL)")
                        .unwrap();
                    tx.execute("INSERT INTO progress(payload) VALUES(?1)", [payload])
                        .unwrap();
                    if fail {
                        return Err(tembra::Error::new(
                            tembra::Code::Unavailable,
                            "fixture.refused",
                            "joint transaction refused",
                        ));
                    }
                    Ok(())
                }),
            )
        }
        .boxed_local()
    });
    let (host, reference) = open_with_prepare(&pool, owner, scenario.clone(), Some(prepare));
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let record_before = record(&mut pool, &host, &reference, id);
    let before: State = serde_json::from_value(record_before.checkpoint.unwrap()).unwrap();
    assert!(matches!(before.phase(), Phase::ToolPending { .. }));
    let connection = rusqlite::Connection::open(&path).unwrap();
    let rows: u64 = connection
        .query_row("SELECT COUNT(*) FROM progress", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        rows, 3,
        "the refused result batch did not publish its domain row"
    );
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    let after = record(&mut pool, &host, &reference, id);
    assert!(matches!(after.status, TaskStatus::Completed(_)));
    let payload: String = connection
        .query_row(
            "SELECT payload FROM progress ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        after.checkpoint.unwrap()
    );
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn provider_authorized_retry_reuses_the_pending_operation_identity() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    scenario.not_started.set(true);
    scenario.safe_retry.set(true);
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    assert!(scenario.effects.borrow().is_empty());
    let state: State =
        serde_json::from_value(record(&mut pool, &host, &reference, id).checkpoint.unwrap())
            .unwrap();
    let Phase::ToolPending { operation, .. } = state.phase() else {
        panic!("missing intent")
    };
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    assert!(matches!(
        record(&mut pool, &host, &reference, id).status,
        TaskStatus::Completed(_)
    ));
    assert_eq!(scenario.effects.borrow().as_slice(), &[*operation]);
    assert_eq!(scenario.reconciled_tools.borrow().as_slice(), &[*operation]);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn shutdown_joins_the_physical_tool_before_releasing_its_assignment() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite");
    let scenario = Rc::new(Scenario::default());
    let (send, receive) = futures::channel::oneshot::channel();
    *scenario.tool_gate.borrow_mut() = Some(receive);
    let (host, reference) = open(&pool, SqliteOwner::new(path.clone()), scenario.clone());
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    assert!(scenario.tool_entered.get());
    assert!(
        record(&mut pool, &host, &reference, id)
            .assignment
            .is_some()
    );
    let mut closing = Box::pin(host.close());
    pool.run_until(async {
        assert!(futures::poll!(closing.as_mut()).is_pending());
    });
    pool.run_until_stalled();
    assert!(scenario.effects.borrow().is_empty());
    send.send(()).unwrap();
    pool.run_until(closing).unwrap();
    assert_eq!(host.kernel().diagnostics().calls, 0);
    assert_eq!(host.kernel().diagnostics().resources, 0);
    let stored = swarmie_storage_sqlite::SqliteTaskStorage::open(path).unwrap();
    let record = pool
        .run_until(tasks::TaskStorage::get(&stored, id))
        .unwrap()
        .unwrap();
    assert_eq!(record.status, TaskStatus::Ready);
    assert!(record.assignment.is_none());
    let state: State = serde_json::from_value(record.checkpoint.unwrap()).unwrap();
    assert!(matches!(state.phase(), Phase::ToolPending { .. }));
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(tasks::TaskStorage::close(&stored)).unwrap();
}

#[test]
fn batch_recovery_keeps_the_first_confirmed_tool_result_and_reconciles_only_the_second() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    scenario.tool_count.set(2);
    scenario.uncertain_at.set(2);
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let state: State =
        serde_json::from_value(record(&mut pool, &host, &reference, id).checkpoint.unwrap())
            .unwrap();
    let Phase::ToolPending {
        operation, next, ..
    } = state.phase()
    else {
        panic!("missing second tool intent")
    };
    assert_eq!(*next, 1);
    assert_eq!(state.messages().len(), 2);
    assert_eq!(scenario.effects.borrow().len(), 2);
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    let TaskStatus::Completed(value) = record(&mut pool, &host, &reference, id).status else {
        panic!("task did not complete")
    };
    let output: Output = serde_json::from_value(value).unwrap();
    assert_eq!(output.messages.len(), 4);
    assert_eq!(scenario.effects.borrow().len(), 2);
    assert_eq!(scenario.reconciled_tools.borrow().as_slice(), &[*operation]);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn completed_tool_is_reconciled_even_when_dispatch_validation_now_blocks() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    scenario.uncertain_tool.set(true);
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    scenario.block.set(true);
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    assert!(matches!(
        record(&mut pool, &host, &reference, id).status,
        TaskStatus::Completed(_)
    ));
    assert_eq!(scenario.effects.borrow().len(), 1);
    assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn provider_authorized_retry_still_requires_current_dispatch_permission() {
    let mut pool = LocalPool::new();
    let directory = tempfile::tempdir().unwrap();
    let scenario = Rc::new(Scenario::default());
    scenario.not_started.set(true);
    scenario.safe_retry.set(true);
    let (host, reference) = open(
        &pool,
        SqliteOwner::new(directory.path().join("tasks.sqlite")),
        scenario.clone(),
    );
    let id = create(&mut pool, &host, &reference);
    resume(&mut pool, &host, &reference);
    let before = record(&mut pool, &host, &reference, id);
    scenario.block.set(true);
    call::<_, ()>(
        &mut pool,
        &host,
        &reference,
        "host",
        tasks::Method::Wake,
        (),
    );
    pool.run_until_stalled();
    let after = record(&mut pool, &host, &reference, id);
    assert_eq!(after.status, TaskStatus::Ready);
    assert_eq!(before.checkpoint, after.checkpoint);
    assert!(scenario.effects.borrow().is_empty());
    assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}
