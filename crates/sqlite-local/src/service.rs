use crate::{Configuration, Database, Schema};
use futures::FutureExt;
use std::{cell::RefCell, rc::Rc};
use tembra::{core, prelude::*};

/// Physical owner identity only; this Contract grants no SQL or domain mutation authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SqliteLocalIdentity {
    pub database_id: String,
}

/// Lifecycle anchor for logical local persistence providers.
#[tembra::contract(local, name = "swarmie.sqlite-local", version = 1)]
pub trait SqliteLocal {
    #[portable]
    async fn identity(&self) -> Result<SqliteLocalIdentity>;
}

/// Native access to the actual Tembra owner Resource. It is valid only in the installation
/// that declared it; retaining this handle never reopens a closed physical database.
#[derive(Clone, Default)]
pub struct DatabaseHandle {
    owner: Rc<RefCell<Option<Resource<Database>>>>,
}
impl DatabaseHandle {
    /// Borrows the live owner after its startup waiter has completed.
    /// # Errors
    /// Reports not-ready or closed lifecycle state instead of opening a second connection.
    pub fn database(&self) -> Result<Database> {
        let owner = self.owner.borrow();
        let owner = owner.as_ref().ok_or_else(|| {
            RuntimeError::unavailable(
                "sqlite_local_not_ready",
                "the database owner has not been declared",
            )
        })?;
        Ok(owner.value()?.as_ref().clone())
    }

    /// Declares a borrowed domain Resource with an explicit dependency on the owner Reference.
    /// Its closer does not close the engine; Tembra closes the actual owner after dependents.
    /// # Errors
    /// Preserves declaration, dependency and startup admission failures.
    pub fn declare(&self, declaration: &mut tembra::Declaration<'_>) -> Result<Resource<Database>> {
        let reference = declaration.get(SqliteLocal, SqliteLocalInput {})?;
        let handle = self.clone();
        let mut resource = ResourceDeclaration::new(move |_| {
            let handle = handle.clone();
            async move { handle.database() }.boxed_local()
        });
        resource.deps.push(core::ResourceDependency::Reference(
            reference.core().clone(),
        ));
        declaration.core_mut().resource("local-sqlite", resource)
    }
}

/// Declares one physical owner. Install it before consumers and await startup before using
/// [`DatabaseHandle::database`] during native administrative bootstrap.
/// # Errors
/// Invalid file/schema/ownership state rejects readiness without deleting existing data.
pub fn facet(
    configuration: &Configuration,
    schemas: Vec<Schema>,
) -> Result<(NativeFacet, DatabaseHandle)> {
    let configuration = configuration.clone();
    let handle = DatabaseHandle::default();
    let target = handle.clone();
    let facet = NativeFacet::new("swarmie.sqlite-local.owner", vec![], move |_, setup| {
        let configuration = configuration.clone();
        let schemas = schemas.clone();
        let target = target.clone();
        setup.serve_with_options(
            SqliteLocal,
            move |_, _, declaration| {
                let configuration = configuration.clone();
                let schemas = schemas.clone();
                let resource = declaration.core_mut().resource(
                    "database",
                    ResourceDeclaration::new(move |_| {
                        let configuration = configuration.clone();
                        let schemas = schemas.clone();
                        async move {
                            Database::open(&configuration, schemas).map_err(RuntimeError::from)
                        }
                        .boxed_local()
                    })
                    .with_close(|database| {
                        async move { database.close().map_err(RuntimeError::from) }.boxed_local()
                    }),
                )?;
                *target.owner.borrow_mut() = Some(resource.clone());
                Ok(Owner { resource })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })?;
    Ok((facet, handle))
}
struct Owner {
    resource: Resource<Database>,
}
impl SqliteLocalProvider for Owner {
    async fn identity(&self, _context: Context) -> Result<SqliteLocalIdentity> {
        Ok(SqliteLocalIdentity {
            database_id: self.resource.value()?.identity().to_owned(),
        })
    }
}
