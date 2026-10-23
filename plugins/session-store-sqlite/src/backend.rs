//! Reuses the SessionStore domain within an explicit physical transaction.
pub use crate::database::Store;
use futures::FutureExt;
use rusqlite::Connection;
use std::{cell::RefCell, rc::Rc};
use swarmie_session_contracts::*;
use swarmie_sqlite_local::{DatabaseHandle, Schema};
use tembra::{core, prelude::*};

/// Current session schema; headers, locks and journal configuration belong to its actual owner.
pub fn sqlite_schema() -> Schema {
    Schema {
        domain: "session-store",
        version: 2,
        initialize: crate::schema::initialize_domain,
        validate: crate::schema::validate_domain,
    }
}

/// Shared validation and exact batch retry logic, without committing the caller's transaction.
/// Refuses autocommit or a different session-domain identity before mutation or receipt replay.
pub fn append_in(
    connection: &Connection,
    store_id: &str,
    request: AppendEvents,
) -> Result<AppendReceipt> {
    if connection.is_autocommit() {
        return Err(RuntimeError::invalid(
            "session_store_transaction_required",
            "joint session writes require an active outer transaction",
        ));
    }
    crate::writes::append_in(connection, store_id, request)
}

/// Current authoritative session watermark, including within a joint transaction.
pub fn head(connection: &Connection) -> Result<StoreHead> {
    crate::database::metadata(connection)
}

type Reconcile = Rc<dyn Fn(&Context)>;
/// Actual SessionStore Resource and its existing managed post-commit publication mechanism.
#[derive(Clone, Default)]
pub struct SessionStoreHandle {
    store: Rc<RefCell<Option<Resource<Store>>>>,
    publication: Rc<RefCell<Option<Reconcile>>>,
}
impl SessionStoreHandle {
    pub fn identity(&self) -> Result<SessionStoreIdentity> {
        self.store()?.identity()
    }

    /// Returns the actual domain after its declared dependency becomes ready.
    pub fn store(&self) -> Result<Rc<Store>> {
        self.store
            .borrow()
            .as_ref()
            .ok_or_else(|| {
                RuntimeError::unavailable(
                    "session_store_not_ready",
                    "the shared session facade is not declared",
                )
            })?
            .value()
    }
    /// Reconciles State only after the enclosing SQL transaction has settled. Publication
    /// failures retain a managed repair hint and never replace an authoritative commit receipt.
    pub fn reconcile(&self, context: &Context) {
        if let Some(reconcile) = self.publication.borrow().as_ref() {
            reconcile(context);
        }
    }
}

/// Declares ordinary history reads/writes and State over the selected physical owner.
/// This constructor performs no I/O; its Resource depends on the actual owner Reference.
pub fn facet_with_database(
    handle: &DatabaseHandle,
    driver: Rc<dyn core::Driver>,
) -> Result<(NativeFacet, SessionStoreHandle)> {
    let handle = handle.clone();
    let target = SessionStoreHandle::default();
    let output = target.clone();
    let facet = NativeFacet::new(crate::FACET_ID, vec![], move |_, setup| {
        let handle = handle.clone();
        let target = target.clone();
        let driver = driver.clone();
        let definition = setup.state("committed")?;
        setup.serve_with_options(
            SessionStore,
            move |_, _, declaration| {
                let database = handle.declare(declaration)?;
                let opening = database.clone();
                let mut resource = ResourceDeclaration::new(move |_| {
                    let database = opening.clone();
                    async move { Store::from_database(database.value()?.as_ref().clone()) }
                        .boxed_local()
                })
                .with_close(|store| async move { store.close() }.boxed_local());
                resource.deps.push(database.dependency());
                let resource = declaration.core_mut().resource("sessions", resource)?;
                *target.store.borrow_mut() = Some(resource.clone());
                let service = crate::service::Service::from_resource(
                    resource,
                    declaration,
                    definition.clone(),
                    driver.clone(),
                )?;
                *target.publication.borrow_mut() = Some(service.reconciler());
                Ok(service)
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })?;
    Ok((facet, output))
}
