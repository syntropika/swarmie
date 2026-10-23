use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use futures::{FutureExt, lock::Mutex};
use swarmie_session_contracts::{SessionStore, SessionStoreInput};
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::{core, prelude::*};

use crate::{
    Configuration, FACET_ID, SharedConfiguration,
    admission::{Bindings, destination_identity, source_identity},
    publisher::Publisher,
    store::Store,
};

/// Select the bridge as a normal compiled Tembra Facet. Stores must already be selected and
/// their exact durable IDs supplied. Eager Resources open the binding database and register
/// the stable Facet-owned consumer. Readiness refuses a replacement store before consuming.
///
/// # Errors
/// Invalid configuration fails without installation. SQLite lock/schema/pin failures, missing
/// dependencies and source consumer ownership conflicts reject the installation waiter.
/// Closing cancels publication and closes SQLite while preserving intent, history and ACKs.
pub fn facet(configuration: &Configuration, driver: Rc<dyn core::Driver>) -> Result<NativeFacet> {
    configuration.validate()?;
    facet_impl(
        &configuration.shared(),
        driver,
        DatabaseSelection::Standalone(configuration.clone()),
    )
}

/// Uses the selected physical SQLite owner for bindings while preserving ordinary admission,
/// consumer ownership and ordered publication. Register [`crate::sqlite_schema`] on that owner.
/// Store identities and the consumer remain pinned; file selection belongs to the owner.
///
/// # Errors
/// Invalid configuration fails before installation. Missing owner/schema, mismatched pins or
/// selected stores and consumer conflicts reject readiness. Closing this Facet revokes its
/// binding Resource; Tembra closes the physical database after its dependent Resources.
pub fn facet_with_database(
    database: &swarmie_sqlite_local::DatabaseHandle,
    configuration: &SharedConfiguration,
    driver: Rc<dyn core::Driver>,
) -> Result<NativeFacet> {
    facet_impl(
        configuration,
        driver,
        DatabaseSelection::Shared(database.clone()),
    )
}

#[derive(Clone)]
enum DatabaseSelection {
    Standalone(Configuration),
    Shared(swarmie_sqlite_local::DatabaseHandle),
}

fn binding_resource(
    declaration: &mut Declaration<'_>,
    selection: &DatabaseSelection,
    configuration: &SharedConfiguration,
) -> Result<Resource<Store>> {
    let opening = match selection {
        DatabaseSelection::Standalone(configuration) => {
            let configuration = configuration.clone();
            ResourceDeclaration::new(move |_| {
                let configuration = configuration.clone();
                async move { Store::open(&configuration) }.boxed_local()
            })
        }
        DatabaseSelection::Shared(handle) => {
            let database = handle.declare(declaration)?;
            let dependency = database.dependency();
            let configuration = configuration.clone();
            let mut opening = ResourceDeclaration::new(move |_| {
                let configuration = configuration.clone();
                let database = database.clone();
                async move {
                    Store::open_shared(database.value()?.as_ref().clone(), &configuration)
                }.boxed_local()
            });
            opening.deps.push(dependency);
            opening
        }
    }
    .with_close(|store| async move { store.close() }.boxed_local());
    declaration.core_mut().resource("bindings", opening)
}

fn facet_impl(
    configuration: &SharedConfiguration,
    driver: Rc<dyn core::Driver>,
    database: DatabaseSelection,
) -> Result<NativeFacet> {
    configuration.validate()?;
    let configuration = configuration.clone();
    NativeFacet::new(FACET_ID, vec![], move |scope, setup| {
        let binding_configuration = configuration.clone();
        let binding_database = database.clone();
        setup.serve_with_options(
            TaskSessionBindings,
            move |_, _, declaration| {
                let configuration = binding_configuration.clone();
                let resource = binding_resource(declaration, &binding_database, &configuration)?;
                Ok(BindingService {
                    bindings: Bindings {
                        configuration,
                        resource,
                        storage: declaration.get(TaskStorage, TaskStorageInput {})?,
                        sessions: declaration.get(SessionStore, SessionStoreInput {})?,
                        tasks: declaration.get(DurableTasks, DurableTasksInput {})?,
                    },
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        let configuration = configuration.clone();
        let scope = scope.clone();
        let driver = driver.clone();
        setup.serve_with_options(
            TaskSessionPublisher,
            move |_, _, declaration| {
                let storage = declaration.get(TaskStorage, TaskStorageInput {})?;
                let sessions = declaration.get(SessionStore, SessionStoreInput {})?;
                let bindings = declaration.get(TaskSessionBindings, TaskSessionBindingsInput {})?;
                let opening_storage = storage.clone();
                let opening_sessions = sessions.clone();
                let opening_configuration = configuration.clone();
                let opening_scope = scope.clone();
                let mut startup = ResourceDeclaration::new(move |_| {
                    let storage = opening_storage.clone();
                    let sessions = opening_sessions.clone();
                    let configuration = opening_configuration.clone();
                    opening_scope.root(move |context| {
                        async move {
                            let context = Context::from_core(context);
                            source_identity(&storage, &context, &configuration.task_store_id)
                                .await?;
                            destination_identity(
                                &sessions,
                                &context,
                                &configuration.session_store_id,
                            )
                            .await?;
                            storage
                                .with_context(&context)
                                .register_outbox_consumer(TaskOutboxConsumer {
                                    id: configuration.consumer_id,
                                })
                                .await
                        }
                        .boxed_local()
                    })
                });
                startup
                    .deps
                    .push(core::ResourceDependency::Reference(storage.core().clone()));
                startup
                    .deps
                    .push(core::ResourceDependency::Reference(sessions.core().clone()));
                startup
                    .deps
                    .push(core::ResourceDependency::Reference(bindings.core().clone()));
                let startup = declaration
                    .core_mut()
                    .resource("outbox-consumer", startup)?;
                let publisher = Rc::new(Publisher {
                    configuration: configuration.clone(),
                    storage,
                    sessions,
                    bindings,
                    startup,
                    driver: driver.clone(),
                    gate: Mutex::new(()),
                    failures: Cell::new(0),
                    last_error: RefCell::new(None),
                    missing: RefCell::new(None),
                });
                if configuration.auto_publish {
                    let activity = publisher.clone();
                    declaration
                        .core_mut()
                        .task(move |scope| activity.clone().run(scope).boxed_local())?;
                }
                Ok(PublisherService { publisher })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct BindingService {
    bindings: Bindings,
}

impl TaskSessionBindingsProvider for BindingService {
    async fn prepare(
        &self,
        request: PrepareBinding,
        context: Context,
    ) -> Result<TaskSessionBinding> {
        self.bindings.prepare(request, &context).await
    }
    async fn prepare_child(
        &self,
        request: PrepareChildBinding,
        context: Context,
    ) -> Result<TaskSessionBinding> {
        self.bindings.prepare_child(request, &context).await
    }
    async fn admit(&self, key: BindingKey, context: Context) -> Result<TaskSessionBinding> {
        self.bindings.admit(key, &context).await
    }
    async fn require_confirmed_binding(
        &self,
        key: BindingKey,
        context: Context,
    ) -> Result<TaskSessionBinding> {
        self.bindings.confirm(key, &context).await
    }
    async fn lookup(
        &self,
        key: BindingKey,
        _context: Context,
    ) -> Result<Option<BindingDisposition>> {
        self.bindings.lookup(&key)
    }
    async fn exclude(&self, request: ExcludedBinding, context: Context) -> Result<ExcludedBinding> {
        self.bindings.exclude(request, &context).await
    }
}

struct PublisherService {
    publisher: Rc<Publisher>,
}

impl TaskSessionPublisherProvider for PublisherService {
    async fn publish(&self, request: PublishRequest, context: Context) -> Result<PublishReceipt> {
        self.publisher.publish(request, &context).await
    }
    async fn diagnostics(&self, context: Context) -> Result<PublisherStatus> {
        self.publisher.diagnostics(&context).await
    }
    async fn compact(
        &self,
        request: TaskOutboxCompaction,
        context: Context,
    ) -> Result<TaskOutboxRetention> {
        self.publisher.compact(request, &context).await
    }
}
