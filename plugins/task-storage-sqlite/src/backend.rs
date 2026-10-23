//! Explicit native task-domain operations for a shared physical owner.
pub use crate::store::{Error, Store};
use futures::FutureExt;
use rusqlite::Connection;
use std::{cell::RefCell, rc::Rc};
use swarmie_sqlite_local::{DatabaseHandle, Schema};
use swarmie_task_contracts::{TaskOutboxLimits, TaskStorage};
use tembra::{core, prelude::*};

/// Current task schema; file-level configuration belongs to the physical owner.
pub fn sqlite_schema() -> Schema {
    Schema {
        domain: "task-storage",
        version: 3,
        initialize,
        validate,
    }
}
fn initialize(connection: &Connection) -> Result<()> {
    crate::schema::initialize_domain(connection).map_err(Error::into_runtime)
}
fn validate(connection: &Connection) -> Result<()> {
    crate::schema::validate_domain(connection).map_err(Error::into_runtime)?;
    let required:bool=connection.query_row("SELECT \"notnull\" FROM pragma_table_info('task_outbox_consumers') WHERE name='owner_facet'",[],|r|r.get(0)).map_err(Error::from).map_err(Error::into_runtime)?;
    if !required {
        return Err(RuntimeError::invalid(
            "task_storage_unsupported",
            "consumer owners must be required",
        ));
    }
    Ok(())
}

/// Native reference to the one TaskStorage Resource, sharing scheduler claim and sequence cache.
#[derive(Clone, Default)]
pub struct TaskStoreHandle {
    store: Rc<RefCell<Option<Resource<Store>>>>,
}
impl TaskStoreHandle {
    /// Returns the actual open domain after its declared dependency is ready.
    pub fn identity(&self) -> Result<swarmie_task_contracts::TaskStorageIdentity> {
        self.store()?.store_identity().map_err(Error::into_runtime)
    }

    pub fn store(&self) -> Result<Rc<Store>> {
        self.store
            .borrow()
            .as_ref()
            .ok_or_else(|| {
                RuntimeError::unavailable(
                    "task_storage_not_ready",
                    "the shared task facade is not declared",
                )
            })?
            .value()
    }
}

/// Declares the ordinary TaskStorage Contract over one explicit physical owner. Constructor
/// validation performs no I/O; startup enforces domain schema and Resource readiness.
pub fn facet_with_database(
    handle: &DatabaseHandle,
    outbox: TaskOutboxLimits,
) -> Result<(NativeFacet, TaskStoreHandle)> {
    outbox.validate()?;
    let handle = handle.clone();
    let target = TaskStoreHandle::default();
    let output = target.clone();
    let facet = NativeFacet::new(crate::FACET_ID, vec![], move |_, setup| {
        let handle = handle.clone();
        let target = target.clone();
        setup.serve_with_options(
            TaskStorage,
            move |_, _, declaration| {
                let database = handle.declare(declaration)?;
                let opening = database.clone();
                let mut resource = ResourceDeclaration::new(move |_| {
                    let database = opening.clone();
                    async move {
                        Store::from_database(database.value()?.as_ref().clone(), outbox)
                            .map_err(Error::into_runtime)
                    }
                    .boxed_local()
                })
                .with_close(|store| {
                    async move { store.close().map_err(Error::into_runtime) }.boxed_local()
                });
                resource.deps.push(database.dependency());
                let resource = declaration.core_mut().resource("tasks", resource)?;
                *target.store.borrow_mut() = Some(resource.clone());
                Ok(crate::service::Service::from_resource(resource))
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })?;
    Ok((facet, output))
}

/// Refuses nonterminal history progress under a cancelled or settling ancestor. TaskStorage's
/// ordinary ownership/relationship checks still run first in the same transaction.
pub fn validate_history_ancestors(
    connection: &Connection,
    records: &[swarmie_task_contracts::TaskRecord],
) -> Result<()> {
    use swarmie_task_contracts::TaskStatus;
    for record in records
        .iter()
        .filter(|record| !matches!(record.status, TaskStatus::Terminal | TaskStatus::Completing))
    {
        let mut parent = record.parent.clone();
        let mut visited = std::collections::BTreeSet::from([record.id.clone()]);
        while let Some(id) = parent {
            if !visited.insert(id.clone()) {
                return Err(RuntimeError::invalid(
                    "task_storage_invalid",
                    "task ownership contains a cycle",
                ));
            }
            let ancestor = crate::store::read_record(connection, &id)
                .map_err(Error::into_runtime)?
                .ok_or_else(|| {
                    RuntimeError::unavailable(
                        "task_storage_corrupt",
                        "an ownership ancestor does not exist",
                    )
                })?
                .0;
            if ancestor.cancellation_requested
                || matches!(
                    ancestor.status,
                    TaskStatus::Terminal | TaskStatus::Completing
                )
            {
                return Err(RuntimeError::invalid(
                    "task_session_commit_cancelled",
                    "an ancestor no longer admits task progress with history",
                ));
            }
            parent = ancestor.parent;
        }
    }
    Ok(())
}

/// Reads the exact bounded task facts written by one logical task commit. The caller must
/// retain the same physical transaction; this never acknowledges or delivers a consumer page.
///
/// # Errors
/// Missing, oversized, malformed or misindexed batches are explicit corruption failures.
pub fn committed_facts_in(
    connection: &Connection,
    task_store_id: &str,
    sequence: u64,
) -> Result<Vec<swarmie_task_contracts::TaskOutboxEvent>> {
    crate::outbox::committed_facts_in(connection, task_store_id, sequence)
        .map_err(Error::into_runtime)
}
