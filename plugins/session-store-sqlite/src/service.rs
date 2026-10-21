use crate::{Configuration, FACET_ID, database::Store};
use futures::FutureExt;
use swarmie_session_contracts::*;
use tembra::{core, prelude::*};

/// Declares a selectable Core Plugin with one eagerly opened, lifecycle-owned SQLite Resource.
///
/// # Errors
/// Invalid configuration fails immediately; lock, schema and filesystem failures reject the
/// installation waiter. Closing the Host retains every event, session and idempotency receipt.
pub fn facet(configuration: &Configuration) -> Result<NativeFacet> {
    configuration.validate()?;
    let database = configuration.database.clone();
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let database = database.clone();
        setup.serve_with_options(
            SessionStore,
            move |_, _, declaration| {
                let database = database.clone();
                let resource = declaration.core_mut().resource(
                    "sqlite",
                    ResourceDeclaration::new(move |_| {
                        let database = database.clone();
                        async move { Store::open(&database) }.boxed_local()
                    })
                    .with_close(|store| async move { store.close() }.boxed_local()),
                )?;
                let initial = resource.clone();
                let committed = declaration.state_with(
                    "committed",
                    StateDeclaration::with_initializer(move |_| {
                        let resource = initial.clone();
                        async move { resource.value()?.head() }
                    })?,
                )?;
                Ok(Service {
                    resource,
                    committed,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Service {
    resource: Resource<Store>,
    committed: Cell<StoreHead>,
}

impl Service {
    // There is no await between commit and State adoption. A publication error has a committed
    // outcome and must never trigger a fresh append identity. Reconnect always reads SQLite.
    fn publish(&self, context: &Context) -> Result<()> {
        let head = self.resource.value()?.head()?;
        if head != self.committed.value()? {
            self.committed.replace(context, head).map_err(|error| {
                RuntimeError::unavailable(
                    "session_store_publication",
                    format!("storage committed; recover using the identical request: {error}"),
                )
            })?;
        }
        Ok(())
    }
}

impl SessionStoreProvider for Service {
    fn committed(&self) -> Readable<StoreHead> {
        self.committed.readable()
    }

    async fn create(&self, request: CreateSession, context: Context) -> Result<SessionRecord> {
        let record = self.resource.value()?.create(request)?;
        self.publish(&context)?;
        Ok(record)
    }

    async fn open(
        &self,
        request: SessionLookup,
        context: Context,
    ) -> Result<Option<SessionRecord>> {
        let record = self.resource.value()?.get(request)?;
        self.publish(&context)?;
        Ok(record)
    }

    async fn append(&self, request: AppendEvents, context: Context) -> Result<AppendReceipt> {
        let receipt = self.resource.value()?.append(request)?;
        self.publish(&context)?;
        Ok(receipt)
    }

    async fn events(&self, query: EventQuery, context: Context) -> Result<EventPage> {
        let page = self.resource.value()?.events(query)?;
        self.publish(&context)?;
        Ok(page)
    }

    async fn project(&self, query: ProjectionQuery, context: Context) -> Result<ProjectionPage> {
        // Projection wrappers are slightly larger than source wrappers. Reserve 1 KiB before
        // source admission so a returned projection always fits the original requested budget.
        let budget = query.source.max_bytes as usize;
        let mut source = query.source;
        if source.max_bytes < 5120 {
            return Err(RuntimeError::invalid(
                "session_store_invalid",
                "projection byte budget requires at least 5120",
            ));
        }
        source.max_bytes -= 1024;
        let page = project(self.resource.value()?.events(source)?, query.kind);
        bounded(&page, budget)?;
        self.publish(&context)?;
        Ok(page)
    }
}
