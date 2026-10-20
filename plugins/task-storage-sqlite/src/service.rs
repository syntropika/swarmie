use futures::FutureExt;
use swarmie_task_contracts::{
    CommitResult, StorageOwner, TaskLookup, TaskPage, TaskQuery, TaskRecord, TaskStorage,
    TaskStorageProvider, TaskTransaction,
};
use tembra::{core, prelude::*};

use crate::{Configuration, FACET_ID, store::Store};

/// Declares the ordinary application Facet and its eagerly opened, Host-owned SQLite Resource.
///
/// # Errors
/// Invalid configuration fails before installation. Filesystem, ownership, schema and database
/// failures reject the Host installation waiter. Resource closure retains every committed row.
pub fn facet(configuration: &Configuration) -> Result<NativeFacet> {
    configuration
        .validate()
        .map_err(|error| RuntimeError::invalid("task_storage_configuration", error.to_string()))?;
    let database = configuration.database.clone();
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let database = database.clone();
        setup.serve_with_options(
            TaskStorage,
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

struct Service {
    resource: Resource<Store>,
}

impl TaskStorageProvider for Service {
    async fn claim_owner(&self, request: StorageOwner, _context: Context) -> Result<()> {
        self.resource
            .value()?
            .claim_owner(request)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn release_owner(&self, request: StorageOwner, _context: Context) -> Result<()> {
        self.resource
            .value()?
            .release_owner(request)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn get(&self, request: TaskLookup, _context: Context) -> Result<Option<TaskRecord>> {
        self.resource
            .value()?
            .get(request)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn scan(&self, query: TaskQuery, _context: Context) -> Result<TaskPage> {
        self.resource
            .value()?
            .scan(query)
            .map_err(crate::store::Error::into_runtime)
    }

    async fn commit(
        &self,
        transaction: TaskTransaction,
        _context: Context,
    ) -> Result<CommitResult> {
        self.resource
            .value()?
            .commit(transaction)
            .map_err(crate::store::Error::into_runtime)
    }
}
