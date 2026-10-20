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
/// The supervisor retains the proof outside plugin objects. Plugins are trusted code, but
/// obtaining an administrative Reference does not grant authority to invoke mutations.
/// SQLite belongs to the actual administrative object's lifecycle.
pub struct PluginManager {
    pub(crate) host: Host,
    pub(crate) proof: AdminProof,
    pub(crate) database: PathBuf,
    pub(crate) store: RefCell<Option<core::WeakResource<Store>>>,
    pub(crate) executor: Executor,
    busy: Flag<bool>,
    fresh_session: Flag<bool>,
    // Observations of facets admitted by this supervisor session, never a dispatch table.
    pub(crate) loaded: RefCell<BTreeSet<String>>,
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
    /// Creates fresh capability material for a supervisor to retain outside plugin state.
    pub fn issue_proof() -> AdminProof {
        AdminProof::new(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ))
    }

    /// Installs the compiled administrative Core Plugin and opens its SQLite Resource.
    ///
    /// The Host must be a fresh session whose dynamic facets have not been installed by a
    /// different supervisor. Restart recovery is explicit through [`Self::recover`]. The
    /// retained Driver must be the Driver that owns this Host.
    ///
    /// # Errors
    /// Refuses empty proof material, database failures, executor admission or Tembra startup
    /// failures. Opening an existing database preserves cached package versions and evidence.
    pub async fn start(
        host: Host,
        driver: Rc<dyn core::Driver>,
        database: impl Into<PathBuf>,
        proof: AdminProof,
    ) -> Result<Rc<Self>> {
        if proof.as_str().len() < 32 {
            return Err(ManagerError::Unauthorized);
        }
        let manager = Rc::new(Self {
            host: host.clone(),
            proof,
            database: database.into(),
            store: RefCell::new(None),
            executor: Executor::new(driver, ExecutorOptions::default())?,
            busy: Flag::new(false),
            fresh_session: Flag::new(true),
            loaded: RefCell::new(BTreeSet::new()),
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
        for mut plugin in manager.storage()?.all()? {
            if plugin.desired == DesiredState::Active {
                plugin.observed = ObservedState::Unavailable;
                plugin.activation_id.clear();
                plugin.error =
                    "runtime session restarted; activation has not been recovered".into();
                manager.storage()?.save(&plugin)?;
            }
        }
        for mut session in manager.storage()?.browser_sessions()? {
            session.observed = BrowserObservation::Unknown;
            session.error =
                "supervisor session restarted; browser availability has not been observed".into();
            manager.storage()?.browser_session(&session)?;
        }
        Ok(manager)
    }

    pub(crate) fn authorize(&self, proof: &AdminProof) -> Result<()> {
        let expected = self.proof.as_str().as_bytes();
        let supplied = proof.as_str().as_bytes();
        let mut difference = expected.len() ^ supplied.len();
        for index in 0..expected.len().max(supplied.len()) {
            difference |= usize::from(
                expected.get(index).copied().unwrap_or(0)
                    ^ supplied.get(index).copied().unwrap_or(0),
            );
        }
        if difference != 0 {
            return Err(ManagerError::Unauthorized);
        }
        Ok(())
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
        self.fresh_session.set(false);
        self.exclusive_guard()
    }

    pub(crate) fn recovery_guard(&self) -> Result<OperationGuard<'_>> {
        if !self.fresh_session.replace(false) {
            return Err(ManagerError::RecoveryRequired);
        }
        self.exclusive_guard()
    }

    fn exclusive_guard(&self) -> Result<OperationGuard<'_>> {
        if !self.uncertain.borrow().is_empty() {
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

    /// Records a compiled Core Plugin after the caller has awaited its actual activation.
    ///
    /// # Errors
    /// Refuses invalid authorization or an identity already installed as a dynamic plugin.
    pub async fn record_core(&self, proof: &AdminProof, id: String, version: String) -> Result<()> {
        self.authorize(proof)?;
        if self.storage()?.load(&id)?.is_some() {
            return Err(ManagerError::CorePlugin(id));
        }
        let mut cores = self.cores.borrow_mut();
        if !cores.iter().any(|core| core.plugin_id == id) {
            cores.push(PluginRecord {
                plugin_id: id,
                version,
                provider_id: "compiled".into(),
                kind: PluginKind::Core,
                desired: DesiredState::Active,
                observed: ObservedState::Active,
                errors: vec![],
            });
        }
        drop(cores);
        Ok(())
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

    pub(crate) fn dynamic(&self, id: &str) -> Result<CachedPlugin> {
        if self.cores.borrow().iter().any(|core| core.plugin_id == id) {
            return Err(ManagerError::CorePlugin(id.into()));
        }
        self.storage()?
            .load(id)?
            .ok_or_else(|| ManagerError::NotInstalled(id.into()))
    }
}
