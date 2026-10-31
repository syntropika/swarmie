//! Controlled restart through PluginManager, ordinary domain calls and real SQLite.

use futures::{FutureExt, channel::oneshot, executor::LocalPool, future::LocalBoxFuture};
use std::{cell::RefCell, num::NonZeroU32, num::NonZeroU64, path::Path, rc::Rc};
use swarmie_atomic_journal as journal;
use swarmie_durable_tasks::{
    Definitions, Execution, Method, Options, TaskDefinition, TaskId, TaskMethod, TaskRecord,
    TaskSpec, TaskStatus, TaskStorage,
};
use swarmie_plugin_manager::{
    Catalog, CorePlugin, InstalledPlugin, PluginChange, PluginId, PluginStatus, PluginStorage,
};
use swarmie_runtime::Runtime;
use swarmie_storage::sqlite::{SqliteOwner, SqliteTaskStorage, SqliteWrite};
use tembra::{Context, Host, Orb, Reference, Service};

const DOCUMENTS: &str = "fixture.recovery_documents";

struct Journal(SqliteOwner);

impl journal::Journal for Journal {
    fn append(&self, context: Context, entry: String) -> LocalBoxFuture<'_, tembra::Result<()>> {
        async move {
            self.0.stage(
                &context,
                SqliteWrite::new(move |transaction| {
                    transaction
                        .execute("INSERT INTO recovery_documents(entry) VALUES(?1)", [entry])
                        .map_err(|error| {
                            tembra::Error::invalid(
                                "fixture.recovery_documents.write",
                                error.to_string(),
                            )
                        })?;
                    Ok(())
                }),
            )
        }
        .boxed_local()
    }
}

struct Control {
    seen: RefCell<Vec<u64>>,
    pause_after: Option<u64>,
    reached: RefCell<Option<oneshot::Sender<()>>>,
}

impl Control {
    fn new(pause_after: Option<u64>) -> (Rc<Self>, oneshot::Receiver<()>) {
        let (send, receive) = oneshot::channel();
        (
            Rc::new(Self {
                seen: RefCell::new(Vec::new()),
                pause_after,
                reached: RefCell::new(Some(send)),
            }),
            receive,
        )
    }
}

fn definition() -> TaskDefinition {
    TaskDefinition::new(DOCUMENTS, NonZeroU32::new(1).unwrap()).unwrap()
}

/// The task knows only its execution and the ordinary journal service.
fn documents(host: Host, control: Rc<Control>) -> Orb {
    Orb::new(DOCUMENTS, 1).service(
        Service::new(DOCUMENTS, move |_| Ok((host.clone(), control.clone()))).method(
            TaskMethod::Execute.as_str(),
            |environment, context, execution: Execution| async move {
                let total = execution.input().as_u64().ok_or_else(|| {
                    tembra::Error::invalid("fixture.recovery_documents.input", "expected a count")
                })?;
                let start = execution
                    .checkpoint()
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                let reference = Rc::new(environment.0.get(
                    journal::SERVICE,
                    vec![execution.id().to_string()],
                    &context.principal()?,
                )?);
                for index in start + 1..=total {
                    let reference = reference.clone();
                    let entry = format!("{}:{index}", execution.id());
                    execution
                        .commit_with(&context, move |context| {
                            async move {
                                reference
                                    .call::<_, ()>(
                                        &context,
                                        journal::Method::Append.as_str(),
                                        (entry,),
                                    )
                                    .await?;
                                Ok(index.into())
                            }
                            .boxed_local()
                        })
                        .await?;
                    environment.1.seen.borrow_mut().push(index);
                    if environment.1.pause_after == Some(index) {
                        if let Some(send) = environment.1.reached.borrow_mut().take() {
                            let _ = send.send(());
                        }
                        execution.cancelled().await;
                        execution.check(&context)?;
                    }
                }
                Ok(swarmie_durable_tasks::TaskOutcome::Completed(
                    serde_json::Value::from(total),
                ))
            },
        ),
    )
}

fn plugin(name: &str) -> InstalledPlugin {
    InstalledPlugin {
        id: PluginId::new(name).unwrap(),
        version: NonZeroU32::new(1).unwrap(),
        enabled: true,
    }
}

fn open(file: &Path, control: Rc<Control>) -> (LocalPool, Runtime, SqliteOwner) {
    let mut pool = LocalPool::new();
    let spawner = pool.spawner();
    let owner = SqliteOwner::new(file);
    let tasks = owner.clone();
    let journal = owner.clone();
    let catalog = Catalog::new([
        CorePlugin::new(
            plugin("tasks").id,
            NonZeroU32::new(1).unwrap(),
            move |context| {
                Ok(vec![swarmie_durable_tasks::orb(
                    context.host,
                    tasks.task_factory(),
                    Definitions::from_plugins(context.index),
                    Options::new(
                        spawner.clone(),
                        NonZeroU64::new(30_000).unwrap(),
                        Rc::new(|duration| {
                            async move { futures_timer::Delay::new(duration).await }.boxed_local()
                        }),
                    ),
                )])
            },
        ),
        CorePlugin::new(
            plugin("journal").id,
            NonZeroU32::new(1).unwrap(),
            move |_| Ok(vec![journal::orb(Rc::new(Journal(journal.clone())))]),
        ),
        CorePlugin::new(
            plugin("documents").id,
            NonZeroU32::new(1).unwrap(),
            move |context| Ok(vec![documents(context.host, control.clone())]),
        )
        .requires([plugin("journal").id])
        .contributes(definition()),
    ])
    .unwrap();
    let runtime = pool
        .run_until(Runtime::open(
            pool.spawner(),
            owner.inventory_factory(),
            catalog,
        ))
        .unwrap();
    let snapshot = pool.run_until(runtime.plugins()).unwrap();
    assert_eq!(snapshot.plugins.len(), 3);
    assert!(snapshot.plugins.iter().all(|plugin| {
        matches!(plugin.observed, PluginStatus::Mounted { .. }) && !plugin.restart_required
    }));
    assert_eq!(runtime.host().kernel().installed_orbs().len(), 4);
    (pool, runtime, owner)
}

fn tasks(host: &Host) -> Rc<Reference> {
    Rc::new(
        host.get(swarmie_durable_tasks::SERVICE, vec!["main".into()], "host")
            .unwrap(),
    )
}

async fn call<A: 'static, R: 'static>(
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: Method,
    arguments: A,
) -> tembra::Result<R> {
    let reference = reference.clone();
    host.root(principal, move |context| {
        async move { reference.call(&context, method.as_str(), arguments).await }.boxed_local()
    })
    .await
}

fn entries(file: &Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(file).unwrap();
    connection
        .prepare("SELECT entry FROM recovery_documents ORDER BY entry")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn plugin_manager_restores_atomic_document_progress_across_a_controlled_restart() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let seed = SqliteOwner::new(&file).inventory().unwrap();
    futures::executor::block_on(
        seed.commit(
            0,
            ["tasks", "documents", "journal"]
                .map(|name| PluginChange::Put(plugin(name)))
                .to_vec(),
        ),
    )
    .unwrap();
    futures::executor::block_on(seed.close()).unwrap();
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch("CREATE TABLE recovery_documents(entry TEXT PRIMARY KEY);")
        .unwrap();

    let (first, reached) = Control::new(Some(2));
    let (mut pool, runtime, owner) = open(&file, first.clone());
    let host = runtime.host().clone();
    let reference = tasks(&host);
    let id: TaskId = pool
        .run_until(call(
            &host,
            &reference,
            "alice",
            Method::Create,
            (TaskSpec {
                id: TaskId::new(),
                definition: definition(),
                input: 4.into(),
            },),
        ))
        .unwrap();
    pool.run_until(call::<_, ()>(&host, &reference, "host", Method::Resume, ()))
        .unwrap();
    pool.run_until(reached).unwrap();
    let expected: Vec<_> = (1..=4).map(|index| format!("{id}:{index}")).collect();
    assert_eq!(*first.seen.borrow(), [1, 2]);
    assert_eq!(entries(&file), expected[..2]);
    let running: Option<TaskRecord> = pool
        .run_until(call(&host, &reference, "alice", Method::Get, (id,)))
        .unwrap();
    assert_eq!(running.as_ref().unwrap().checkpoint, Some(2.into()));
    assert!(running.unwrap().assignment.is_some());

    pool.run_until(runtime.close()).unwrap();
    assert!(owner.authority().check().is_err());
    assert_eq!(host.kernel().diagnostics().calls, 0);
    assert_eq!(host.kernel().diagnostics().resources, 0);
    assert_eq!(host.kernel().context_stats().0, 0);
    let stored = SqliteTaskStorage::open(&file).unwrap();
    let record = pool.run_until(stored.get(id)).unwrap().unwrap();
    assert_eq!(record.checkpoint, Some(2.into()));
    assert_eq!(record.status, TaskStatus::Ready);
    assert!(record.assignment.is_none());
    pool.run_until(stored.close()).unwrap();
    drop((reference, host, owner, pool));

    let (second, _) = Control::new(None);
    let (mut pool, runtime, owner) = open(&file, second.clone());
    let reference = tasks(runtime.host());
    pool.run_until(reference.ready()).unwrap();
    pool.run_until_stalled();
    assert!(
        second.seen.borrow().is_empty(),
        "recovery waits for explicit resume"
    );
    assert_eq!(entries(&file), expected[..2]);
    pool.run_until(call::<_, ()>(
        runtime.host(),
        &reference,
        "host",
        Method::Resume,
        (),
    ))
    .unwrap();
    pool.run_until_stalled();
    assert_eq!(*second.seen.borrow(), [3, 4]);
    assert_eq!(
        *first.seen.borrow(),
        [1, 2],
        "the old handler remains settled"
    );
    assert_eq!(entries(&file), expected);
    pool.run_until(runtime.close()).unwrap();
    assert!(owner.authority().check().is_err());
    let stored = SqliteTaskStorage::open(&file).unwrap();
    let record = pool.run_until(stored.get(id)).unwrap().unwrap();
    assert_eq!(record.status, TaskStatus::Completed(4.into()));
    assert_eq!(record.checkpoint, Some(4.into()));
    assert!(record.assignment.is_none());
    pool.run_until(stored.close()).unwrap();
}
