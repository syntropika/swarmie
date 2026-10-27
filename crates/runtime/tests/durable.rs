use futures::{
    FutureExt,
    executor::{LocalPool, LocalSpawner, block_on},
};
use std::{
    num::{NonZeroU32, NonZeroU64},
    rc::Rc,
};
use swarmie_durable_documents as documents;
use swarmie_durable_tasks::{
    Definitions, Method, Options, TaskId, TaskRecord, TaskSpec, TaskStatus, TaskStorage,
};
use swarmie_plugin_manager::{
    Catalog, CorePlugin, InstalledPlugin, PluginChange, PluginId, PluginStorage,
};
use swarmie_runtime::Runtime;
use swarmie_storage_sqlite::{SqliteOwner, SqliteTaskStorage};
use tembra::{Host, Reference};
fn plugin(name: &str) -> InstalledPlugin {
    InstalledPlugin {
        id: PluginId::new(name).unwrap(),
        version: NonZeroU32::new(1).unwrap(),
        enabled: true,
    }
}
fn catalog(owner: SqliteOwner, spawner: LocalSpawner, control: Rc<documents::Control>) -> Catalog {
    Catalog::new([
        CorePlugin::new(
            PluginId::new("tasks").unwrap(),
            NonZeroU32::new(1).unwrap(),
            move |context| {
                let options = Options::new(
                    spawner.clone(),
                    NonZeroU64::new(300).unwrap(),
                    Rc::new(|duration| {
                        async move { futures_timer::Delay::new(duration).await }.boxed_local()
                    }),
                );
                Ok(vec![swarmie_durable_tasks::orb(
                    context.host,
                    owner.task_factory(),
                    Definitions::from_plugins(context.index),
                    options,
                )])
            },
        ),
        CorePlugin::new(
            PluginId::new("documents").unwrap(),
            NonZeroU32::new(1).unwrap(),
            move |context| {
                Ok(vec![
                    documents::orb(context.host, control.clone()),
                    swarmie_foundation_counter::orb(),
                ])
            },
        )
        .contributes(documents::definition()),
    ])
    .unwrap()
}
async fn call<A: 'static, R: 'static>(
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: Method,
    args: A,
) -> tembra::Result<R> {
    let reference = reference.clone();
    host.root(principal, move |ctx| {
        async move { reference.call(&ctx, method.as_str(), args).await }.boxed_local()
    })
    .await
}
fn tasks(host: &Host) -> Rc<Reference> {
    Rc::new(
        host.get(
            swarmie_durable_tasks::SERVICE,
            vec![swarmie_durable_tasks::INSTANCE_KEY.into()],
            "host",
        )
        .unwrap(),
    )
}
#[test]
fn live_plugin_index_and_shared_owner_recover_a_orbs_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let seed = SqliteOwner::new(file.clone()).inventory().unwrap();
    block_on(seed.commit(0, vec![PluginChange::Put(plugin("tasks"))])).unwrap();
    block_on(seed.close()).unwrap();
    let mut pool = LocalPool::new();
    let spawner = pool.spawner();
    let owner = SqliteOwner::new(file.clone());
    let first = documents::Control::new(Some(2));
    let runtime = pool
        .run_until(Runtime::open(
            spawner.clone(),
            owner.inventory_factory(),
            catalog(owner.clone(), spawner.clone(), first.clone()),
        ))
        .unwrap();
    let host = runtime.host().clone();
    let reference = tasks(&host);
    let id = pool
        .run_until(call::<_, TaskId>(
            &host,
            &reference,
            "alice",
            Method::Create,
            (TaskSpec {
                id: TaskId::new(),
                definition: documents::definition(),
                input: 4.into(),
            },),
        ))
        .unwrap();
    pool.run_until_stalled();
    assert!(first.seen.borrow().is_empty());
    pool.run_until(call::<_, ()>(&host, &reference, "host", Method::Resume, ()))
        .unwrap();
    pool.run_until_stalled();
    assert!(
        first.seen.borrow().is_empty(),
        "missing plugin stays unassigned"
    );
    pool.run_until(runtime.configure(plugin("documents")))
        .unwrap();
    pool.run_until(first.reached());
    assert_eq!(*first.seen.borrow(), vec![1, 2]);
    pool.run_until(runtime.close()).unwrap();
    assert!(owner.authority().check().is_err());
    assert_eq!(host.kernel().diagnostics().resources, 0);
    let persisted = SqliteTaskStorage::open(&file).unwrap();
    let record = pool.run_until(persisted.get(id)).unwrap().unwrap();
    assert_eq!(record.checkpoint, Some(2.into()));
    assert_eq!(record.status, TaskStatus::Ready);
    assert!(record.assignment.is_none());
    pool.run_until(persisted.close()).unwrap();
    let owner = SqliteOwner::new(file);
    let second = documents::Control::new(None);
    let runtime = pool
        .run_until(Runtime::open(
            spawner.clone(),
            owner.inventory_factory(),
            catalog(owner, spawner, second.clone()),
        ))
        .unwrap();
    let reference = tasks(runtime.host());
    pool.run_until(reference.ready()).unwrap();
    assert!(second.seen.borrow().is_empty());
    pool.run_until(call::<_, ()>(
        runtime.host(),
        &reference,
        "host",
        Method::Resume,
        (),
    ))
    .unwrap();
    pool.run_until_stalled();
    assert_eq!(*second.seen.borrow(), vec![3, 4]);
    let record = pool
        .run_until(call::<_, Option<TaskRecord>>(
            runtime.host(),
            &reference,
            "alice",
            Method::Get,
            (id,),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(record.status, TaskStatus::Completed(4.into()));
    assert_eq!(record.spec.input, serde_json::Value::from(4));
    assert!(record.assignment.is_none());
    assert!(
        pool.run_until(call::<_, Option<TaskRecord>>(
            runtime.host(),
            &reference,
            "bob",
            Method::Get,
            (id,)
        ))
        .unwrap()
        .is_none()
    );
    pool.run_until(runtime.close()).unwrap();
}

#[test]
fn corrupt_task_payload_does_not_block_an_unrelated_task() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let seed = SqliteOwner::new(file.clone()).inventory().unwrap();
    block_on(seed.commit(
        0,
        vec![
            PluginChange::Put(plugin("tasks")),
            PluginChange::Put(plugin("documents")),
        ],
    ))
    .unwrap();
    block_on(seed.close()).unwrap();
    let mut pool = LocalPool::new();
    let spawner = pool.spawner();
    let owner = SqliteOwner::new(file.clone());
    let control = documents::Control::new(None);
    let runtime = pool
        .run_until(Runtime::open(
            spawner.clone(),
            owner.inventory_factory(),
            catalog(owner, spawner, control.clone()),
        ))
        .unwrap();
    let reference = tasks(runtime.host());
    let mut ids = vec![];
    for _ in 0..2 {
        ids.push(
            pool.run_until(call::<_, TaskId>(
                runtime.host(),
                &reference,
                "alice",
                Method::Create,
                (TaskSpec {
                    id: TaskId::new(),
                    definition: documents::definition(),
                    input: 1.into(),
                },),
            ))
            .unwrap(),
        );
    }
    let connection = rusqlite::Connection::open(file).unwrap();
    connection
        .execute(
            "UPDATE swarmie_tasks SET data='broken json' WHERE id=?1",
            [ids[0].to_string()],
        )
        .unwrap();
    pool.run_until(call::<_, ()>(
        runtime.host(),
        &reference,
        "host",
        Method::Resume,
        (),
    ))
    .unwrap();
    pool.run_until_stalled();
    let record = pool
        .run_until(call::<_, Option<TaskRecord>>(
            runtime.host(),
            &reference,
            "alice",
            Method::Get,
            (ids[1],),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(record.status, TaskStatus::Completed(1.into()));
    assert_eq!(*control.seen.borrow(), vec![1]);
    pool.run_until(runtime.close()).unwrap();
}
