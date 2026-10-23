use crate::{Configuration, FACET_ID, commit::Committer};
use futures::FutureExt;
use std::rc::Rc;
use swarmie_session_contracts::{SessionStore, SessionStoreInput};
use swarmie_session_store_sqlite::SessionStoreHandle;
use swarmie_sqlite_local::DatabaseHandle;
use swarmie_task_contracts::{TaskStorage, TaskStorageInput};
use swarmie_task_session_contracts::*;
use swarmie_task_storage_sqlite::TaskStoreHandle;
use tembra::{core, prelude::*};

/// Assembles ordinary logical task/session providers and their explicit joint capability.
/// Construction performs no I/O; the caller installs the physical owner with all three schemas.
/// Independently configured providers continue using their own constructors.
pub fn facets_with_database(
    handle: &DatabaseHandle,
    configuration: &Configuration,
    driver: Rc<dyn core::Driver>,
) -> Result<Vec<NativeFacet>> {
    let (tasks, task_handle) =
        swarmie_task_storage_sqlite::facet_with_database(handle, configuration.outbox)?;
    let (sessions, session_handle) =
        swarmie_session_store_sqlite::facet_with_database(handle, driver)?;
    let joint = facet_with_database(handle, &task_handle, &session_handle)?;
    Ok(vec![tasks, sessions, joint])
}

/// Declares a joint committer from explicit native domain handles. Startup proves actual owner
/// identity; matching path strings or matching store labels never substitute for that proof.
pub fn facet_with_database(
    handle: &DatabaseHandle,
    tasks: &TaskStoreHandle,
    sessions: &SessionStoreHandle,
) -> Result<NativeFacet> {
    let handle = handle.clone();
    let tasks = tasks.clone();
    let sessions = sessions.clone();
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let handle = handle.clone();
        let tasks = tasks.clone();
        let sessions = sessions.clone();
        setup.serve_with_options(
            TaskSessionCommit,
            move |_, _, declaration| {
                let task_ref = declaration.get(TaskStorage, TaskStorageInput {})?;
                let session_ref = declaration.get(SessionStore, SessionStoreInput {})?;
                let database = handle.declare(declaration)?;
                let opening = database.clone();
                let task_handle = tasks.clone();
                let session_handle = sessions.clone();
                let mut resource = ResourceDeclaration::new(move |_| {
                    let database = opening.clone();
                    let tasks = task_handle.clone();
                    let sessions = session_handle.clone();
                    async move {
                        let database = database.value()?.as_ref().clone();
                        let task = tasks.store()?;
                        let session = sessions.store()?;
                        if !task.shares_database(&database) || !session.shares_database(&database) {
                            return Err(RuntimeError::invalid(
                                "task_session_commit_owner_mismatch",
                                "joint domains do not share the selected physical owner",
                            ));
                        }
                        let identity = TaskSessionCommitIdentity {
                            task_store_id: tasks.identity()?.store_id,
                            session_store_id: sessions.identity()?.store_id,
                        };
                        Ok(Committer {
                            database,
                            tasks: task,
                            identity,
                        })
                    }
                    .boxed_local()
                });
                resource.deps.push(database.dependency());
                resource
                    .deps
                    .push(core::ResourceDependency::Reference(task_ref.core().clone()));
                resource.deps.push(core::ResourceDependency::Reference(
                    session_ref.core().clone(),
                ));
                let resource = declaration
                    .core_mut()
                    .resource("joint-committer", resource)?;
                Ok(Service {
                    resource,
                    sessions: sessions.clone(),
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
    resource: Resource<Committer>,
    sessions: SessionStoreHandle,
}
impl TaskSessionCommitProvider for Service {
    async fn identity(&self, _context: Context) -> Result<TaskSessionCommitIdentity> {
        Ok(self.resource.value()?.identity.clone())
    }
    async fn commit(
        &self,
        request: TaskSessionCommitRequest,
        context: Context,
    ) -> Result<TaskSessionCommitResult> {
        let result = self.resource.value()?.commit(request);
        // SQL completion is authoritative. Existing SessionStore State adoption and managed
        // repair run after it, including uncertain errors; they never rewrite this result.
        self.sessions.reconcile(&context);
        result
    }
}
