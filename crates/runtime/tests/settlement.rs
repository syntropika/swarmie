use futures::{
    FutureExt,
    channel::oneshot,
    executor::LocalPool,
    future::{LocalBoxFuture, join, poll_fn},
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    num::NonZeroU32,
    rc::Rc,
    task::Poll,
};
use swarmie_plugin_manager::{
    ADMIN_PRINCIPAL, Catalog, CommitOutcome, INSTANCE_KEY, InstalledPlugin, Inventory,
    ManagerMethod, PluginChange, PluginId, PluginStorage, SERVICE, StorageError, StorageFactory,
};
use swarmie_runtime::Runtime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    WriteEntered,
    WriteApplied,
    StorageClosed,
}

struct PendingStorage {
    owner: swarmie_storage::StorageOwner,
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    events: RefCell<Vec<Event>>,
    revision: Cell<u64>,
    records: RefCell<Vec<InstalledPlugin>>,
}

impl PluginStorage for PendingStorage {
    fn owner(&self) -> swarmie_storage::StorageOwner {
        self.owner.clone()
    }
    fn inventory(&self) -> LocalBoxFuture<'_, Result<Inventory, StorageError>> {
        async move {
            Ok(Inventory {
                revision: self.revision.get(),
                plugins: self.records.borrow().clone(),
            })
        }
        .boxed_local()
    }

    fn commit(
        &self,
        _: u64,
        changes: Vec<PluginChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            self.events.borrow_mut().push(Event::WriteEntered);
            let gate = self
                .gate
                .borrow_mut()
                .take()
                .expect("only one write may enter");
            gate.await.expect("test releases physical storage work");
            for change in changes {
                match change {
                    PluginChange::Put(record) => self.records.borrow_mut().push(record),
                    PluginChange::Remove(_) => unreachable!("fixture only configures a plugin"),
                }
            }
            self.revision.set(self.revision.get() + 1);
            self.events.borrow_mut().push(Event::WriteApplied);
            Ok(CommitOutcome::Applied {
                revision: self.revision.get(),
            })
        }
        .boxed_local()
    }

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move {
            self.events.borrow_mut().push(Event::StorageClosed);
            Ok(())
        }
        .boxed_local()
    }
}

#[test]
fn shutdown_and_competing_mutations_wait_for_the_physical_storage_operation() {
    let (send, receive) = oneshot::channel();
    let storage = Rc::new(PendingStorage {
        owner: Default::default(),
        gate: RefCell::new(Some(receive)),
        events: RefCell::new(vec![]),
        revision: Cell::new(0),
        records: RefCell::new(vec![]),
    });
    let resource = storage.clone();
    let factory: StorageFactory = Rc::new(move || {
        let resource = resource.clone();
        async move { Ok(resource as Rc<dyn PluginStorage>) }.boxed_local()
    });
    let mut executor = LocalPool::new();
    let runtime = executor
        .run_until(Runtime::open(
            executor.spawner(),
            factory,
            Catalog::default(),
        ))
        .unwrap();
    let host = runtime.host().clone();
    let manager = Rc::new(
        host.get(SERVICE, vec![INSTANCE_KEY.into()], ADMIN_PRINCIPAL)
            .unwrap(),
    );
    let record = InstalledPlugin {
        id: PluginId::new("future.plugin").unwrap(),
        version: NonZeroU32::new(1).unwrap(),
        enabled: true,
    };
    let command = record.clone();
    let mut operation = Box::pin(host.root(ADMIN_PRINCIPAL, move |context| {
        async move {
            manager
                .call::<_, swarmie_plugin_manager::PluginSnapshot>(
                    &context,
                    ManagerMethod::Configure.as_str(),
                    (command,),
                )
                .await
        }
        .boxed_local()
    }));
    executor.run_until(poll_fn(|context| {
        assert!(operation.as_mut().poll(context).is_pending());
        Poll::Ready(())
    }));
    executor.run_until_stalled();
    assert_eq!(*storage.events.borrow(), [Event::WriteEntered]);
    let error = executor
        .run_until(runtime.configure(record.clone()))
        .unwrap_err();
    assert_eq!(error.code, tembra::Code::Conflict);
    let mut closing = Box::pin(runtime.close());
    executor.run_until(poll_fn(|context| {
        assert!(closing.as_mut().poll(context).is_pending());
        Poll::Ready(())
    }));
    executor.run_until_stalled();
    assert_eq!(*storage.events.borrow(), [Event::WriteEntered]);
    send.send(()).unwrap();
    let (operation, closed) = executor.run_until(join(operation, closing));
    operation.unwrap();
    closed.unwrap();
    assert_eq!(
        *storage.events.borrow(),
        [
            Event::WriteEntered,
            Event::WriteApplied,
            Event::StorageClosed
        ]
    );
    assert_eq!(*storage.records.borrow(), [record]);
    assert_eq!(host.kernel().diagnostics().resources, 0);
}
