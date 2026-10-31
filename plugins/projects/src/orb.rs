use crate::{Command, Cursor, Page, Revision, StorageFactory, model::invalid, projection::Encoded};
use futures::FutureExt;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tembra::{Context, Orb, Resource, Service, State};

pub const SERVICE: &str = "swarmie.projects";
pub const INSTANCE_KEY: &str = "main";

type StorageResource = Resource<Rc<dyn crate::Storage>>;
type StorageSlot = Rc<RefCell<Option<StorageResource>>>;

struct Environment {
    storage: StorageResource,
    state: State<Revision>,
    pages: RefCell<Option<Encoded>>,
    latest_revision: Cell<u64>,
    principal: String,
}

impl Environment {
    fn authorize(&self, context: &Context) -> tembra::Result<()> {
        context.check()?;
        if context.principal()? != self.principal {
            return Err(tembra::Error::new(
                tembra::Code::Forbidden,
                "swarmie.projects.principal",
                "unbound principal",
            ));
        }
        Ok(())
    }

    async fn list(&self, context: &Context) -> tembra::Result<Page> {
        self.authorize(context)?;
        let current = self.storage.value()?.load(&self.principal).await?;
        current.validate()?;
        if current.revision < self.latest_revision.get() {
            return Err(stale());
        }
        let encoded = Encoded {
            revision: current.revision,
            data: serde_json::to_string(&current)
                .map_err(|_| invalid("snapshot could not be encoded"))?,
        };
        let first = encoded.page(0)?;
        self.latest_revision.set(current.revision);
        *self.pages.borrow_mut() = Some(encoded);
        let _ = self.state.update(context, |_| {
            Ok(Revision {
                revision: current.revision,
            })
        });
        Ok(first)
    }

    fn page(&self, context: &Context, cursor: Cursor) -> tembra::Result<Page> {
        self.authorize(context)?;
        let pages = self.pages.borrow();
        let encoded = pages
            .as_ref()
            .filter(|encoded| encoded.revision == cursor.revision)
            .ok_or_else(stale)?;
        encoded.page(cursor.offset)
    }

    async fn change(&self, context: &Context, command: Command) -> tembra::Result<Revision> {
        self.authorize(context)?;
        let storage = self.storage.value()?;
        let current = storage.load(&self.principal).await?;
        let revision = current.revision;
        let next = current.apply(command)?;
        context.check()?;
        let outcome = storage
            .replace(&self.principal, revision, next.clone())
            .await;
        // Even an unknown commit invalidates the previously cached observation.
        self.pages.borrow_mut().take();
        outcome?;
        self.latest_revision
            .set(self.latest_revision.get().max(next.revision));
        let receipt = Revision {
            revision: next.revision,
        };
        // A confirmed storage result survives cancelled or failed State observation.
        let _ = self.state.update(context, |_| Ok(receipt.clone()));
        Ok(receipt)
    }
}

/// Compose an independently usable capability. The embedding selects permitted principals.
/// The orb owns storage independently of UI observers; Tembra closes its storage lease.
/// Other plugins publish through ordinary references with the same authenticated principal.
pub fn orb(
    storage: StorageFactory,
    authorize: impl Fn(&str) -> tembra::Result<()> + 'static,
) -> Orb {
    let shared: StorageSlot = Rc::new(RefCell::new(None));
    let declared = shared.clone();
    let orb = Orb::new(SERVICE, 1).create(move |declaration| {
        let factory = storage.clone();
        let resource = declaration.resource_async(
            "projects.storage",
            1,
            &[],
            move |_| async move { factory().await.map_err(Into::into) }.boxed_local(),
            |storage, _| async move { storage.close().await.map_err(Into::into) }.boxed_local(),
        )?;
        *declared.borrow_mut() = Some(resource);
        Ok(())
    });
    let service =
        Service::new(SERVICE, move |declaration| {
            let address = declaration.address()?;
            authorize(&address.principal)?;
            if address.key != [INSTANCE_KEY] {
                return Err(invalid("this service uses the main key"));
            }
            let resource = shared
                .borrow()
                .clone()
                .ok_or_else(|| invalid("storage is not declared"))?;
            let state = declaration.state(Revision::default(), 1, |_| Ok(()))?;
            let storage = declaration.resource_async(
                "projects.access",
                1,
                &[resource.key()],
                move |_| async move { Ok(resource.value()?.as_ref().clone()) }.boxed_local(),
                |_, _| async { Ok(()) }.boxed_local(),
            )?;
            Ok(Environment {
                storage,
                state,
                pages: RefCell::new(None),
                latest_revision: Cell::new(0),
                principal: address.principal,
            })
        })
        .method("list", |environment, context, (): ()| async move {
            environment.list(&context).await
        })
        .method(
            "page",
            |environment, context, (cursor,): (Cursor,)| async move {
                environment.page(&context, cursor)
            },
        )
        .method(
            "change",
            |environment, context, (command,): (Command,)| async move {
                environment.change(&context, command).await
            },
        );
    orb.service(service)
}

fn stale() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Conflict,
        "projection.revision",
        "read snapshot changed; restart the read",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Snapshot, Storage, StorageError};
    use futures::{executor::LocalPool, future::LocalBoxFuture};

    #[derive(Default)]
    struct ConfirmedStore(RefCell<Snapshot>, Cell<bool>);

    impl Storage for ConfirmedStore {
        fn load<'a>(&'a self, _: &'a str) -> LocalBoxFuture<'a, Result<Snapshot, StorageError>> {
            async { Ok(self.0.borrow().clone()) }.boxed_local()
        }

        fn replace<'a>(
            &'a self,
            _: &'a str,
            expected: u64,
            next: Snapshot,
        ) -> LocalBoxFuture<'a, Result<(), StorageError>> {
            async move {
                assert_eq!(self.0.borrow().revision, expected);
                *self.0.borrow_mut() = next;
                if self.1.replace(false) {
                    return Err(StorageError::Unknown(
                        "injected lost commit confirmation".into(),
                    ));
                }
                Ok(())
            }
            .boxed_local()
        }

        fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
            async { Ok(()) }.boxed_local()
        }
    }

    #[test]
    fn confirmed_commit_survives_state_failure_and_unknown_commit_invalidates_cached_pages() {
        let mut pool = LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        let store = Rc::new(ConfirmedStore::default());
        let captured = store.clone();
        host.install(vec![
            Orb::new("fixture", 1).service(
                Service::new("fixture", move |declaration| {
                    let store = captured.clone();
                    let storage = declaration.resource(
                        "fixture.storage",
                        1,
                        move || Ok(store as Rc<dyn Storage>),
                        |_, _| async { Ok(()) }.boxed_local(),
                    )?;
                    let state = declaration.state(Revision::default(), 1, |value| {
                        if value.revision > 0 {
                            Err(invalid("injected observation rejection"))
                        } else {
                            Ok(())
                        }
                    })?;
                    Ok(Environment {
                        storage,
                        state,
                        pages: RefCell::new(None),
                        latest_revision: Cell::new(0),
                        principal: "alice".into(),
                    })
                })
                .method("list", |env, context, (): ()| async move {
                    env.list(&context).await
                })
                .method("page", |env, context, (cursor,): (Cursor,)| async move {
                    env.page(&context, cursor)
                })
                .method(
                    "change",
                    |env, context, (command,): (Command,)| async move {
                        env.change(&context, command).await
                    },
                ),
            ),
        ])
        .unwrap();
        let reference = Rc::new(host.get("fixture", vec!["main".into()], "alice").unwrap());
        pool.run_until(reference.ready()).unwrap();
        let observed = reference.state::<Revision>().unwrap();
        let invocation = reference.clone();
        let receipt: Revision = pool
            .run_until(host.root("alice", move |context| {
                async move {
                    invocation
                        .call(
                            &context,
                            "change",
                            (Command::Create {
                                id: "one".into(),
                                name: "One".into(),
                                description: String::new(),
                            },),
                        )
                        .await
                }
                .boxed_local()
            }))
            .unwrap();
        assert_eq!(receipt.revision, 1);
        assert_eq!(store.0.borrow().revision, 1);
        assert_eq!(observed.value().unwrap().revision, 0);

        let invocation = reference.clone();
        let page: Page = pool
            .run_until(host.root("alice", move |context| {
                async move { invocation.call(&context, "list", ()).await }.boxed_local()
            }))
            .unwrap();
        store.1.set(true);
        let invocation = reference.clone();
        let unknown: tembra::Result<Revision> =
            pool.run_until(host.root("alice", move |context| {
                async move {
                    invocation
                        .call(
                            &context,
                            "change",
                            (Command::Update {
                                id: "one".into(),
                                name: "Updated".into(),
                                description: String::new(),
                            },),
                        )
                        .await
                }
                .boxed_local()
            }));
        assert!(unknown.is_err());
        assert_eq!(store.0.borrow().revision, 2);
        let invocation = reference.clone();
        let stale: tembra::Result<Page> = pool.run_until(host.root("alice", move |context| {
            async move {
                invocation
                    .call(
                        &context,
                        "page",
                        (Cursor {
                            revision: page.revision,
                            offset: 0,
                        },),
                    )
                    .await
            }
            .boxed_local()
        }));
        assert_eq!(stale.unwrap_err().code, tembra::Code::Conflict);
        pool.run_until(host.close()).unwrap();
    }
}
