use futures::{FutureExt, executor::LocalPool};
use std::num::{NonZeroU32, NonZeroU64};
use swarmie_durable_tasks::*;
use swarmie_storage_sqlite::{SqliteOwner, SqliteWrite};
use tembra::{Code, Host};

#[test]
fn foreign_owner_and_unknown_command_types_are_refused_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("joint.sqlite");
    let first = SqliteOwner::new(file.clone());
    let second = SqliteOwner::new(file.clone());
    let tasks = first.tasks().unwrap();
    let other_tasks = second.tasks().unwrap();
    let connection = rusqlite::Connection::open(file).unwrap();
    connection
        .execute_batch("CREATE TABLE journal(entry TEXT);")
        .unwrap();
    let mut pool = LocalPool::new();
    let spec = TaskSpec {
        id: TaskId::new(),
        definition: TaskDefinition::new("fixture.task", NonZeroU32::new(1).unwrap()).unwrap(),
        input: 0.into(),
    };
    let id = spec.id;
    let attempt = AttemptId::new();
    assert!(matches!(
        pool.run_until(tasks.commit(vec![
            TaskChange::Create {
                spec,
                principal: "alice".into()
            },
            TaskChange::Acquire {
                id,
                revision: 0,
                attempt,
                executor: ExecutorId::new(),
                lease_ms: NonZeroU64::new(30_000).unwrap()
            },
        ]))
        .unwrap(),
        CommitOutcome::Applied { .. }
    ));
    let checkpoint = || TaskChange::Checkpoint {
        id,
        revision: 0,
        attempt,
        checkpoint: 7.into(),
    };
    let host = Host::new(pool.spawner());
    let owner = first.clone();
    let prepared = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                let scope = owner.authority().prepare(&ctx)?;
                owner.stage(
                    scope.context(),
                    SqliteWrite::new(|tx| {
                        tx.execute("INSERT INTO journal VALUES('foreign')", [])
                            .unwrap();
                        Ok(())
                    }),
                )?;
                scope.seal()
            }
            .boxed_local()
        }))
        .unwrap();
    // Equal paths do not confer equal in-process storage ownership.
    let error = pool
        .run_until(other_tasks.commit_with(vec![checkpoint()], prepared))
        .unwrap_err();
    assert!(matches!(error, StorageError::Refused(e) if e.code == Code::Forbidden));
    let owner = first.authority();
    let prepared = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                let scope = owner.prepare(&ctx)?;
                owner.stage(scope.context(), 7_u64)?;
                scope.seal()
            }
            .boxed_local()
        }))
        .unwrap();
    let error = pool
        .run_until(tasks.commit_with(vec![checkpoint()], prepared))
        .unwrap_err();
    assert!(matches!(error, StorageError::Refused(e) if e.code == Code::Unsupported));
    assert_eq!(
        pool.run_until(tasks.get(id)).unwrap().unwrap().checkpoint,
        None
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM journal", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert!(first.authority().check().is_ok());
    pool.run_until(host.close()).unwrap();
    pool.run_until(tasks.close()).unwrap();
    pool.run_until(other_tasks.close()).unwrap();
}
