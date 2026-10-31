use futures::{
    FutureExt,
    executor::{LocalPool, block_on},
    future::LocalBoxFuture,
};
use std::{
    cell::{Cell, RefCell},
    num::NonZeroU32,
    rc::Rc,
};
use swarmie_foundation_counter as counter;
use swarmie_plugin_manager::{
    ADMIN_PRINCIPAL, BlockReason, Catalog, CommitOutcome, CorePlugin, INSTANCE_KEY,
    InstalledPlugin, Inventory, MountFailure, PluginChange, PluginId, PluginSnapshot, PluginStatus,
    PluginStorage, SERVICE, StorageError, StorageFactory,
};
use swarmie_runtime::Runtime;
use swarmie_storage::sqlite::SqliteStorage;
use tembra::{Code, Error, Orb};

fn id(name: &str) -> PluginId {
    PluginId::new(name).unwrap()
}

fn version(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn desired(name: &str) -> InstalledPlugin {
    InstalledPlugin {
        id: id(name),
        version: version(1),
        enabled: true,
    }
}

fn compiled(name: &str, create: impl Fn() -> tembra::Result<Vec<Orb>> + 'static) -> CorePlugin {
    CorePlugin::new(id(name), version(1), move |_| create())
}

fn counter_catalog() -> Catalog {
    Catalog::new([compiled("counter", || Ok(vec![counter::orb()]))]).unwrap()
}

fn status<'a>(snapshot: &'a PluginSnapshot, name: &str) -> &'a PluginStatus {
    &snapshot
        .plugins
        .iter()
        .find(|plugin| plugin.id == id(name))
        .unwrap()
        .observed
}

fn seed(file: &std::path::Path, records: Vec<InstalledPlugin>) {
    let storage = SqliteStorage::open(file).unwrap();
    block_on(async {
        assert!(matches!(
            storage
                .commit(0, records.into_iter().map(PluginChange::Put).collect())
                .await
                .unwrap(),
            CommitOutcome::Applied { .. }
        ));
        storage.close().await.unwrap();
    });
}

#[test]
fn empty_bootstrap_has_only_the_manager_and_managed_resources_close() {
    let directory = tempfile::tempdir().unwrap();
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(
            spawner,
            swarmie_storage::sqlite::factory(directory.path().join("runtime.sqlite")),
            Catalog::default(),
        )
        .await
        .unwrap();
        let host = runtime.host().clone();
        assert_eq!(host.kernel().installed_orbs().len(), 1);
        assert_eq!(host.kernel().installed_orbs()[0].declaration.name, SERVICE);
        assert!(runtime.plugins().await.unwrap().plugins.is_empty());
        assert_eq!(host.kernel().diagnostics().resources, 2);
        runtime.close().await.unwrap();
        assert_eq!(host.kernel().diagnostics().resources, 0);
        assert_eq!(host.kernel().diagnostics().objects, 0);
    });
}

#[test]
fn startup_isolates_factory_and_installation_failures_and_blocks_dependents() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    seed(
        &file,
        vec![
            desired("bad"),
            desired("dependent"),
            desired("counter"),
            desired("invalid"),
            desired("other"),
        ],
    );
    let catalog = Catalog::new([
        compiled("bad", || {
            Err(Error::invalid(
                "fixture.failure",
                "injected factory failure",
            ))
        }),
        compiled("dependent", || Ok(vec![Orb::new("dependent", 1)])).requires([id("bad")]),
        compiled("counter", || Ok(vec![counter::orb()])),
        compiled("invalid", || Ok(vec![Orb::new("", 1)])),
        compiled("other", || Ok(vec![Orb::new("other", 1)])),
    ])
    .unwrap();
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime=Runtime::open(spawner,swarmie_storage::sqlite::factory(&file),catalog).await.unwrap();
        let snapshot=runtime.plugins().await.unwrap();
        assert!(matches!(status(&snapshot,"bad"),PluginStatus::Failed { failure: MountFailure::Factory { .. } }));
        assert!(matches!(status(&snapshot,"invalid"),PluginStatus::Failed { failure: MountFailure::Installation { .. } }));
        assert!(matches!(status(&snapshot,"dependent"),PluginStatus::Blocked { reason: BlockReason::Dependencies { plugins } } if plugins == &[id("bad")]));
        assert!(matches!(status(&snapshot,"counter"),PluginStatus::Mounted { .. }));
        assert!(matches!(status(&snapshot,"other"),PluginStatus::Mounted { .. }));
        assert_eq!(runtime.host().kernel().installed_orbs().len(),3);
        let reference=Rc::new(runtime.host().get(counter::SERVICE,vec!["main".into()],"test").unwrap());
        let count: u64=runtime.host().root("test",move |context| async move {
            reference.call(&context,counter::Method::Increment.as_str(),(4_u64,)).await
        }.boxed_local()).await.unwrap();
        assert_eq!(count,4);
        runtime.close().await.unwrap();
    });
    let reopened = SqliteStorage::open(&file).unwrap();
    assert!(
        block_on(reopened.inventory())
            .unwrap()
            .plugins
            .iter()
            .all(|plugin| plugin.enabled)
    );
    block_on(reopened.close()).unwrap();
}

#[test]
fn startup_orders_dependencies_and_cycles_do_not_stop_unrelated_plugins() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    seed(
        &file,
        vec![
            desired("a-consumer"),
            desired("z-provider"),
            desired("cycle-a"),
            desired("cycle-b"),
        ],
    );
    let order = Rc::new(RefCell::new(Vec::new()));
    let provider = order.clone();
    let consumer = order.clone();
    let catalog = Catalog::new([
        compiled("a-consumer", move || {
            consumer.borrow_mut().push("consumer");
            Ok(vec![Orb::new("consumer", 1)])
        })
        .requires([id("z-provider")]),
        compiled("z-provider", move || {
            provider.borrow_mut().push("provider");
            Ok(vec![Orb::new("provider", 1)])
        }),
        compiled("cycle-a", || Ok(vec![Orb::new("cycle-a", 1)])).requires([id("cycle-b")]),
        compiled("cycle-b", || Ok(vec![Orb::new("cycle-b", 1)])).requires([id("cycle-a")]),
    ])
    .unwrap();
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(spawner, swarmie_storage::sqlite::factory(&file), catalog)
            .await
            .unwrap();
        assert_eq!(&*order.borrow(), &["provider", "consumer"]);
        let snapshot = runtime.plugins().await.unwrap();
        assert!(matches!(
            status(&snapshot, "cycle-a"),
            PluginStatus::Blocked { .. }
        ));
        assert!(matches!(
            status(&snapshot, "cycle-b"),
            PluginStatus::Blocked { .. }
        ));
        runtime.close().await.unwrap();
    });
}

#[test]
fn configuration_is_durable_and_native_disable_takes_effect_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(
            spawner.clone(),
            swarmie_storage::sqlite::factory(&file),
            counter_catalog(),
        )
        .await
        .unwrap();
        let before = runtime.plugins().await.unwrap();
        let enabled = runtime.configure(desired("counter")).await.unwrap();
        assert!(before.plugins.is_empty());
        assert!(matches!(
            status(&enabled, "counter"),
            PluginStatus::Mounted { .. }
        ));
        runtime.close().await.unwrap();
        let runtime = Runtime::open(
            spawner.clone(),
            swarmie_storage::sqlite::factory(&file),
            counter_catalog(),
        )
        .await
        .unwrap();
        assert!(matches!(
            status(&runtime.plugins().await.unwrap(), "counter"),
            PluginStatus::Mounted { .. }
        ));
        let mut disabled = desired("counter");
        disabled.enabled = false;
        let changed = runtime.configure(disabled).await.unwrap();
        assert!(matches!(
            status(&changed, "counter"),
            PluginStatus::Mounted { .. }
        ));
        assert!(changed.plugins[0].restart_required);
        runtime.close().await.unwrap();
        let runtime = Runtime::open(
            spawner,
            swarmie_storage::sqlite::factory(&file),
            counter_catalog(),
        )
        .await
        .unwrap();
        let snapshot = runtime.plugins().await.unwrap();
        assert_eq!(status(&snapshot, "counter"), &PluginStatus::Disabled);
        assert!(!snapshot.plugins[0].restart_required);
        assert_eq!(runtime.host().kernel().installed_orbs().len(), 1);
        runtime.close().await.unwrap();
    });
}

#[test]
fn revision_conflict_preserves_index_then_refresh_adopts_confirmed_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(
            spawner,
            swarmie_storage::sqlite::factory(&file),
            counter_catalog(),
        )
        .await
        .unwrap();
        let other = SqliteStorage::open(&file).unwrap();
        other
            .commit(0, vec![PluginChange::Put(desired("counter"))])
            .await
            .unwrap();
        let result = runtime.configure(desired("counter")).await.unwrap_err();
        assert_eq!(result.code, Code::Conflict);
        assert!(runtime.plugins().await.unwrap().plugins.is_empty());
        let snapshot = runtime.refresh().await.unwrap();
        assert_eq!(snapshot.inventory_revision, 1);
        assert!(matches!(
            status(&snapshot, "counter"),
            PluginStatus::Mounted { .. }
        ));
        other
            .commit(1, vec![PluginChange::Remove(id("counter"))])
            .await
            .unwrap();
        let snapshot = runtime.refresh().await.unwrap();
        assert!(snapshot.plugins[0].desired.is_none());
        assert!(snapshot.plugins[0].restart_required);
        assert!(matches!(
            snapshot.plugins[0].observed,
            PluginStatus::Mounted { .. }
        ));
        other.close().await.unwrap();
        runtime.close().await.unwrap();
    });
}

#[derive(Default)]
struct MemoryStorage {
    owner: swarmie_storage::StorageOwner,
    closes: Cell<u32>,
    writes: Cell<u32>,
    fail_read: Cell<bool>,
    unknown_commit: Cell<bool>,
}

impl PluginStorage for MemoryStorage {
    fn owner(&self) -> swarmie_storage::StorageOwner {
        self.owner.clone()
    }
    fn inventory(&self) -> LocalBoxFuture<'_, Result<Inventory, StorageError>> {
        async move {
            if self.fail_read.get() {
                return Err(StorageError::Unavailable("injected read failure".into()));
            }
            Ok(Inventory {
                revision: 0,
                plugins: vec![],
            })
        }
        .boxed_local()
    }

    fn commit(
        &self,
        _: u64,
        _: Vec<PluginChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            self.writes.set(self.writes.get() + 1);
            Ok(if self.unknown_commit.get() {
                CommitOutcome::Unknown {
                    message: "injected uncertain acknowledgement".into(),
                }
            } else {
                CommitOutcome::Applied { revision: 1 }
            })
        }
        .boxed_local()
    }

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move {
            self.closes.set(self.closes.get() + 1);
            Ok(())
        }
        .boxed_local()
    }
}

fn memory_factory(storage: Rc<MemoryStorage>) -> StorageFactory {
    Rc::new(move || {
        let storage = storage.clone();
        async move { Ok(storage as Rc<dyn PluginStorage>) }.boxed_local()
    })
}

#[test]
fn unreadable_storage_aborts_startup_and_closes_the_partial_resource_graph() {
    let storage = Rc::new(MemoryStorage::default());
    storage.fail_read.set(true);
    let mut executor = LocalPool::new();
    let result = executor.run_until(Runtime::open(
        executor.spawner(),
        memory_factory(storage.clone()),
        Catalog::default(),
    ));
    assert!(result.is_err());
    assert_eq!(storage.closes.get(), 1);
    assert_eq!(storage.writes.get(), 0);
}

#[test]
fn uncertain_commit_freezes_mutations_without_adopting_or_repeating_it() {
    let storage = Rc::new(MemoryStorage::default());
    storage.unknown_commit.set(true);
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(spawner, memory_factory(storage.clone()), counter_catalog())
            .await
            .unwrap();
        assert_eq!(
            runtime
                .configure(desired("counter"))
                .await
                .unwrap_err()
                .code,
            Code::Unavailable
        );
        assert!(runtime.configure(desired("counter")).await.is_err());
        assert!(runtime.refresh().await.is_err());
        assert_eq!(storage.writes.get(), 1);
        assert!(runtime.plugins().await.unwrap().plugins.is_empty());
        assert_eq!(runtime.host().kernel().installed_orbs().len(), 1);
        runtime.close().await.unwrap();
        assert_eq!(storage.closes.get(), 1);
    });
}

#[test]
fn unauthorized_and_duplicate_manager_addresses_cannot_open_another_store() {
    let storage = Rc::new(MemoryStorage::default());
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(spawner, memory_factory(storage.clone()), Catalog::default())
            .await
            .unwrap();
        assert!(
            runtime
                .host()
                .get(SERVICE, vec![INSTANCE_KEY.into()], "untrusted")
                .is_err()
        );
        assert!(
            runtime
                .host()
                .get(SERVICE, vec!["another".into()], ADMIN_PRINCIPAL)
                .is_err()
        );
        assert_eq!(runtime.host().kernel().diagnostics().resources, 2);
        runtime.close().await.unwrap();
        assert_eq!(storage.closes.get(), 1);
    });
}

#[test]
fn required_plugins_seed_once_include_dependencies_and_remain_managed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.sqlite");
    for _ in 0..2 {
        let mut executor = LocalPool::new();
        let catalog = Catalog::new([
            swarmie_runtime::node_plugin(swarmie_runtime::NodeId::new("embedded-node").unwrap()),
            compiled("counter", || Ok(vec![counter::orb()]))
                .requires([id(swarmie_runtime::NODE_SERVICE)]),
        ])
        .unwrap()
        .require([id("counter")])
        .unwrap();
        let index = catalog.index();
        assert!(!index.required_active());
        let spawner = executor.spawner();
        executor.run_until(async {
            let runtime = Runtime::open(spawner, swarmie_storage::sqlite::factory(&path), catalog)
                .await
                .unwrap();
            let snapshot = runtime.plugins().await.unwrap();
            assert_eq!(snapshot.inventory_revision, 1);
            assert_eq!(snapshot.plugins.len(), 2);
            assert!(
                snapshot
                    .plugins
                    .iter()
                    .all(|plugin| matches!(plugin.observed, PluginStatus::Mounted { .. }))
            );
            assert!(index.required_active());
            let identity = runtime
                .host()
                .get(
                    swarmie_runtime::NODE_SERVICE,
                    vec![swarmie_runtime::NODE_KEY.into()],
                    "caller",
                )
                .unwrap();
            let value: swarmie_runtime::NodeIdentity = runtime
                .host()
                .root("caller", move |context| {
                    async move { identity.call(&context, "identify", ()).await }.boxed_local()
                })
                .await
                .unwrap();
            assert_eq!(value.node_id.as_str(), "embedded-node");
            assert_eq!(value.principal, "caller");
            let host = runtime.host().clone();
            runtime.close().await.unwrap();
            assert!(!index.required_active());
            assert_eq!(host.kernel().diagnostics().resources, 0);
        });
    }
}

#[test]
fn existing_missing_disabled_or_incompatible_required_inventory_is_preserved() {
    for record in [
        desired("unrelated"),
        InstalledPlugin {
            enabled: false,
            ..desired("counter")
        },
        InstalledPlugin {
            version: version(2),
            ..desired("counter")
        },
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime.sqlite");
        seed(&path, vec![record]);
        let storage = SqliteStorage::open(&path).unwrap();
        let before = block_on(storage.inventory()).unwrap();
        block_on(storage.close()).unwrap();
        let mut executor = LocalPool::new();
        let spawner = executor.spawner();
        let catalog = counter_catalog().require([id("counter")]).unwrap();
        assert!(
            executor
                .run_until(Runtime::open(
                    spawner,
                    swarmie_storage::sqlite::factory(&path),
                    catalog
                ))
                .is_err()
        );
        let storage = SqliteStorage::open(&path).unwrap();
        assert_eq!(block_on(storage.inventory()).unwrap(), before);
        block_on(storage.close()).unwrap();
    }
}

#[test]
fn required_policy_refuses_mutation_and_invalidates_cached_eligibility_on_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.sqlite");
    let catalog = counter_catalog().require([id("counter")]).unwrap();
    let index = catalog.index();
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(spawner, swarmie_storage::sqlite::factory(&path), catalog)
            .await
            .unwrap();
        for record in [
            InstalledPlugin {
                enabled: false,
                ..desired("counter")
            },
            InstalledPlugin {
                version: version(2),
                ..desired("counter")
            },
        ] {
            assert!(runtime.configure(record).await.is_err());
            assert_eq!(runtime.plugins().await.unwrap().inventory_revision, 1);
            assert!(index.required_active());
        }
        let other = SqliteStorage::open(&path).unwrap();
        other
            .commit(1, vec![PluginChange::Remove(id("counter"))])
            .await
            .unwrap();
        assert!(runtime.refresh().await.is_err());
        assert!(!index.required_active());
        assert!(other.inventory().await.unwrap().plugins.is_empty());
        other.close().await.unwrap();
        runtime.close().await.unwrap();
    });
}

#[test]
fn shared_owner_uncertainty_synchronously_invalidates_required_eligibility() {
    let directory = tempfile::tempdir().unwrap();
    let owner = swarmie_storage::sqlite::SqliteOwner::new(directory.path().join("shared.sqlite"));
    let catalog = counter_catalog().require([id("counter")]).unwrap();
    let index = catalog.index();
    let mut executor = LocalPool::new();
    let spawner = executor.spawner();
    executor.run_until(async {
        let runtime = Runtime::open(spawner, owner.inventory_factory(), catalog)
            .await
            .unwrap();
        let changes = Rc::new(Cell::new(0));
        let captured = changes.clone();
        let _subscription = index
            .changes()
            .subscribe(move || captured.set(captured.get() + 1));
        assert!(index.required_active());
        owner.authority().require_recovery();
        assert!(!index.required_active());
        assert_eq!(changes.get(), 1);
        assert!(runtime.refresh().await.is_err());
        assert!(runtime.configure(desired("optional")).await.is_err());
        assert!(!index.required_active());
        assert_eq!(changes.get(), 1);
        assert_eq!(runtime.plugins().await.unwrap().inventory_revision, 1);
        let host = runtime.host().clone();
        runtime.close().await.unwrap();
        assert_eq!(host.kernel().diagnostics().resources, 0);
        assert_eq!(changes.get(), 1);
    });
}

#[test]
fn detected_inventory_conflict_requires_confirmed_refresh_before_new_work() {
    for remove_required in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("inventory.sqlite");
        let catalog = counter_catalog().require([id("counter")]).unwrap();
        let index = catalog.index();
        let mut executor = LocalPool::new();
        let spawner = executor.spawner();
        executor.run_until(async {
            let runtime = Runtime::open(spawner, swarmie_storage::sqlite::factory(&file), catalog)
                .await
                .unwrap();
            let other = SqliteStorage::open(&file).unwrap();
            let change = if remove_required {
                PluginChange::Remove(id("counter"))
            } else {
                PluginChange::Put(desired("optional"))
            };
            other.commit(1, vec![change]).await.unwrap();
            let committed = other.inventory().await.unwrap();
            assert_eq!(
                runtime
                    .configure(desired("candidate"))
                    .await
                    .unwrap_err()
                    .code,
                Code::Conflict
            );
            assert!(!index.required_active());
            assert_eq!(other.inventory().await.unwrap(), committed);
            if remove_required {
                assert!(runtime.refresh().await.is_err());
                assert!(!index.required_active());
            } else {
                assert_eq!(runtime.refresh().await.unwrap().inventory_revision, 2);
                assert!(index.required_active());
            }
            runtime.close().await.unwrap();
            other.close().await.unwrap();
        });
    }
}

#[test]
fn unknown_initial_inventory_is_not_replayed_and_closes_storage() {
    let storage = Rc::new(MemoryStorage::default());
    storage.unknown_commit.set(true);
    let mut executor = LocalPool::new();
    let catalog = counter_catalog().require([id("counter")]).unwrap();
    let index = catalog.index();
    let result = executor.run_until(Runtime::open(
        executor.spawner(),
        memory_factory(storage.clone()),
        catalog,
    ));
    assert!(result.is_err());
    assert_eq!(storage.writes.get(), 1);
    assert_eq!(storage.closes.get(), 1);
    assert!(storage.owner.check().is_err());
    assert!(!index.required_active());
}

#[test]
fn required_mount_failure_aborts_runtime_and_closes_its_partial_host() {
    let storage = Rc::new(MemoryStorage::default());
    let captured = Rc::new(RefCell::new(None));
    let target = captured.clone();
    let catalog = Catalog::new([CorePlugin::new(
        id("required"),
        version(1),
        move |context| {
            *target.borrow_mut() = Some(context.host);
            Err(Error::invalid("fixture", "required factory failed"))
        },
    )])
    .unwrap()
    .require([id("required")])
    .unwrap();
    let mut executor = LocalPool::new();
    assert!(
        executor
            .run_until(Runtime::open(
                executor.spawner(),
                memory_factory(storage.clone()),
                catalog
            ))
            .is_err()
    );
    assert_eq!(storage.closes.get(), 1);
    assert_eq!(
        captured
            .borrow()
            .as_ref()
            .unwrap()
            .kernel()
            .diagnostics()
            .resources,
        0
    );
}
