use futures::FutureExt;
use tembra::{core, prelude::*};

use crate::{
    Configuration, DeleteRequest, FACET_ID, GetRequest, GetResult, ListRequest, MemoryStore,
    MemoryStoreProvider, PutRequest, contract::validate, store::Store,
};

/// Prepares the compiled storage facet without opening a database or mutating a Host.
///
/// The Host eagerly opens one SQLite Resource on installation. Its close callback revokes
/// storage access and closes the connection. Dropping the final resource value also drops its
/// connection; use explicit Host closure for deterministic cleanup with retained References.
/// No database file or rows are deleted by cleanup. Native replacement requires a fresh Host.
///
/// # Errors
/// Returns `memory_store_configuration` for invalid configuration, or a Tembra declaration
/// error. Filesystem, locking and storage errors are reported by the installation startup waiter.
pub fn facet(configuration: &Configuration) -> Result<NativeFacet> {
    configuration
        .validate()
        .map_err(|error| RuntimeError::invalid("memory_store_configuration", error.to_string()))?;
    let database = configuration.database.clone();
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let database = database.clone();
        setup.serve_with_options(
            MemoryStore,
            move |_, _, declaration| {
                let database = database.clone();
                let resource = declaration.core_mut().resource(
                    "sqlite",
                    ResourceDeclaration::new(move |_| {
                        let database = database.clone();
                        async move {
                                Store::open(&database).map_err(crate::store::Error::into_runtime)
                            }
                            .boxed_local()
                    })
                    .with_close(|store| {
                        async move { store.close().map_err(crate::store::Error::into_runtime) }
                            .boxed_local()
                    }),
                )?;
                Ok(Service { resource })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

/// Declares the independent memory domain over an existing physical SQLite owner.
///
/// Register [`crate::sqlite_schema`] with that owner before startup. The actual Tembra
/// Resource dependency orders closure; retiring this domain revokes its access without
/// closing the shared database or deleting data owned by any domain.
///
/// # Errors
/// Missing owner readiness, unavailable schema and ordinary resource failures reject startup.
pub fn facet_with_database(handle: &swarmie_sqlite_local::DatabaseHandle) -> Result<NativeFacet> {
    let handle = handle.clone();
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let handle = handle.clone();
        setup.serve_with_options(
            MemoryStore,
            move |_, _, declaration| {
                let database = handle.declare(declaration)?;
                let opening = database.clone();
                let mut domain = ResourceDeclaration::new(move |_| {
                    let database = opening.value().map(|database| (*database).clone());
                    async move {
                        Store::from_database(database?).map_err(crate::store::Error::into_runtime)
                    }
                    .boxed_local()
                })
                .with_close(|store| {
                    async move { store.close().map_err(crate::store::Error::into_runtime) }
                        .boxed_local()
                });
                domain.deps.push(database.dependency());
                let resource = declaration.core_mut().resource("memory-domain", domain)?;
                Ok(Service { resource })
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
}

impl MemoryStoreProvider for Service {
    async fn get(&self, request: GetRequest, _context: Context) -> Result<GetResult> {
        validate(&request.namespace, Some(&request.key))?;
        self.resource
            .value()?
            .get(&request.namespace, &request.key)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn put(&self, request: PutRequest, _context: Context) -> Result<()> {
        validate(&request.namespace, Some(&request.key))?;
        self.resource
            .value()?
            .put(&request.namespace, &request.key, &request.value)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn delete(&self, request: DeleteRequest, _context: Context) -> Result<bool> {
        validate(&request.namespace, Some(&request.key))?;
        self.resource
            .value()?
            .delete(&request.namespace, &request.key)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn list(&self, request: ListRequest, _context: Context) -> Result<Vec<String>> {
        validate(&request.namespace, None)?;
        self.resource
            .value()?
            .list(&request.namespace)
            .map_err(crate::store::Error::into_runtime)
    }
}
