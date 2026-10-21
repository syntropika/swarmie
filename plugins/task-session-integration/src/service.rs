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
    Configuration, FACET_ID,
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
    let configuration = configuration.clone();
    NativeFacet::new(FACET_ID, vec![], move |scope, setup| {
        let binding_configuration = configuration.clone();
        setup.serve_with_options(
            TaskSessionBindings,
            move |_, _, declaration| {
                let configuration = binding_configuration.clone();
                let opening = configuration.clone();
                let resource = declaration.core_mut().resource(
                    "bindings",
                    ResourceDeclaration::new(move |_| {
                        let configuration = opening.clone();
                        async move { Store::open(&configuration) }.boxed_local()
                    })
                    .with_close(|store| async move { store.close() }.boxed_local()),
                )?;
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
