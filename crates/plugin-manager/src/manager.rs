use std::{
    cell::{Cell as Flag, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
};

use swarmie_plugin_contracts::*;
use tembra::{core, prelude::*};
use tembra_executor_js::{Executor, ExecutorOptions};

use crate::{
    ManagerError, Result,
    store::{CachedPlugin, Store},
};

/// Native administrative owner for one fresh Tembra Host session.
///
/// Administration is a local Tembra service for trusted in-Host callers. The transport
/// embedding owns remote connection authority; this service exposes no remote administration.
/// SQLite belongs to the actual administrative object's lifecycle.
pub struct PluginManager {
    pub(crate) host: Host,
    pub(crate) database: PathBuf,
    pub(crate) store: RefCell<Option<core::WeakResource<Store>>>,
    pub(crate) executor: Executor,
    pub(crate) busy: Flag<bool>,
    pub(crate) fresh_session: Flag<bool>,
    pub(crate) restart_required: Flag<bool>,
    pub(crate) reconciling: Flag<bool>,
    pub(crate) composition_selection: RefCell<Option<BTreeSet<String>>>,
    pub(crate) composition_staged: RefCell<BTreeSet<String>>,
    // Real Tembra claims retained only for availability observation, never invocation dispatch.
    pub(crate) references: RefCell<BTreeMap<String, Vec<core::Reference>>>,
    pub(crate) reference_failures: RefCell<BTreeMap<String, BTreeMap<String, String>>>,
    pub(crate) core_facets: RefCell<BTreeMap<String, BTreeSet<String>>>,
    pub(crate) trusted_sources: RefCell<BTreeSet<String>>,
    pub(crate) cores: RefCell<Vec<PluginRecord>>,
    pub(crate) uncertain: RefCell<BTreeMap<String, PluginRecord>>,
}

pub(crate) struct OperationGuard<'a>(&'a Flag<bool>);
impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl PluginManager {
    /// Observes session uncertainty without changing admission or inferring object readiness.
    pub fn requires_recovery(&self) -> bool {
        self.restart_required.get()
            || !self.uncertain.borrow().is_empty()
            || self
                .storage()
                .and_then(|store| store.operations())
                .map_or(true, |operations| {
                    operations.iter().any(|operation| {
                        matches!(
                            operation.status,
                            OperationStatus::Pending | OperationStatus::Running
                        )
                    })
                })
    }

    /// Installs the compiled administrative Core Plugin and opens its SQLite Resource.
    ///
    /// The Host must be a fresh session whose dynamic facets have not been installed by a
    /// different supervisor. Restart recovery is explicit through [`Self::recover`]. The
    /// retained Driver must be the Driver that owns this Host.
    ///
    /// # Errors
    /// Refuses database failures, executor admission or Tembra startup
    /// failures. Opening an existing database preserves cached package versions and evidence.
    pub async fn start(
        host: Host,
        driver: Rc<dyn core::Driver>,
        database: impl Into<PathBuf>,
    ) -> Result<Rc<Self>> {
        let manager = Rc::new(Self {
            host: host.clone(),

            database: database.into(),
            store: RefCell::new(None),
            executor: Executor::new(driver, ExecutorOptions::default())?,
            busy: Flag::new(false),
            fresh_session: Flag::new(true),
            restart_required: Flag::new(false),
            reconciling: Flag::new(false),
            composition_selection: RefCell::new(None),
            composition_staged: RefCell::new(BTreeSet::new()),
            references: RefCell::new(BTreeMap::new()),
            reference_failures: RefCell::new(BTreeMap::new()),
            core_facets: RefCell::new(BTreeMap::from([(
                "swarmie.plugin-manager".into(),
                BTreeSet::from(["swarmie.plugin-manager".into()]),
            )])),
            trusted_sources: RefCell::new(BTreeSet::new()),
            uncertain: RefCell::new(BTreeMap::new()),
            cores: RefCell::new(vec![PluginRecord {
                plugin_id: "swarmie.plugin-manager".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                provider_id: "compiled".into(),
                kind: PluginKind::Core,
                desired: DesiredState::Active,
                observed: ObservedState::Active,
                errors: vec![],
            }]),
        });
        host.install(
            crate::service::facet(Rc::downgrade(&manager))?,
            core::LoadOptions::default(),
        )?
        .await?;
        for mut operation in manager.storage()?.operations()? {
            if matches!(
                operation.status,
                OperationStatus::Pending | OperationStatus::Running
            ) {
                operation.status = OperationStatus::Interrupted;
                operation.error = "previous supervisor session ended before settlement was recorded; external effects are unknown".into();
                manager.storage()?.operation(&operation)?;
            }
        }
        for mut plugin in manager.storage()?.all()? {
            if plugin.desired == DesiredState::Active {
                plugin.observed = ObservedState::Unavailable;
                plugin.activation_id.clear();
                plugin.error =
                    "runtime session restarted; activation has not been recovered".into();
            } else {
                // A fresh Host cannot retain executable work from an earlier supervisor.
                plugin.observed = ObservedState::Installed;
                plugin.activation_id.clear();
            }
            manager.storage()?.save(&plugin)?;
        }
        for mut session in manager.storage()?.browser_sessions()? {
            session.observed = BrowserObservation::Unknown;
            session.error =
                "supervisor session restarted; browser availability has not been observed".into();
            manager.storage()?.browser_session(&session)?;
        }
        Ok(manager)
    }

    pub(crate) fn guard(&self) -> Result<OperationGuard<'_>> {
        if self.busy.get() {
            return Err(ManagerError::Busy);
        }
        if self.storage()?.operations()?.iter().any(|operation| {
            matches!(
                operation.status,
                OperationStatus::Pending | OperationStatus::Running
            )
        }) {
            return Err(ManagerError::RecoveryRequired);
        }
        self.exclusive_guard()
    }

    pub(crate) fn recovery_guard(&self) -> Result<OperationGuard<'_>> {
        self.require_composition_reconciliation()?;
        if !self.fresh_session.replace(false) {
            return Err(ManagerError::RecoveryRequired);
        }
        self.guard()
    }

    fn exclusive_guard(&self) -> Result<OperationGuard<'_>> {
        if self.restart_required.get() || !self.uncertain.borrow().is_empty() {
            return Err(ManagerError::RecoveryRequired);
        }
        if self.busy.replace(true) {
            return Err(ManagerError::Busy);
        }
        Ok(OperationGuard(&self.busy))
    }

    pub(crate) fn storage(&self) -> Result<Rc<Store>> {
        Ok(self
            .store
            .borrow()
            .as_ref()
            .ok_or(ManagerError::Closed)?
            .upgrade()?
            .value()?)
    }

    pub(crate) fn publication_unrecorded(
        &self,
        package: &AcquiredPackage,
        desired: DesiredState,
        cause: ManagerError,
    ) -> ManagerError {
        let error = ManagerError::PublicationUnrecorded {
            plugin_id: package.manifest.id.clone(),
            version: package.manifest.version.clone(),
            cause: Box::new(cause),
        };
        self.uncertain.borrow_mut().insert(
            package.manifest.id.clone(),
            PluginRecord {
                plugin_id: package.manifest.id.clone(),
                version: package.manifest.version.clone(),
                provider_id: package.provenance.provider_id.clone(),
                kind: PluginKind::Dynamic,
                desired,
                observed: ObservedState::Unavailable,
                errors: vec![error.to_string()],
            },
        );
        error
    }

    pub(crate) fn failure_unrecorded(
        &self,
        failure: ManagerError,
        recording: ManagerError,
    ) -> ManagerError {
        self.restart_required.set(true);
        ManagerError::FailureUnrecorded {
            failure: Box::new(failure),
            recording: Box::new(recording),
        }
    }

    pub(crate) fn dynamic(&self, id: &str) -> Result<CachedPlugin> {
        if self.cores.borrow().iter().any(|core| core.plugin_id == id) {
            return Err(ManagerError::CorePlugin(id.into()));
        }
        self.storage()?
            .load(id)?
            .ok_or_else(|| ManagerError::NotInstalled(id.into()))
    }
}
